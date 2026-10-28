//! The one place SPIR-V translation is configured.
//!
//! Shared, by `#[path]`, between [`crate::compiler`] (runtime translation,
//! behind the `compiler` feature) and this crate's `build.rs` (which bakes the
//! built-in library to SPIR-V at compile time — see [`crate::baked`]). One
//! file, so a shader baked at build time and the same shader translated at
//! run time cannot be configured differently; `baked`'s tests check the bytes
//! agree.
//!
//! Only `naga` types appear here, so `build.rs` can include it without the
//! rest of the crate.

/// Parse and validate `wgsl` exactly as every vieww shader is.
///
/// `Capabilities::all()`: capabilities only gate optional hardware features,
/// and naga's validator still rejects anything no target can run.
pub(crate) fn parse_and_validate(
    wgsl: &str,
) -> Result<(naga::Module, naga::valid::ModuleInfo), String> {
    let module = naga::front::wgsl::parse_str(wgsl).map_err(|e| format!("parse: {e}"))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| format!("validate: {e}"))?;
    Ok((module, info))
}

/// SPIR-V writer options.
///
/// Bounds-checked image loads: naga's default leaves an out-of-range
/// `textureLoad` unchecked, which is undefined behaviour that software
/// rasterisers happen to forgive and real drivers do not. Out of range reads
/// as transparent — what the CPU compositor does.
pub(crate) fn spirv_options() -> naga::back::spv::Options<'static> {
    naga::back::spv::Options {
        bounds_check_policies: naga::proc::BoundsCheckPolicies {
            index: naga::proc::BoundsCheckPolicy::Restrict,
            buffer: naga::proc::BoundsCheckPolicy::Restrict,
            image_load: naga::proc::BoundsCheckPolicy::ReadZeroSkipWrite,
            binding_array: naga::proc::BoundsCheckPolicy::Restrict,
        },
        ..naga::back::spv::Options::default()
    }
}

/// The whole module as SPIR-V words. SPIR-V carries every entry point, so
/// there is one blob per WGSL module, not one per entry point.
/// SPIR-V for **one** entry point: a module holding only `entry_point` of
/// `stage` and what it reaches.
///
/// One module per entry point, not one per file. A WGSL file with several
/// fragment entry points (post.wgsl: `fs_main`, `fs_blur`, `fs_no_blur`)
/// becomes, written whole, a SPIR-V module declaring an output variable at
/// `Location 0` once per entry point. That is valid SPIR-V, and lavapipe,
/// MoltenVK and the validation layers accept it, but Qualcomm's Adreno
/// drivers fail it at pipeline creation ("Failed to link shaders",
/// `VK_ERROR_UNKNOWN`) — every GPU pass on the Adreno 506, 650 and 840 in
/// Device Farm run 40720367260, after post.wgsl gained its D3D12 entry
/// points. wgpu writes one module per entry point for the same reason.
pub(crate) fn write_spirv_entry(
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    entry_point: &str,
    stage: naga::ShaderStage,
) -> Result<Vec<u32>, String> {
    let mut writer = naga::back::spv::Writer::new(&spirv_options()).map_err(|e| e.to_string())?;
    let mut spirv = Vec::new();
    let pipeline = naga::back::spv::PipelineOptions {
        shader_stage: stage,
        entry_point: entry_point.to_owned(),
    };
    writer
        .write(module, info, Some(&pipeline), &None, &mut spirv)
        .map_err(|e| e.to_string())?;
    Ok(spirv)
}
