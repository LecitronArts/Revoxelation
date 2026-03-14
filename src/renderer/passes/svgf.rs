use super::{PrepareContext, RecordContext, RenderPass};

#[derive(Debug, Default)]
pub struct SvgfPass;

impl RenderPass for SvgfPass {
    fn label(&self) -> &'static str {
        "svgf"
    }

    fn prepare(&mut self, context: &PrepareContext<'_>) {
        assert!(
            context.svgf_ready(),
            "SvgfPass requires init/resolve bind groups and enough atrous bind groups",
        );
    }

    fn record(&self, context: &mut RecordContext<'_>) {
        let [groups_x, groups_y] = context.groups();

        {
            let mut pass = context
                .encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("svgf-init-pass"),
                    timestamp_writes: None,
                });
            pass.set_pipeline(context.svgf_init_pipeline);
            pass.set_bind_group(0, context.svgf_init_bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }

        for bind_group in context
            .svgf_atrous_bind_groups
            .iter()
            .take(context.svgf_passes)
        {
            let mut pass = context
                .encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("svgf-atrous-pass"),
                    timestamp_writes: None,
                });
            pass.set_pipeline(context.svgf_atrous_pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }

        {
            let mut pass = context
                .encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("svgf-resolve-pass"),
                    timestamp_writes: None,
                });
            pass.set_pipeline(context.svgf_resolve_pipeline);
            pass.set_bind_group(0, context.svgf_resolve_bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
    }
}
