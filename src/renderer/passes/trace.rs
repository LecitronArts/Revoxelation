use super::{PrepareContext, RecordContext, RenderPass};

#[derive(Debug, Default)]
pub struct TracePass;

impl RenderPass for TracePass {
    fn label(&self) -> &'static str {
        "trace"
    }

    fn prepare(&mut self, context: &PrepareContext<'_>) {
        assert!(
            context.trace_ready(),
            "TracePass requires a valid trace bind group before record()",
        );
    }

    fn record(&self, context: &mut RecordContext<'_>) {
        let [groups_x, groups_y] = context.groups();
        let mut pass = context
            .encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("trace-compute-pass"),
                timestamp_writes: None,
            });
        pass.set_pipeline(context.trace_pipeline);
        pass.set_bind_group(0, context.trace_bind_group, &[]);
        pass.dispatch_workgroups(groups_x, groups_y, 1);
    }
}
