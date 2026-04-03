use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    for shader in shader_sources() {
        println!("cargo:rerun-if-changed={shader}");
    }
    // Also rerun if the shared include changes.
    println!("cargo:rerun-if-changed=shaders/common.glsl");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must exist"));
    let compiler = shaderc::Compiler::new().expect("shader compiler should initialize");

    for shader in shader_sources() {
        compile_shader(&compiler, shader, &out_dir);
    }
}

fn shader_sources() -> [&'static str; 16] {
    [
        "shaders/chunk_mesh.vert",
        "shaders/chunk_mesh.frag",
        "shaders/chunk_cull.comp",
        "shaders/hiz_generate.comp",
        "shaders/egui.vert",
        "shaders/egui.frag",
        "shaders/meshlet_cull.comp",
        "shaders/meshlet_draw.vert",
        "shaders/meshlet_draw.frag",
        "shaders/meshlet.task",
        "shaders/meshlet.mesh",
        "shaders/shadow_depth.vert",
        "shaders/ssao_compute.comp",
        "shaders/ssao_blur.comp",
        "shaders/sky.vert",
        "shaders/sky.frag",
    ]
}

fn compile_shader(compiler: &shaderc::Compiler, shader: &str, out_dir: &Path) {
    let source = fs::read_to_string(shader).unwrap_or_else(|err| {
        panic!("failed to read shader source {shader}: {err}");
    });
    let kind = shader_kind(shader);
    let mut options = shaderc::CompileOptions::new().expect("shaderc options should initialize");
    // Target SPIR-V 1.5 (Vulkan 1.2) to enable subgroup operations.
    options.set_target_spirv(shaderc::SpirvVersion::V1_5);
    options.set_target_env(
        shaderc::TargetEnv::Vulkan,
        shaderc::EnvVersion::Vulkan1_2 as u32,
    );
    // POLISH-07: Enable SPIR-V performance optimization.
    options.set_optimization_level(shaderc::OptimizationLevel::Performance);
    // POLISH-04: Include callback for #include "common.glsl" (relative to shaders/).
    let shader_dir = Path::new("shaders")
        .canonicalize()
        .expect("shaders/ directory must exist");
    options.set_include_callback(move |name, _include_type, _source_file, _depth| {
        let include_path = shader_dir.join(name);
        match fs::read_to_string(&include_path) {
            Ok(content) => Ok(shaderc::ResolvedInclude {
                resolved_name: include_path.to_string_lossy().into_owned(),
                content,
            }),
            Err(err) => Err(format!(
                "failed to read include file {}: {err}",
                include_path.display()
            )),
        }
    });
    let artifact = compiler
        .compile_into_spirv(&source, kind, shader, "main", Some(&options))
        .unwrap_or_else(|err| panic!("failed to compile shader {shader}: {err}"));
    let file_name = Path::new(shader)
        .file_name()
        .expect("shader source should have a file name");
    let output_path = out_dir.join(format!("{}.spv", file_name.to_string_lossy()));
    fs::write(&output_path, artifact.as_binary_u8()).unwrap_or_else(|err| {
        panic!(
            "failed to write compiled shader {}: {err}",
            output_path.display()
        );
    });
}

fn shader_kind(shader: &str) -> shaderc::ShaderKind {
    if shader.ends_with(".vert") {
        shaderc::ShaderKind::Vertex
    } else if shader.ends_with(".frag") {
        shaderc::ShaderKind::Fragment
    } else if shader.ends_with(".comp") {
        shaderc::ShaderKind::Compute
    } else if shader.ends_with(".task") {
        shaderc::ShaderKind::Task
    } else if shader.ends_with(".mesh") {
        shaderc::ShaderKind::Mesh
    } else {
        panic!("unsupported shader extension for {shader}");
    }
}
