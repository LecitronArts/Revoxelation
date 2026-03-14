use std::time::Instant;

use glam::{Vec3, vec3};
use hecs::{Entity, World};
use winit::keyboard::KeyCode;

use crate::renderer::camera::PhysicalCamera;

#[derive(Debug, Clone, Copy)]
struct Transform {
    position: Vec3,
    yaw: f32,
    pitch: f32,
}

#[derive(Debug, Clone, Copy)]
struct Lens {
    fov_y_degrees: f32,
    aperture: f32,
    focus_distance: f32,
    depth_adapt: f32,
    near: f32,
    far: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct CameraSettings {
    pub fov_y_degrees: f32,
    pub aperture: f32,
    pub focus_distance: f32,
    pub depth_adapt: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            fov_y_degrees: 55.0,
            aperture: 0.02,
            focus_distance: 110.0,
            depth_adapt: 0.35,
            near: 0.01,
            far: 1000.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ControllerSettings {
    pub move_speed: f32,
    pub sprint_multiplier: f32,
    pub mouse_sensitivity: f32,
    pub invert_y: bool,
}

impl Default for ControllerSettings {
    fn default() -> Self {
        Self {
            move_speed: 14.0,
            sprint_multiplier: 2.25,
            mouse_sensitivity: 0.0022,
            invert_y: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CameraState {
    pub position: Vec3,
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct InputState {
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    sprint: bool,
}

pub struct LogicScheduler {
    ecs: World,
    camera_entity: Entity,
    input: InputState,
    controller: ControllerSettings,
    initial_transform: Transform,
    last_tick: Instant,
}

impl LogicScheduler {
    pub fn new() -> Self {
        let mut ecs = World::new();
        let initial_transform = Transform {
            position: vec3(48.0, 42.0, -48.0),
            yaw: 0.68,
            pitch: -0.32,
        };
        let default_camera = CameraSettings::default();
        let camera_entity = ecs.spawn((
            initial_transform,
            Lens {
                fov_y_degrees: default_camera.fov_y_degrees,
                aperture: default_camera.aperture,
                focus_distance: default_camera.focus_distance,
                depth_adapt: default_camera.depth_adapt,
                near: default_camera.near,
                far: default_camera.far,
            },
        ));

        Self {
            ecs,
            camera_entity,
            input: InputState::default(),
            controller: ControllerSettings::default(),
            initial_transform,
            last_tick: Instant::now(),
        }
    }

    pub fn set_key_state(&mut self, key: KeyCode, pressed: bool) {
        match key {
            KeyCode::KeyW => self.input.forward = pressed,
            KeyCode::KeyS => self.input.backward = pressed,
            KeyCode::KeyA => self.input.left = pressed,
            KeyCode::KeyD => self.input.right = pressed,
            KeyCode::Space => self.input.up = pressed,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => self.input.down = pressed,
            KeyCode::ControlLeft | KeyCode::ControlRight => self.input.sprint = pressed,
            _ => {}
        }
    }

    pub fn clear_input(&mut self) {
        self.input = InputState::default();
    }

    pub fn add_mouse_delta(&mut self, delta_x: f32, delta_y: f32) {
        let sensitivity = self.controller.mouse_sensitivity;
        let y_sign = if self.controller.invert_y { 1.0 } else { -1.0 };
        if let Ok(mut transform) = self.ecs.get::<&mut Transform>(self.camera_entity) {
            transform.yaw += delta_x * sensitivity;
            transform.pitch =
                (transform.pitch + delta_y * sensitivity * y_sign).clamp(-1.553, 1.553);
        }
    }

    pub fn update(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().clamp(0.0, 0.05);
        self.last_tick = now;

        if let Ok(mut transform) = self.ecs.get::<&mut Transform>(self.camera_entity) {
            let forward_flat =
                vec3(transform.yaw.cos(), 0.0, transform.yaw.sin()).normalize_or_zero();
            let right_flat = vec3(-forward_flat.z, 0.0, forward_flat.x);

            let mut move_dir = Vec3::ZERO;
            if self.input.forward {
                move_dir += forward_flat;
            }
            if self.input.backward {
                move_dir -= forward_flat;
            }
            if self.input.right {
                move_dir += right_flat;
            }
            if self.input.left {
                move_dir -= right_flat;
            }
            if self.input.up {
                move_dir += Vec3::Y;
            }
            if self.input.down {
                move_dir -= Vec3::Y;
            }

            if move_dir.length_squared() > 0.0 {
                let speed = if self.input.sprint {
                    self.controller.move_speed * self.controller.sprint_multiplier
                } else {
                    self.controller.move_speed
                };
                transform.position += move_dir.normalize_or_zero() * speed * dt;
            }
        }
    }

    pub fn camera_settings(&self) -> CameraSettings {
        self.ecs
            .get::<&Lens>(self.camera_entity)
            .map(|lens| CameraSettings {
                fov_y_degrees: lens.fov_y_degrees,
                aperture: lens.aperture,
                focus_distance: lens.focus_distance,
                depth_adapt: lens.depth_adapt,
                near: lens.near,
                far: lens.far,
            })
            .unwrap_or_default()
    }

    pub fn set_camera_settings(&mut self, settings: CameraSettings) {
        if let Ok(mut lens) = self.ecs.get::<&mut Lens>(self.camera_entity) {
            lens.fov_y_degrees = settings.fov_y_degrees.clamp(20.0, 120.0);
            lens.aperture = settings.aperture.clamp(0.0, 2.0);
            lens.focus_distance = settings.focus_distance.clamp(0.1, 5000.0);
            lens.depth_adapt = settings.depth_adapt.clamp(0.0, 2.0);
            lens.near = settings.near.clamp(0.001, 10.0);
            lens.far = settings.far.clamp(lens.near + 0.01, 20000.0);
        }
    }

    pub fn controller_settings(&self) -> ControllerSettings {
        self.controller
    }

    pub fn set_controller_settings(&mut self, settings: ControllerSettings) {
        self.controller = ControllerSettings {
            move_speed: settings.move_speed.clamp(0.1, 200.0),
            sprint_multiplier: settings.sprint_multiplier.clamp(1.0, 20.0),
            mouse_sensitivity: settings.mouse_sensitivity.clamp(0.0001, 0.05),
            invert_y: settings.invert_y,
        };
    }

    pub fn camera_state(&self) -> CameraState {
        let transform = self
            .ecs
            .get::<&Transform>(self.camera_entity)
            .map(|value| *value)
            .unwrap_or(self.initial_transform);
        CameraState {
            position: transform.position,
            yaw_degrees: transform.yaw.to_degrees(),
            pitch_degrees: transform.pitch.to_degrees(),
        }
    }

    pub fn reset_camera_pose(&mut self) {
        if let Ok(mut transform) = self.ecs.get::<&mut Transform>(self.camera_entity) {
            *transform = self.initial_transform;
        }
    }

    pub fn primary_camera(&self) -> PhysicalCamera {
        let transform = self
            .ecs
            .get::<&Transform>(self.camera_entity)
            .map(|value| *value)
            .unwrap_or(Transform {
                position: vec3(0.0, 0.0, 0.0),
                yaw: 0.0,
                pitch: 0.0,
            });

        let lens = self
            .ecs
            .get::<&Lens>(self.camera_entity)
            .map(|value| *value)
            .unwrap_or(Lens {
                fov_y_degrees: 60.0,
                aperture: 0.0,
                focus_distance: 32.0,
                depth_adapt: 0.2,
                near: 0.01,
                far: 1000.0,
            });

        let forward = vec3(
            transform.yaw.cos() * transform.pitch.cos(),
            transform.pitch.sin(),
            transform.yaw.sin() * transform.pitch.cos(),
        )
        .normalize_or_zero();

        PhysicalCamera {
            position: transform.position,
            forward,
            up: Vec3::Y,
            fov_y_radians: lens.fov_y_degrees.to_radians(),
            aperture: lens.aperture,
            focus_distance: lens.focus_distance,
            near: lens.near,
            far: lens.far,
            depth_adapt: lens.depth_adapt,
        }
    }
}
