//! The manifest language — §5.1's "verify the exact revision, tokenizer,
//! licence and redistribution requirements before packaging", as data the
//! runtime refuses to run without.
//!
//! A [`Manifest`] is one weights file: its family, size, quantisation, the
//! exact repository revision it was cut from, the SHA-256 of the bytes
//! themselves, the licence those bytes carry (an APK is a redistribution,
//! so the licence is a shipping fact, not a nicety), and where the exact
//! bytes came from. A [`TokenizerManifest`] is the same discipline for the
//! tokenizer the weights expect. Neither downloads anything: fetching is a
//! developer-side act with `curl` in `docs/models.md`, because the app
//! itself holds no network permission at all and must never grow one
//! silently.
//!
//! The three manifests the plan's §5.2 table asks to compare are committed
//! beside this crate (`models/manifests/`) and embedded here, so the
//! runtime and the benchmark agree on what "Q2_K" means without a path
//! argument — while the *weights* stay outside the repository, verified at
//! load time against the digest embedded from these very files.
//!
//! # The honest quant labels
//!
//! The plan's table names Q2_K, Q3_K and Q4_K. The files it compares are
//! the Q3_K_M and Q4_K_M variants of those families (the community's
//! standard mobile cuts) — each manifest's `file` field says exactly which,
//! because a label that quietly meant a different file would be a small
//! lie with a benchmark built on it.

use u_core::Error;

/// The quantisation formats the plan's §5.2 table names, with the effective
/// weight bits it lists — because "Q2_K" is not two bits, and the number
/// that matters when sizing a phone's memory is the honest one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Quant {
    /// ~2.625 effective bits — the aggressive low-memory experiment.
    Q2K,
    /// ~3.4375 effective bits — the memory/quality compromise.
    Q3K,
    /// ~4.5 effective bits — the quality-oriented mobile baseline.
    Q4K,
}

impl Quant {
    /// The plan's own effective-bits column, verbatim.
    #[must_use]
    pub const fn effective_bits(self) -> f32 {
        match self {
            Self::Q2K => 2.625,
            Self::Q3K => 3.4375,
            Self::Q4K => 4.5,
        }
    }

    /// The format's own name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Q2K => "Q2_K",
            Self::Q3K => "Q3_K",
            Self::Q4K => "Q4_K",
        }
    }

    /// The short name a runtime's own name is built from.
    #[must_use]
    pub const fn stem(self) -> &'static str {
        match self {
            Self::Q2K => "q2k",
            Self::Q3K => "q3k",
            Self::Q4K => "q4k",
        }
    }
}

/// A weights file, described — everything §5.1 asks to verify, as fields
/// that must all be answered before anything loads.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    /// The family — "qwen3".
    pub family: String,
    /// The parameter count, as the label it ships under.
    pub parameters: String,
    /// The quantisation.
    pub quant: Quant,
    /// The exact repository revision the weights were cut from.
    pub revision: String,
    /// The SHA-256 of the weights file, hex — what a download is checked
    /// against, byte for byte, before anything runs.
    pub sha256: String,
    /// The licence the weights carry.
    pub license: String,
    /// The weights file name, as it is expected on disk.
    pub file: String,
    /// Where the exact bytes came from — provenance a person can re-check.
    #[serde(default)]
    pub url: String,
    /// The human-facing source, quoted in settings-grade honesty.
    #[serde(default)]
    pub source: String,
}

impl Manifest {
    /// A human-readable one-liner, the shape a benchmark report prints.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{}-{} {} ({} effective bits, {})",
            self.family,
            self.parameters,
            self.quant.label(),
            self.quant.effective_bits(),
            self.revision
        )
    }

    /// The runtime's own name for this model — what settings shows and a
    /// report prints, stable because it is built from the manifest, not
    /// from whichever file happened to be on disk. The parameter count is
    /// lowercased ("qwen3-0.6b-q2k", not "qwen3-0.6B-q2k"): a name is an
    /// identifier, and `Qwen`'s static names agree with it by test.
    #[must_use]
    pub fn runtime_name(&self) -> String {
        format!(
            "{}-{}-{}",
            self.family,
            self.parameters.to_lowercase(),
            self.quant.stem()
        )
    }

    /// The manifests embedded from `models/manifests/`, in the plan's §5.2
    /// comparison order. These are the *only* models the runtime will
    /// claim to know about; anything else on disk is not U's.
    pub const EMBEDDED_JSON: [&'static str; 3] = [
        include_str!("../../../models/manifests/qwen3-0.6b-q2k.json"),
        include_str!("../../../models/manifests/qwen3-0.6b-q3k.json"),
        include_str!("../../../models/manifests/qwen3-0.6b-q4k.json"),
    ];

    /// [`Self::EMBEDDED_JSON`], parsed — the plan's three candidates,
    /// present wherever this crate is, with no file system involved.
    ///
    /// # Panics
    /// Only if a committed manifest is malformed — a build-time fact the
    /// tests pin, not a runtime condition a phone could reach.
    #[must_use]
    pub fn embedded() -> Vec<Self> {
        Self::EMBEDDED_JSON
            .iter()
            .map(|raw| serde_json::from_str(raw).expect("a committed manifest parses"))
            .collect()
    }

    /// Read a manifest from a JSON file.
    ///
    /// # Errors
    /// [`Error::NoRuntime`] when the file is missing or not a manifest —
    /// a manifest that cannot be read is a model that cannot run, and the
    /// caller falls back rather than guesses at weights.
    pub fn load(path: &std::path::Path) -> Result<Self, Error> {
        let raw =
            std::fs::read_to_string(path).map_err(|_| Error::NoRuntime("the model manifest"))?;
        serde_json::from_str(&raw).map_err(|_| Error::NoRuntime("a readable model manifest"))
    }
}

/// The tokenizer a family of weights expects, described with the same
/// discipline — §5.1 names the tokenizer as its own thing to verify, so
/// it gets its own manifest rather than a footnote in the weights'.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TokenizerManifest {
    /// The family the tokenizer belongs to.
    pub family: String,
    /// The exact repository revision the tokenizer was cut from.
    pub revision: String,
    /// The SHA-256 of the tokenizer file, hex.
    pub sha256: String,
    /// The licence the tokenizer carries.
    pub license: String,
    /// The tokenizer file name, as it is expected on disk.
    pub file: String,
    /// Where the exact bytes came from.
    #[serde(default)]
    pub url: String,
    /// The human-facing source.
    #[serde(default)]
    pub source: String,
}

impl TokenizerManifest {
    /// The embedded provenance of the Qwen3 tokenizer, from
    /// `models/manifests/tokenizer.json`.
    pub const EMBEDDED_JSON: &str = include_str!("../../../models/manifests/tokenizer.json");

    /// [`Self::EMBEDDED_JSON`], parsed.
    ///
    /// # Panics
    /// Only if the committed manifest is malformed — pinned by tests.
    #[must_use]
    pub fn embedded() -> Self {
        serde_json::from_str(Self::EMBEDDED_JSON).expect("the committed tokenizer manifest parses")
    }

    /// Read a tokenizer manifest from a JSON file.
    ///
    /// # Errors
    /// [`Error::NoRuntime`] when missing or unreadable, exactly as
    /// [`Manifest::load`].
    pub fn load(path: &std::path::Path) -> Result<Self, Error> {
        let raw = std::fs::read_to_string(path)
            .map_err(|_| Error::NoRuntime("the tokenizer manifest"))?;
        serde_json::from_str(&raw).map_err(|_| Error::NoRuntime("a readable tokenizer manifest"))
    }
}

/// The SHA-256 of a file, streamed — a phone never holds a second copy of
/// a 400 MB weight file just to hash it.
///
/// # Errors
/// [`Error::NoRuntime`] when the file cannot be read.
#[cfg(feature = "model")]
pub fn digest_of(path: &std::path::Path) -> Result<String, Error> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|_| Error::NoRuntime("the weights file"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let read = file
            .read(&mut buf)
            .map_err(|_| Error::NoRuntime("the weights file"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let out = hasher.finalize();
    Ok(out.iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether the file at `path` matches the digest — the §5.1 gate a load
/// refuses to skip, and the §10 gate a release cannot pass without.
///
/// # Errors
/// [`Error::NoRuntime`] when the file is missing, [`Error::Unverified`]
/// when it is present and wrong — different facts a caller acts on
/// differently: one fetches, the other deletes and fetches.
#[cfg(feature = "model")]
pub fn verify_digest(path: &std::path::Path, expected: &str) -> Result<(), Error> {
    let got = digest_of(path)?;
    // Constant-time comparison is ceremony for a digest that is public
    // metadata; equality is the property that matters here.
    if got.eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err(Error::Unverified {
            what: "the weights",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_manifests_are_the_plan_table_and_parse() {
        let all = Manifest::embedded();
        assert_eq!(
            all.iter().map(|m| m.quant).collect::<Vec<_>>(),
            vec![Quant::Q2K, Quant::Q3K, Quant::Q4K],
            "§5.2's comparison order"
        );
        for m in &all {
            assert_eq!(m.family, "qwen3");
            assert_eq!(m.parameters, "0.6B");
            assert_eq!(m.sha256.len(), 64, "a digest is a digest, not a prefix");
            assert_eq!(m.revision.len(), 40, "a git revision, not a tag");
            assert_eq!(m.license, "Apache-2.0");
            assert!(!m.file.is_empty());
            assert!(m.url.starts_with("https://"), "provenance is a URL");
        }
        // The honest-label rule: Q3K/Q4K say which variant file they mean.
        assert!(Manifest::embedded()[1].file.contains("Q3_K_M"));
        assert!(Manifest::embedded()[2].file.contains("Q4_K_M"));
    }

    #[test]
    fn the_runtime_name_is_built_from_the_manifest_not_the_disk() {
        let q2k = &Manifest::embedded()[0];
        assert_eq!(q2k.runtime_name(), "qwen3-0.6b-q2k");
        assert!(q2k.summary().contains("Q2_K"));
        assert!(
            q2k.summary().contains("2.625"),
            "effective bits, not the label"
        );
    }

    #[test]
    fn the_tokenizer_provenance_is_complete() {
        let t = TokenizerManifest::embedded();
        assert_eq!(t.family, "qwen3");
        assert_eq!(t.file, "tokenizer.json");
        assert_eq!(t.sha256.len(), 64);
        assert_eq!(t.license, "Apache-2.0");
    }

    #[test]
    fn a_missing_manifest_is_no_runtime_not_a_crash() {
        let missing = std::path::Path::new("/nonexistent/manifest.json");
        assert_eq!(
            Manifest::load(missing),
            Err(Error::NoRuntime("the model manifest"))
        );
        assert_eq!(
            TokenizerManifest::load(missing),
            Err(Error::NoRuntime("the tokenizer manifest"))
        );
    }

    #[cfg(feature = "model")]
    #[test]
    fn a_digest_check_tells_present_and_wrong_from_absent() {
        let dir = std::env::temp_dir().join("u-ml-runtime-digest-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("tiny.gguf");
        std::fs::write(&file, b"weights, allegedly").unwrap();
        // Absent is NoRuntime…
        let absent = dir.join("no-such.gguf");
        assert_eq!(
            verify_digest(&absent, "00"),
            Err(Error::NoRuntime("the weights file"))
        );
        // …present and wrong is Unverified…
        assert_eq!(
            verify_digest(
                &file,
                "0000000000000000000000000000000000000000000000000000000000000000"
            ),
            Err(Error::Unverified {
                what: "the weights"
            })
        );
        // …and present and right is nothing at all.
        let right = digest_of(&file).unwrap();
        assert!(verify_digest(&file, &right).is_ok());
        // Case and surrounding whitespace in the manifest are tolerated.
        assert!(verify_digest(&file, &format!("  {}  ", right.to_uppercase())).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
