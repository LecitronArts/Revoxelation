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
    let _: fn(&mut revoxelation::renderer::Renderer, u64) -> anyhow::Result<()> =
        revoxelation::renderer::submit_frame;
}
