//! The fuzz oracle, run against every known-good fixture: a harness that
//! fails on valid files is worse than no harness. (This is a normal test,
//! not a fuzz target — `cargo test` inside this crate runs it.)

#[path = "../fuzz_targets/common/mod.rs"]
mod common;

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures")
}

#[test]
fn every_fixture_passes_the_oracle() {
    let mut checked = 0;
    let mut opus = 0;
    let dir = fixtures();
    let names = std::fs::read_dir(&dir)
        .expect("fixtures directory")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            !n.starts_with('.')
                && !n.contains("make")
                && !n.ends_with(".ref")
                && !n.ends_with(".rng")
                && !n.starts_with("README")
        })
        .collect::<Vec<_>>();
    assert!(!names.is_empty(), "no fixtures found in {}", dir.display());
    for name in &names {
        let bytes = std::fs::read(dir.join(name)).expect(name);
        // Every fixture is meant to decode — a rejection here means
        // the oracle's shape bounds (or a decoder) drifted.
        match vieww_audio::codec::decode(&bytes) {
            Ok(d) => {
                common::check(&d);
                checked += 1;
                opus += usize::from(name.ends_with(".opus"));
            }
            Err(e) => panic!("{name} stopped decoding: {e}"),
        }
    }
    assert!(opus >= 7, "the Opus fixtures went missing: {opus}");
    assert!(checked >= 10, "only {checked} fixtures checked");
}
