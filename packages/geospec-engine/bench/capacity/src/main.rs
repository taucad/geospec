//! CAP-0 capacity bench CLI.
//!
//! ```text
//! geospec-capacity list
//! geospec-capacity generate <fixture-id> <dir>
//! geospec-capacity probe <dir> [--limits <json>] [--budget <units>] [--claim <id>]
//! ```
//!
//! `probe` admits every subject of `<dir>/fixture.json` into one backend-free
//! engine, runs the fixture's claims and prints one JSON measurement record.
//! It never decides pass or fail for the qualification: the runner classifies.
mod fixtures;
mod gltf;
mod probe;

use std::{path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("list") => {
            for id in fixtures::FIXTURES {
                println!("{id}");
            }
            ExitCode::SUCCESS
        }
        Some("generate") if args.len() == 3 => {
            match fixtures::generate(&args[1], Path::new(&args[2])) {
                Ok(manifest) => {
                    println!(
                        "{}",
                        serde_json::to_string(&manifest).expect("manifest encodes")
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("generate {}: {error}", args[1]);
                    ExitCode::from(2)
                }
            }
        }
        Some("probe") if args.len() >= 2 => {
            let mut limits = None;
            let mut budget = 8_000_000_u64;
            let mut only = None;
            let mut rest = args[2..].iter();
            while let Some(flag) = rest.next() {
                let value = rest.next();
                match (flag.as_str(), value) {
                    ("--limits", Some(value)) => limits = Some(value.clone()),
                    ("--claim", Some(value)) => only = Some(value.clone()),
                    ("--budget", Some(value)) => match value.parse() {
                        Ok(value) => budget = value,
                        Err(_) => return usage(),
                    },
                    _ => return usage(),
                }
            }
            match probe::run(
                Path::new(&args[1]),
                limits.as_deref(),
                budget,
                only.as_deref(),
            ) {
                Ok(record) => {
                    println!(
                        "{}",
                        serde_json::to_string(&record).expect("record encodes")
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("probe: {error}");
                    ExitCode::from(2)
                }
            }
        }
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: geospec-capacity list | generate <id> <dir> | probe <dir> [--limits <json>] [--budget <units>] [--claim <id>]");
    ExitCode::from(64)
}
