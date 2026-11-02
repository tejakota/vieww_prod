//! AAX, refused by design — and named here, as the codecs name what they
//! refuse.
//!
//! Avid Audio eXtension is the plugin format of Pro Tools, and it is the
//! one major format this crate will never wrap, for reasons that are not
//! engineering:
//!
//! * **the SDK is proprietary.** AAX headers and libraries are
//!   distributed by Avid under a licence and NDA that forbid
//!   redistribution and reimplementation. Every other format this crate
//!   wraps publishes its ABI: CLAP's headers are MIT and vendored into
//!   this repository's plugin-check cache; VST3's interfaces are in
//!   Steinberg's public SDK under GPLv3/proprietary dual terms and the
//!   ABI is documented; LV2's headers are ISC and transcribed in
//!   `lv2/sys.rs`; even AUv3's surface is the Objective-C runtime plus
//!   published headers (see [`auv3`](crate::auv3), which is honest
//!   about never having run). AAX has no published ABI to transcribe.
//! * **the binaries are cryptographically signed.** Pro Tools loads an
//!   AAX plugin only after Avid's signing tool (PACE/wrap, tied to a
//!   paid developer account) has sealed it. An unsigned reimplementation
//!   would be refused by the host at load, whatever its correctness —
//!   so "written and type-checked" (this crate's AUv3 standard) is not
//!   even a meaningful milestone for AAX; nothing short of the
//!   licensed SDK and signer can run there.
//!
//! This is the same discipline as the AAC decoder's refusal of HE-AAC
//! SBR/PS by *name* with a precise error, or the Opus decoder's
//! `REFUSED_PAYLOAD`: what is not done is stated, not hidden. A host or
//! tool asking this crate for AAX gets [`REFUSED`] — a `&'static str`
//! reason, not a silent empty format list.
//!
//! What a developer who needs Pro Tools support does today: build the
//! CLAP or VST3 wrapper (they are complete, tested, and validated
//! against the formats' own validators in CI), and use Avid's own SDK
//! outside this crate — or a wrapper host that bridges VST3 into Pro
//! Tools, which is Avid's documented escape hatch and none of this
//! repository's business.
//!
//! If Avid ever publishes the ABI (or a Rust-friendly signing path),
//! this module is where the wrapper would go; the refusal below is the
//! marker that it has not.

/// The named refusal: what [`refused`] returns and what every mention of
/// AAX in this crate means.
pub const REFUSED: &str = concat!(
    "AAX is refused by design: Avid's SDK is proprietary and binary-signed; ",
    "there is no published ABI to transcribe"
);

/// The AAX refusal as a function, so tooling and hosts get one shape.
///
/// ```
/// # use vieww_audio_plugin::aax;
/// assert!(aax::refused().contains("AAX"));
/// ```
#[must_use]
pub fn refused() -> &'static str {
    REFUSED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refusal_is_named_and_precise() {
        assert!(REFUSED.contains("AAX"));
        assert!(REFUSED.contains("proprietary"));
        assert_eq!(refused(), REFUSED);
    }
}
