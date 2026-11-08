//! Compiles the OCCT bridge's source with the facts export
//! (`export/geospec_occt_facts_export.cpp` includes the bridge) against the
//! static OCCT prefix and links its toolkits, as the OCCT crate's build script
//! does for the bridge alone: C++17, no floating-point contraction, the
//! toolkits dependents-first on Linux.

// A build script reports a missing or malformed prefix by panicking.
#![allow(clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-env-changed=GEOSPEC_OCCT_PREFIX");
    println!("cargo:rerun-if-changed=export/geospec_occt_facts_export.cpp");
    println!("cargo:rerun-if-changed=export/geospec_occt_facts_export.h");
    println!("cargo:rerun-if-changed=../../occt/bridge/geospec_occt_bridge.cpp");
    println!("cargo:rerun-if-changed=../../occt/bridge/geospec_occt_bridge.h");

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repository = manifest
        .ancestors()
        .nth(5)
        .expect("the facts OCCT adapter must stay under the engine package");
    let prefix = env::var_os("GEOSPEC_OCCT_PREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repository.join("node_modules/.cache/geospec-engine-native/occt/install")
        });
    let include = prefix.join("include/opencascade");
    let library = prefix.join("lib");
    if !include.join("Standard_Version.hxx").is_file() || !library.is_dir() {
        panic!(
            "OCCT static prefix is missing at {}. Run native/occt/build-occt.sh first.",
            prefix.display()
        );
    }
    println!("cargo:rerun-if-changed={}", include.display());
    let mut toolkits = configured_toolkits(&prefix);
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        toolkits = dependents_first(&prefix, toolkits);
    }
    for toolkit in &toolkits {
        println!(
            "cargo:rerun-if-changed={}",
            library.join(format!("lib{toolkit}.a")).display()
        );
    }
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .warnings(true)
        .flag("-Werror=deprecated-declarations")
        // The bridge is a main file included here: its document type holds
        // its own anonymous-namespace types.
        .flag_if_supported("-Wno-subobject-linkage")
        .flag(format!("-ffile-prefix-map={}=occt", prefix.display()))
        // Every operation rounds, as in the OCCT prefix and the bridge.
        .flag("-ffp-contract=off")
        .include(&include)
        .include("../../occt/bridge")
        .file("export/geospec_occt_facts_export.cpp")
        .compile("geospec_occt_facts_export");
    println!("cargo:rustc-link-search=native={}", library.display());
    for toolkit in toolkits {
        println!("cargo:rustc-link-lib=static={toolkit}");
    }
}

fn configured_toolkits(prefix: &Path) -> Vec<String> {
    let configuration = prefix.join("lib/cmake/opencascade/OpenCASCADEConfig.cmake");
    println!("cargo:rerun-if-changed={}", configuration.display());
    let contents = fs::read_to_string(&configuration)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", configuration.display()));
    let marker = "set (OpenCASCADE_LIBRARIES ";
    let start = contents.find(marker).unwrap_or_else(|| {
        panic!(
            "{} has no OpenCASCADE library list",
            configuration.display()
        )
    }) + marker.len();
    let end = contents[start..]
        .find(')')
        .map(|offset| start + offset)
        .unwrap_or_else(|| {
            panic!(
                "{} has an unterminated library list",
                configuration.display()
            )
        });
    let toolkits: Vec<_> = contents[start..end]
        .split(|character: char| character == ';' || character.is_whitespace())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    if toolkits.is_empty() || !toolkits.iter().any(|name| name == "TKDESTEP") {
        panic!(
            "{} does not contain the TKDESTEP closure",
            configuration.display()
        );
    }
    toolkits
}

/// `toolkits` reordered so every toolkit precedes the toolkits it links, from
/// the installed CMake targets' `INTERFACE_LINK_LIBRARIES`; ties keep the
/// configured order.
fn dependents_first(prefix: &Path, toolkits: Vec<String>) -> Vec<String> {
    let directory = prefix.join("lib/cmake/opencascade");
    let mut uses: Vec<Vec<usize>> = vec![Vec::new(); toolkits.len()];
    let entries = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("OCCT CMake directory entry").path();
        let is_targets = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("Targets.cmake"));
        if !is_targets {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let mut rest = contents.as_str();
        while let Some(start) = rest.find("set_target_properties(") {
            rest = &rest[start + "set_target_properties(".len()..];
            let end = rest
                .find("\n)")
                .unwrap_or_else(|| panic!("{} has an unterminated target block", path.display()));
            let block = &rest[..end];
            rest = &rest[end..];
            let Some(owner) = block
                .split_whitespace()
                .next()
                .and_then(|name| toolkits.iter().position(|toolkit| toolkit == name))
            else {
                continue;
            };
            let marker = "INTERFACE_LINK_LIBRARIES \"";
            let Some(libraries) = block.find(marker).map(|at| &block[at + marker.len()..]) else {
                continue;
            };
            let libraries = &libraries[..libraries.find('"').unwrap_or(libraries.len())];
            for library in libraries.split(';') {
                if let Some(used) = toolkits.iter().position(|toolkit| toolkit == library) {
                    if used != owner && !uses[owner].contains(&used) {
                        uses[owner].push(used);
                    }
                }
            }
        }
    }
    // Kahn's algorithm: a toolkit is placed once every toolkit using it is.
    let mut users = vec![0_usize; toolkits.len()];
    for used in uses.iter().flatten() {
        users[*used] += 1;
    }
    let mut placed = vec![false; toolkits.len()];
    let mut order = Vec::with_capacity(toolkits.len());
    while order.len() < toolkits.len() {
        let next = (0..toolkits.len())
            .find(|&index| !placed[index] && users[index] == 0)
            .unwrap_or_else(|| panic!("OCCT toolkit dependencies form a cycle: {toolkits:?}"));
        placed[next] = true;
        for used in &uses[next] {
            users[*used] -= 1;
        }
        order.push(next);
    }
    order
        .into_iter()
        .map(|index| toolkits[index].clone())
        .collect()
}
