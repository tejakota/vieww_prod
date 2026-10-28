//! The built-in library, already translated to SPIR-V when this crate was
//! compiled.
//!
//! **Why.** Translating WGSL at run time means shipping a shader compiler in
//! every app: `naga` was the second-largest crate in a stripped vieww window
//! (1.1 MiB of machine code of 8.1, measured with `cargo bloat`, 2026-10-15),
//! and translating the scene and post shaders cost ~4.5 ms of start-up on a
//! desktop CPU before the first frame. Every shader the renderer itself uses
//! is one of this crate's `shaders/*.wgsl`, known at compile time, so
//! `build.rs` translates them then — with the same options as the runtime
//! compiler, from one shared file (`spirv_options.rs`) — and a Vulkan backend
//! looks them up here.
//!
//! Runtime translation remains, behind the `compiler` feature, for WGSL that
//! is *not* built in: a user's compute kernel, hot reload, MSL and HLSL.
//!
//! This module never needs `naga`, which is the point.

/// A shader stage, without `naga`'s type (which this module must not need).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Vertex,
    Fragment,
    Compute,
}

/// One built-in WGSL module and its SPIR-V, one module per entry point.
#[derive(Debug)]
pub struct Baked {
    /// The file stem under `shaders/`: `"scene"`, `"post"`, …
    pub name: &'static str,
    /// The WGSL it was translated from — the same text as the matching
    /// [`crate::library`] constant.
    pub wgsl: &'static str,
    /// Each entry point with its stage and its own SPIR-V module (only that
    /// entry point and what it reaches; see `spirv_options::write_spirv_entry`
    /// for why a module never holds more than one).
    pub entries: &'static [(&'static str, Stage, &'static [u32])],
}

impl Baked {
    /// Whether this module has an entry point `name` of `stage`.
    #[must_use]
    pub fn has_entry_point(&self, name: &str, stage: Stage) -> bool {
        self.spirv(name, stage).is_some()
    }

    /// The SPIR-V module for entry point `name` of `stage`.
    #[must_use]
    pub fn spirv(&self, name: &str, stage: Stage) -> Option<&'static [u32]> {
        self.entries
            .iter()
            .find(|&&(n, s, _)| n == name && s == stage)
            .map(|&(_, _, words)| words)
    }
}

include!(concat!(env!("OUT_DIR"), "/baked_spirv.rs"));

/// Every baked module, in file-name order.
#[must_use]
pub fn all() -> &'static [Baked] {
    BAKED
}

/// The baked module whose WGSL is exactly `wgsl`, if it is a built-in.
///
/// Matched on the text itself, not on which constant it came from, so a
/// caller holding [`crate::library::SCENE_WGSL`] and one holding the same
/// text from anywhere else both find it; a pointer comparison is tried first,
/// which is the common case and costs nothing.
#[must_use]
pub fn find(wgsl: &str) -> Option<&'static Baked> {
    BAKED
        .iter()
        .find(|b| std::ptr::eq(b.wgsl, wgsl))
        .or_else(|| BAKED.iter().find(|b| b.wgsl == wgsl))
}

/// SPIR-V for `entry_point` of `stage` in the built-in module `wgsl`.
///
/// `None` if `wgsl` is not a built-in or has no such entry point.
#[must_use]
pub fn spirv(wgsl: &str, entry_point: &str, stage: Stage) -> Option<&'static [u32]> {
    find(wgsl).and_then(|b| b.spirv(entry_point, stage))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library;

    #[test]
    fn every_library_constant_is_baked() {
        for wgsl in [
            library::CLEAR_WGSL,
            library::SOLID_WGSL,
            library::SCENE_WGSL,
            library::POST_WGSL,
            library::UNLIT3D_WGSL,
            library::LIT3D_WGSL,
            library::GRADIENT_WGSL,
            library::IMAGE_WGSL,
            library::TEXT_WGSL,
            library::BLUR_WGSL,
            library::SHADOW_WGSL,
            library::MASK_WGSL,
            library::COMPOSITE_WGSL,
        ] {
            let baked = find(wgsl).expect("a library shader is baked");
            for &(entry, _, words) in baked.entries {
                assert!(words.len() > 5, "{} {entry} is empty", baked.name);
                assert_eq!(
                    words[0], 0x0723_0203,
                    "{} {entry} lacks the SPIR-V magic",
                    baked.name
                );
            }
            assert!(
                baked.has_entry_point("vs_main", Stage::Vertex),
                "{}",
                baked.name
            );
            assert!(
                baked.has_entry_point("fs_main", Stage::Fragment),
                "{}",
                baked.name
            );
        }
        assert_eq!(
            all().len(),
            13,
            "a new shaders/*.wgsl needs a library constant and this list"
        );
    }

    #[test]
    fn text_equal_to_a_built_in_finds_it_too() {
        let copy = library::SCENE_WGSL.to_string();
        assert_eq!(find(&copy).map(|b| b.name), Some("scene"));
        assert!(find("fn main() {}").is_none());
    }

    #[test]
    fn a_wrong_entry_point_or_stage_is_none() {
        assert!(spirv(library::SCENE_WGSL, "vs_main", Stage::Vertex).is_some());
        assert!(spirv(library::SCENE_WGSL, "vs_main", Stage::Fragment).is_none());
        assert!(spirv(library::SCENE_WGSL, "nope", Stage::Vertex).is_none());
    }

    /// The baked bytes are what the runtime compiler produces now — the
    /// shared options file makes drift unlikely; this makes it impossible
    /// to miss. Every entry point, not only `vs_main`.
    #[cfg(feature = "compiler")]
    #[test]
    fn baked_spirv_equals_runtime_translation() {
        for baked in all() {
            let parsed = crate::ParsedShader::parse(baked.wgsl).expect("parses");
            for &(entry, stage, words) in baked.entries {
                let stage = match stage {
                    Stage::Vertex => crate::ShaderStage::Vertex,
                    Stage::Fragment => crate::ShaderStage::Fragment,
                    Stage::Compute => crate::ShaderStage::Compute,
                };
                let runtime = parsed.to_spirv(entry, stage).expect("translates");
                assert_eq!(
                    runtime, words,
                    "{} {entry} differs from runtime translation",
                    baked.name
                );
            }
        }
    }

    /// The Adreno rule: a SPIR-V module carries exactly one entry point
    /// (`OpEntryPoint` appears once). post.wgsl has four; written whole, its
    /// module failed pipeline creation on every Adreno phone.
    #[test]
    fn every_baked_module_has_exactly_one_entry_point() {
        const OP_ENTRY_POINT: u32 = 15;
        for baked in all() {
            for &(entry, _, words) in baked.entries {
                let mut at = 5; // past the header
                let mut count = 0;
                while at < words.len() {
                    let len = (words[at] >> 16) as usize;
                    if words[at] & 0xFFFF == OP_ENTRY_POINT {
                        count += 1;
                    }
                    at += len.max(1);
                }
                assert_eq!(count, 1, "{} {entry}: {count} OpEntryPoint", baked.name);
            }
        }
        let post = find(crate::library::POST_WGSL).expect("post is baked");
        assert!(post.entries.len() >= 4, "post.wgsl's D3D12 entry points");
    }
}
