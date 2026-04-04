//! Extracted egui debug panel drawing functions (REFAC-09).
//!
//! Each function is a pure UI draw call — it takes specific data references
//! and renders to an egui context. No side effects beyond egui state.

use crate::renderer::config::RenderConfig;
use crate::renderer::lighting::{FogType, LightingState};
use crate::renderer::perf_counters::GpuPerfCounters;
use crate::renderer::shadow::{CASCADE_COUNT, ShadowConfig};
use crate::renderer::sky::{AtmosphereModel, SkyConfig};
use crate::renderer::ssao::{SsaoAlgorithm, SsaoConfig};

/// Draw the main "Debug" info window — FPS, frame counter, camera position,
/// chunk stats, and meshlet LOD statistics.
pub(crate) fn draw_debug_window(
    ctx: &egui::Context,
    perf: &GpuPerfCounters,
    frame_index: u64,
    camera_pos: glam::Vec3,
) {
    egui::Window::new("Debug").show(ctx, |ui| {
        // FPS counter.
        let fps = if perf.frame_time_ms > 0.0 {
            1000.0 / perf.frame_time_ms
        } else {
            0.0
        };
        ui.label(format!("FPS: {:.0} ({:.1}ms)", fps, perf.frame_time_ms));
        ui.label(format!("Frame: {}", frame_index));
        // Camera position display.
        ui.label(format!(
            "Pos: ({:.1}, {:.1}, {:.1})",
            camera_pos.x, camera_pos.y, camera_pos.z
        ));
        ui.separator();
        ui.label(format!(
            "Chunks: {}/{} | Slots: {}/{} | Frame: {:.1}ms",
            perf.visible_chunks,
            perf.total_chunks,
            perf.total_chunks,
            perf.chunk_capacity,
            perf.frame_time_ms
        ));

        // Meshlet LOD statistics (MSHL-05).
        ui.separator();
        ui.label(format!(
            "Meshlets: {} (LOD0: {}, LOD1: {})",
            perf.total_meshlets, perf.lod0_meshlets, perf.lod1_meshlets
        ));
        ui.label(format!(
            "Visible: {} | Cull rate: {:.1}%",
            perf.visible_meshlets,
            perf.meshlet_cull_rate * 100.0
        ));
    });
}

/// Draw the "Meshlet Culling" controls window — backface/frustum/Hi-Z toggles,
/// meshlet rendering toggle, and SSE threshold slider.
pub(crate) fn draw_meshlet_culling_window(ctx: &egui::Context, config: &mut RenderConfig) {
    egui::Window::new("Meshlet Culling").show(ctx, |ui| {
        ui.checkbox(&mut config.meshlet_cull_backface, "Backface culling");
        ui.checkbox(&mut config.meshlet_cull_frustum, "Frustum culling");
        ui.checkbox(&mut config.meshlet_cull_hiz, "Hi-Z occlusion culling");
        ui.checkbox(&mut config.use_meshlet_rendering, "Meshlet rendering");
        ui.separator();
        ui.label("SSE threshold (LOD)");
        ui.add(egui::Slider::new(&mut config.sse_threshold, 0.1..=16.0).text("px"));
    });
}

/// Draw the "Lighting" controls window — sun elevation/azimuth, intensity,
/// ambient intensity, and time of day sliders.
///
/// Manual sliders are disabled when the day-night cycle is active.
pub(crate) fn draw_lighting_window(ctx: &egui::Context, ls: &mut LightingState) {
    egui::Window::new("Lighting").show(ctx, |ui| {
        // H5 fix: Disable manual sliders when day-night cycle overrides them.
        let cycle_active = ls.use_day_night_cycle;
        ui.add_enabled_ui(!cycle_active, |ui| {
            if cycle_active {
                ui.label(
                    egui::RichText::new("(Overridden by day-night cycle)")
                        .italics()
                        .weak(),
                );
            }
            ui.label("Sun Elevation");
            ui.add(egui::Slider::new(&mut ls.sun_elevation, 0.0..=90.0).text("deg"));
            ui.label("Sun Azimuth");
            ui.add(egui::Slider::new(&mut ls.sun_azimuth, 0.0..=360.0).text("deg"));
            ui.label("Sun Intensity");
            ui.add(egui::Slider::new(&mut ls.sun_intensity, 0.0..=5.0));
            ui.label("Ambient Intensity");
            ui.add(egui::Slider::new(&mut ls.ambient_intensity, 0.0..=1.0));
            ui.label("Time of Day");
            ui.add(egui::Slider::new(&mut ls.time_of_day, 0.0..=1.0));
        });
    });
}

/// Draw the "Shadows" configuration window — enable toggle, split lambda,
/// bias constant/slope, debug cascade colours, and resolution info.
pub(crate) fn draw_shadow_window(
    ctx: &egui::Context,
    sc: &mut ShadowConfig,
    shadow_resolution: Option<u32>,
) {
    egui::Window::new("Shadows").show(ctx, |ui| {
        ui.checkbox(&mut sc.enabled, "Shadows enabled");
        ui.separator();
        ui.label("Split Lambda");
        ui.add(egui::Slider::new(&mut sc.split_lambda, 0.0..=1.0).text("lambda"));
        ui.label("Bias Constant");
        ui.add(egui::Slider::new(&mut sc.bias_constant, 0.0..=5.0));
        ui.label("Bias Slope");
        ui.add(egui::Slider::new(&mut sc.bias_slope, 0.0..=5.0));
        ui.checkbox(&mut sc.debug_cascades, "Debug cascade colors");
        if let Some(res) = shadow_resolution {
            ui.separator();
            ui.label(format!("Resolution: {}x{}", res, res));
            ui.label(format!("Cascades: {}", CASCADE_COUNT));
        }
    });
}

/// Draw the "SSAO" configuration window — enable toggle, algorithm selector,
/// AO radius/intensity/sample count, half-resolution toggle, debug view,
/// and current AO buffer size.
pub(crate) fn draw_ssao_window(
    ctx: &egui::Context,
    ssao_cfg: &mut SsaoConfig,
    ao_size: Option<(u32, u32)>,
) {
    egui::Window::new("SSAO").show(ctx, |ui| {
        ui.checkbox(&mut ssao_cfg.enabled, "SSAO enabled");
        ui.separator();

        // Algorithm selector.
        let algo_label = ssao_cfg.algorithm.as_str();
        egui::ComboBox::from_label("Algorithm")
            .selected_text(algo_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut ssao_cfg.algorithm, SsaoAlgorithm::Gtao, "GTAO");
                ui.selectable_value(&mut ssao_cfg.algorithm, SsaoAlgorithm::HbaoPlus, "HBAO+");
                ui.selectable_value(
                    &mut ssao_cfg.algorithm,
                    SsaoAlgorithm::ClassicSsao,
                    "Classic SSAO",
                );
            });

        ui.label("AO Radius");
        ui.add(egui::Slider::new(&mut ssao_cfg.radius, 0.1..=2.0).text("world"));
        ui.label("AO Intensity");
        ui.add(egui::Slider::new(&mut ssao_cfg.intensity, 0.0..=3.0));
        ui.label("Sample Count");
        ui.add(egui::Slider::new(&mut ssao_cfg.sample_count, 4..=64));
        ui.checkbox(&mut ssao_cfg.half_resolution, "Half resolution");
        ui.checkbox(&mut ssao_cfg.debug_view, "Debug AO view");

        if let Some((w, h)) = ao_size {
            ui.separator();
            ui.label(format!("AO size: {}x{}", w, h));
        }
    });
}

/// Draw the "Sky & Atmosphere" controls window — enable toggle, atmosphere
/// model selector, turbidity slider, and sun disk size slider.
pub(crate) fn draw_sky_window(ctx: &egui::Context, sky_config: &mut SkyConfig) {
    egui::Window::new("Sky & Atmosphere").show(ctx, |ui| {
        ui.checkbox(&mut sky_config.enabled, "Sky enabled");
        ui.separator();

        // Atmosphere model selector.
        let model_label = sky_config.atmosphere_model.as_str();
        egui::ComboBox::from_label("Atmosphere Model")
            .selected_text(model_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut sky_config.atmosphere_model,
                    AtmosphereModel::Preetham,
                    "Preetham",
                );
                ui.selectable_value(
                    &mut sky_config.atmosphere_model,
                    AtmosphereModel::HosekWilkie,
                    "Hosek-Wilkie",
                );
            });

        ui.label("Turbidity");
        ui.add(egui::Slider::new(&mut sky_config.turbidity, 1.0..=10.0));
        ui.label("Sun Disk Size");
        ui.add(
            egui::Slider::new(&mut sky_config.sun_angular_radius, 0.001..=0.05).text("rad"),
        );
    });
}

/// Draw the "Day-Night Cycle" controls window — cycle toggle, time display,
/// time slider, day speed slider, pause toggle, and lighting summary.
pub(crate) fn draw_day_night_window(ctx: &egui::Context, ls: &mut LightingState) {
    egui::Window::new("Day-Night Cycle").show(ctx, |ui| {
        ui.checkbox(&mut ls.use_day_night_cycle, "Use day-night cycle");
        ui.separator();

        // Time display.
        let time_str = ls.day_night.time_as_hhmm();
        let elevation = ls.day_night.sun_elevation();
        ui.label(format!("Time: {} | Sun elev: {:.2}", time_str, elevation));
        ui.separator();

        // Time of day slider.
        ui.label("Time of Day");
        let time_labels = "Midnight          Dawn          Noon          Dusk";
        ui.label(time_labels);
        ui.add(egui::Slider::new(&mut ls.day_night.time_of_day, 0.0..=1.0));

        // Day speed slider.
        ui.label("Day Speed (seconds per game day)");
        ui.add(egui::Slider::new(&mut ls.day_night.day_speed, 60.0..=3600.0).text("sec"));

        // Pause toggle.
        ui.checkbox(&mut ls.day_night.paused, "Paused");

        // Lighting summary.
        ui.separator();
        ui.label(format!(
            "Sun color: [{:.2}, {:.2}, {:.2}]",
            ls.sun_color[0], ls.sun_color[1], ls.sun_color[2]
        ));
        ui.label(format!("Sun intensity: {:.2}", ls.sun_intensity));
        ui.label(format!(
            "Ambient: [{:.2}, {:.2}, {:.2}] @ {:.2}",
            ls.ambient_color[0], ls.ambient_color[1], ls.ambient_color[2], ls.ambient_intensity
        ));
    });
}

/// Draw the "Distance Fog" controls window — enable toggle, fog type selector,
/// density slider, linear start/end sliders, and current fog color.
pub(crate) fn draw_fog_window(ctx: &egui::Context, ls: &mut LightingState) {
    egui::Window::new("Distance Fog").show(ctx, |ui| {
        ui.checkbox(&mut ls.fog_config.enabled, "Fog enabled");
        ui.separator();

        // Fog type selector.
        let fog_label = ls.fog_config.fog_type.as_str();
        egui::ComboBox::from_label("Fog Type")
            .selected_text(fog_label)
            .show_ui(ui, |ui| {
                for &ft in FogType::all() {
                    ui.selectable_value(&mut ls.fog_config.fog_type, ft, ft.as_str());
                }
            });

        ui.label("Fog Density");
        ui.add(
            egui::Slider::new(&mut ls.fog_config.density, 0.001..=0.1).logarithmic(true),
        );

        // Linear fog start/end (only relevant for linear fog type).
        ui.label("Fog Start (linear)");
        ui.add(egui::Slider::new(&mut ls.fog_config.start, 10.0..=500.0).text("m"));
        ui.label("Fog End (linear)");
        ui.add(egui::Slider::new(&mut ls.fog_config.end, 50.0..=2000.0).text("m"));

        // Show current fog color.
        let fc = ls.day_night.fog_color();
        ui.label(format!(
            "Fog color: [{:.2}, {:.2}, {:.2}]",
            fc[0], fc[1], fc[2]
        ));
    });
}
