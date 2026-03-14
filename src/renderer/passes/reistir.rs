use super::{PrepareContext, RecordContext, RenderPass};

#[derive(Debug, Default)]
pub struct ReSTIRPass;

impl RenderPass for ReSTIRPass {
    fn label(&self) -> &'static str {
        "reistir"
    }

    fn prepare(&mut self, context: &PrepareContext<'_>) {
        assert!(
            context.reistir_ready(),
            "ReSTIRPass requires a valid ReSTIR bind group before record()",
        );
    }

    fn record(&self, context: &mut RecordContext<'_>) {
        let [groups_x, groups_y] = context.groups();
        let mut pass = context
            .encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("reistir-spatial-pass"),
                timestamp_writes: None,
            });
        pass.set_pipeline(context.reistir_pipeline);
        pass.set_bind_group(0, context.reistir_bind_group, &[]);
        pass.dispatch_workgroups(groups_x, groups_y, 1);
    }
}
