fn main() {
    use vieww_shaders::{compiler::ParsedShader, library, ShaderStage};
    let which = std::env::args().nth(1).unwrap_or_else(|| "scene".into());
    let src = match which.as_str() { "post" => library::POST_WGSL, "clear" => library::CLEAR_WGSL, _ => library::SCENE_WGSL };
    let p = ParsedShader::parse(src).unwrap();
    print!("{}", p.to_hlsl("fs_main", ShaderStage::Fragment).unwrap());
}
