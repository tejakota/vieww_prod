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
    let mut refused = 0;
    let dir = fixtures();
    let names = std::fs::read_dir(&dir)
        .expect("fixtures directory")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            !n.starts_with('.')
                && !n.contains("make")
                && !n.ends_with(".ref")
                && !n.starts_with("README")
        })
        .collect::<Vec<_>>();
    assert!(!names.is_empty(), "no fixtures found in {}", dir.display());
    for name in &names {
        let bytes = std::fs::read(dir.join(name)).expect(name);
        // The Opus fixtures are the one deliberate exception: the container
        // parses purely, the SILK/CELT payload is refused by name and rides
        // the platform bridge instead (AU-06; the bridge end-to-end is
        // tested in the crate's own suites, with a fake PlatformCodec).
        // Here, without a bridge installed, the refusal itself is the
        // contract — the exact stable string the fallback matches on.
        if name.ends_with(".opus") {
            assert_eq!(
                vieww_audio::codec::decode(&bytes).err(),
                Some(vieww_audio::codec::CodecError::Unsupported(
                    vieww_audio::codec::opus::REFUSED_PAYLOAD
                )),
                "{name}: Opus refuses its payload by name"
            );
            refused += 1;
            continue;
        }
        // Every other fixture is meant to decode — a rejection here means
        // the oracle's shape bounds (or a decoder) drifted.
        match vieww_audio::codec::decode(&bytes) {
            Ok(d) => {
                common::check(&d);
                checked += 1;
            }
            Err(e) => panic!("{name} stopped decoding: {e}"),
        }
    }
    assert!(refused >= 2, "the Opus fixtures went missing: {refused}");
    assert!(checked >= 10, "only {checked} fixtures checked");
}
