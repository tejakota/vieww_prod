//! Print the HLSL naga generates for a library shader's entry points —
//! what the D3D12 backend hands FXC. `cargo run -p vieww-shaders --example
//! dump_hlsl -- post` (or scene, clear, ... ; `all` lists every shader).
fn main() {
    use vieww_shaders::{compiler::ParsedShader, library};
    let all = [
        ("clear", library::CLEAR_WGSL),
        ("solid", library::SOLID_WGSL),
        ("scene", library::SCENE_WGSL),
        ("post", library::POST_WGSL),
        ("gradient", library::GRADIENT_WGSL),
        ("image", library::IMAGE_WGSL),
        ("text", library::TEXT_WGSL),
        ("blur", library::BLUR_WGSL),
        ("shadow", library::SHADOW_WGSL),
        ("mask", library::MASK_WGSL),
        ("composite", library::COMPOSITE_WGSL),
        ("unlit3d", library::UNLIT3D_WGSL),
        ("lit3d", library::LIT3D_WGSL),
    ];
    let which = std::env::args().nth(1).unwrap_or_else(|| "post".into());
    for (name, src) in all {
        if which != "all" && which != name {
            continue;
        }
        let p = ParsedShader::parse(src).expect("library shaders parse");
        for (entry, stage) in p.entry_points() {
            println!("// ===== {name}.wgsl {entry} =====");
            print!(
                "{}",
                p.to_hlsl(&entry, stage).expect("library shaders translate")
            );
        }
    }
}
