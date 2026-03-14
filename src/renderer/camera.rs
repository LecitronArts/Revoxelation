use glam::Vec3;

use super::protocol::CameraGpu;

#[derive(Debug, Clone, Copy)]
pub struct PhysicalCamera {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub fov_y_radians: f32,
    pub aperture: f32,
    pub focus_distance: f32,
    pub near: f32,
    pub far: f32,
    pub depth_adapt: f32,
}

impl PhysicalCamera {
    pub fn to_gpu(self, width: u32, height: u32, frame_index: u32) -> CameraGpu {
        let mut forward = self.forward.normalize_or_zero();
        if forward.length_squared() < 1e-6 {
            forward = Vec3::Z;
        }

        let mut up = self.up.normalize_or_zero();
        if up.length_squared() < 1e-6 {
            up = Vec3::Y;
        }

        let mut right = forward.cross(up).normalize_or_zero();
        if right.length_squared() < 1e-6 {
            right = Vec3::X;
        }
        up = right.cross(forward).normalize_or_zero();

        let aspect = if height == 0 {
            1.0
        } else {
            width as f32 / height as f32
        };

        CameraGpu {
            position_lens: [
                self.position.x,
                self.position.y,
                self.position.z,
                self.aperture * 0.5,
            ],
            forward_fov: [forward.x, forward.y, forward.z, self.fov_y_radians],
            right_aspect: [right.x, right.y, right.z, aspect],
            up_focus: [up.x, up.y, up.z, self.focus_distance],
            clip_depth: [self.near, self.far, self.depth_adapt, 0.0],
            resolution_frame: [width.max(1), height.max(1), frame_index, 0],
        }
    }
}
