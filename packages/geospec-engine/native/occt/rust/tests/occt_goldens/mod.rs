//! The OCCT crate's floating-point goldens: one observation, written by
//! `examples/write_occt_goldens.rs` to `tests/fixtures/occt-goldens.json` and
//! read back by the tests that compare against it. Every target builds OCCT
//! and the bridge without floating-point contraction, so one file holds for
//! every producer; a target that measures otherwise fails its tests.
#![allow(dead_code)]

use geospec_engine_native_occt::{BrepSubject, Document, ShapeParts};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The written file, relative to the crate manifest.
pub const PATH: &str = "tests/fixtures/occt-goldens.json";
pub const SCHEMA: &str = "geospec-occt-goldens-v1";

/// (fixture, performance-lab fixture id) of the mesh authority report soups.
pub const SOUP_FIXTURES: [(&str, &str); 4] = [
    ("component-interference/original.step", "m3-control-step"),
    ("nominal-bore-void/guide.step", "bore-guide-step"),
    ("ap242-box.step", "box-step"),
    ("parallel-plane-distance-source.step", "m3-pmi-step"),
];
/// The M3 campaign's `toHaveCenterOfMass` subject.
pub const CENTER_OF_MASS_FIXTURE: &str = "component-interference/original.step";
pub const NIST_FIXTURE: &str = "nist-pmi-bspline.step";
pub const FREE_EDGES_FIXTURE: &str = "bounds-free-edges.step";

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Goldens {
    pub schema: String,
    pub generator: String,
    /// `document.tessellate(0.1, 0.5)` triangles of the NIST PMI B-spline part.
    pub nist_tessellation_triangles: usize,
    /// Whole-shape `AddOptimal` bounds of the free edge and vertex fixture, as f64 bits.
    pub free_edge_bounds: BoundsBits,
    /// Whole-shape centre of mass of the campaign subject, as f64 bits.
    pub center_of_mass: PointBits,
    pub report_soups: Vec<Soup>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoundsBits {
    pub min: [String; 3],
    pub max: [String; 3],
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointBits {
    pub fixture: String,
    pub bits: [String; 3],
    /// Shortest round-trip decimals of `bits`, for readers; the tests compare bits.
    pub decimal: [String; 3],
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Soup {
    pub fixture: String,
    pub id: String,
    pub triangles: usize,
    /// FNV-1a 64 of the position bits, then the triangle indices, little-endian.
    pub fnv1a64: String,
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(manifest().join("tests/fixtures").join(name))
        .expect("retained fixture must be readable")
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn bits(value: f64) -> String {
    format!("{:#018x}", value.to_bits())
}

pub fn bits3(values: [f64; 3]) -> [String; 3] {
    values.map(bits)
}

/// One fresh document's report soup, so no triangulation is reused.
pub fn report_soup(name: &str) -> (usize, String) {
    let document = Document::from_step(&fixture(name)).unwrap();
    let mesh = BrepSubject::reported_mesh(&document).unwrap();
    let positions = mesh.positions.iter().flatten().map(|value| value.to_bits());
    let bytes = positions.flat_map(u64::to_le_bytes).chain(
        mesh.triangles
            .iter()
            .flatten()
            .flat_map(|index| index.to_le_bytes()),
    );
    let hash = bytes.fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    (mesh.triangles.len(), format!("{hash:#018x}"))
}

/// What this build measures, in the written form.
pub fn observe() -> Goldens {
    let nist = Document::from_step(&fixture(NIST_FIXTURE)).unwrap();
    let bounds = Document::from_step(&fixture(FREE_EDGES_FIXTURE))
        .unwrap()
        .reported_shape_parts(ShapeParts::BOUNDS)
        .unwrap()
        .bounds;
    let center = Document::from_step(&fixture(CENTER_OF_MASS_FIXTURE))
        .unwrap()
        .reported_shape()
        .unwrap()
        .center_of_mass;
    Goldens {
        schema: SCHEMA.to_owned(),
        generator: "pnpm nx run geospec-engine:generate-occt-goldens".to_owned(),
        nist_tessellation_triangles: nist.tessellate(0.1, 0.5).unwrap().triangles.len(),
        free_edge_bounds: BoundsBits {
            min: bits3(bounds.min),
            max: bits3(bounds.max),
        },
        center_of_mass: PointBits {
            fixture: CENTER_OF_MASS_FIXTURE.to_owned(),
            bits: bits3(center),
            decimal: center.map(|value| format!("{value:?}")),
        },
        report_soups: SOUP_FIXTURES
            .iter()
            .map(|&(fixture, id)| {
                let (triangles, fnv1a64) = report_soup(fixture);
                Soup {
                    fixture: fixture.to_owned(),
                    id: id.to_owned(),
                    triangles,
                    fnv1a64,
                }
            })
            .collect(),
    }
}

/// The written goldens.
pub fn pinned() -> Goldens {
    let text = std::fs::read_to_string(manifest().join(PATH))
        .expect("tests/fixtures/occt-goldens.json must be generated");
    let goldens: Goldens = serde_json::from_str(&text).expect("occt-goldens.json must parse");
    assert_eq!(goldens.schema, SCHEMA);
    goldens
}

/// The generator's exact bytes for `goldens`.
pub fn render(goldens: &Goldens) -> String {
    serde_json::to_string_pretty(goldens).unwrap() + "\n"
}

/// Writes this build's observation to `PATH`.
pub fn write() -> PathBuf {
    let path = manifest().join(PATH);
    std::fs::write(&path, render(&observe())).unwrap();
    path
}

pub fn parse_bits(value: &str) -> u64 {
    u64::from_str_radix(value.trim_start_matches("0x"), 16).expect("hex f64 bits")
}
