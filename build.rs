use std::{env, fs, path::{Path, PathBuf}};

fn main() {
    for shader in shader_sources() {
        println!("cargo:rerun-if-changed={shader}");
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must exist"));
    let compiler = shaderc::Compiler::new().expect("shader compiler should initialize");

    for shader in shader_sources() {
        compile_shader(&compiler, shader, &out_dir);
    }
}

fn shader_sources() -> [&'static str; 5] {
    [
        "shaders/chunk_mesh.vert",
        "shaders/chunk_mesh.frag",
        "shaders/chunk_cull.comp",
        "shaders/egui.vert",
        "shaders/egui.frag",
    ]
}

fn compile_shader(compiler: &shaderc::Compiler, shader: &str, out_dir: &Path) {
    let source = fs::read_to_string(shader).unwrap_or_else(|err| {
        panic!("failed to read shader source {shader}: {err}");
    });
    let kind = shader_kind(shader);
    let artifact = compiler
        .compile_into_spirv(&source, kind, shader, "main", None)
        .unwrap_or_else(|err| panic!("failed to compile shader {shader}: {err}"));
    let file_name = Path::new(shader)
        .file_name()
        .expect("shader source should have a file name");
    let output_path = out_dir.join(format!("{}.spv", file_name.to_string_lossy()));
    fs::write(&output_path, artifact.as_binary_u8()).unwrap_or_else(|err| {
        panic!("failed to write compiled shader {}: {err}", output_path.display());
    });
}

fn shader_kind(shader: &str) -> shaderc::ShaderKind {
    if shader.ends_with(".vert") {
        shaderc::ShaderKind::Vertex
    } else if shader.ends_with(".frag") {
        shaderc::ShaderKind::Fragment
    } else if shader.ends_with(".comp") {
        shaderc::ShaderKind::Compute
    } else {
        panic!("unsupported shader extension for {shader}");
    }
}
