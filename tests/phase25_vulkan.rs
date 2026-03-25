// Phase 2.5 compile-check stubs.
// These tests do NOT create a Vulkan instance (no GPU required in CI).
// They exist solely to confirm the public API compiles.

#[test]
fn renderer_module_types_compile() {
    let _: fn() -> () = || {
        let _: revoxelation::renderer::device::DeviceContext;
        let _: revoxelation::renderer::swapchain::SwapchainContext;
        let _: revoxelation::renderer::frame::FrameData;
        let _: revoxelation::renderer::Renderer;
    };
}

#[test]
fn submit_frame_fn_exists() {
    let _: fn(&mut revoxelation::renderer::Renderer, u64, &revoxelation::renderer::camera::CameraUniforms) -> anyhow::Result<()> =
        revoxelation::renderer::submit_frame;
}

#[test]
fn staging_buffer_api_compiles() {
    let _: fn(&mut revoxelation::renderer::Renderer, u64) -> anyhow::Result<
        revoxelation::renderer::StagingBuffer,
    > = revoxelation::renderer::StagingBuffer::new;

    let _: fn(&mut revoxelation::renderer::StagingBuffer, &[u8]) =
        revoxelation::renderer::StagingBuffer::write;

    let _: fn(
        &revoxelation::renderer::StagingBuffer,
        &revoxelation::renderer::Renderer,
        ash::vk::Buffer,
        u64,
    ) -> anyhow::Result<()> = revoxelation::renderer::StagingBuffer::copy_to;

    let _: fn(
        revoxelation::renderer::StagingBuffer,
        &mut revoxelation::renderer::Renderer,
    ) -> anyhow::Result<()> = revoxelation::renderer::StagingBuffer::destroy;
}

#[test]
fn egui_backend_type_compiles() {
    let _: fn(
        &mut revoxelation::renderer::Renderer,
    ) -> anyhow::Result<revoxelation::renderer::egui_backend::EguiAshBackend> =
        revoxelation::renderer::egui_backend::EguiAshBackend::new;
}
