//! Writes `tests/fixtures/occt-goldens.json`, the only producer of the OCCT
//! crate's floating-point goldens. Run it through
//! `pnpm nx run geospec-engine:generate-occt-goldens`.

#[path = "../tests/occt_goldens/mod.rs"]
mod occt_goldens;

fn main() {
    let path = occt_goldens::write();
    println!("wrote {}", path.display());
}
