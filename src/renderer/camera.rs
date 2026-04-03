use glam::{Mat4, Vec3};

/// Key abstraction for camera movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraKey {
    Forward,
    Backward,
    Left,
    Right,
    Up,
    Down,
}

/// Push-constant-compatible camera uniforms (80 bytes).
///
/// Layout must match the GLSL `push_constant` block in the vertex shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 3],
    pub _pad: f32,
}

/// First-person-shooter style camera with position, yaw, and pitch.
pub struct FpsCamera {
    pub position: Vec3,
    /// Yaw in radians (rotation around Y axis). 0 = looking along -Z.
    pub yaw: f32,
    /// Pitch in radians (rotation around X axis). Clamped to +/-89 degrees.
    pub pitch: f32,
    /// Vertical field-of-view in radians.
    pub fov_y: f32,
    /// Near clip plane distance.
    pub near: f32,
    /// Far clip plane distance.
    pub far: f32,
    /// Movement speed in units per second (configurable, POLISH-09).
    pub move_speed: f32,
    /// Mouse look sensitivity (configurable, POLISH-09).
    pub mouse_sensitivity: f32,
}

impl Default for FpsCamera {
    fn default() -> Self {
        Self {
            position: Vec3::new(32.0, 48.0, -60.0),
            yaw: 0.0,
            pitch: 0.0,
            fov_y: 60.0_f32.to_radians(),
            near: 0.1,
            far: 2000.0,
            move_speed: 10.0,
            mouse_sensitivity: 0.1,
        }
    }
}

impl FpsCamera {
    /// Forward direction derived from yaw and pitch (right-handed, -Z forward at yaw=0).
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        )
        .normalize_or_zero()
    }

    /// Right direction (perpendicular to forward and world-up).
    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize_or_zero()
    }

    /// Compute the combined view-projection matrix and return as CameraUniforms.
    pub fn view_proj(&self, aspect: f32) -> CameraUniforms {
        let forward = self.forward();
        let target = self.position + forward;
        let view = Mat4::look_at_rh(self.position, target, Vec3::Y);
        let proj = Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far);
        let view_proj = proj * view;

        CameraUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            camera_pos: self.position.to_array(),
            _pad: 0.0,
        }
    }

    /// Process a keyboard movement key with delta_time for frame-rate-independent movement (POLISH-09).
    ///
    /// Movement = direction * move_speed * delta_time.
    pub fn process_keyboard(&mut self, key: CameraKey, pressed: bool, delta_time: f32) {
        if !pressed {
            return;
        }
        let velocity = self.move_speed * delta_time;
        let forward = self.forward();
        let right = self.right();

        match key {
            CameraKey::Forward => self.position += forward * velocity,
            CameraKey::Backward => self.position -= forward * velocity,
            CameraKey::Left => self.position -= right * velocity,
            CameraKey::Right => self.position += right * velocity,
            CameraKey::Up => self.position += Vec3::Y * velocity,
            CameraKey::Down => self.position -= Vec3::Y * velocity,
        }
    }

    /// Process mouse delta for look rotation (POLISH-09).
    ///
    /// `dx`/`dy` are raw pixel deltas. Uses the camera's `mouse_sensitivity` field.
    pub fn process_mouse(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * self.mouse_sensitivity * 0.01;
        self.pitch -= dy * self.mouse_sensitivity * 0.01;

        // Clamp pitch to +/- 89 degrees.
        let max_pitch = 89.0_f32.to_radians();
        self.pitch = self.pitch.clamp(-max_pitch, max_pitch);
    }
}

/// Six frustum planes in ax + by + cz + d = 0 form.
///
/// 96 bytes — suitable for a small GPU SSBO.
/// Planes are normalized (|normal| = 1) for correct distance testing.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FrustumPlanes {
    /// Order: left, right, bottom, top, near, far.
    /// Each plane is `[a, b, c, d]` where `a*x + b*y + c*z + d >= 0` means inside.
    pub planes: [[f32; 4]; 6],
}

/// Extract 6 frustum planes from a combined view-projection matrix using the
/// Gribb-Hartmann method. Each plane is normalized so that the normal has unit length.
///
/// Plane order: left, right, bottom, top, near, far.
pub fn extract_frustum_planes(view_proj: &Mat4) -> FrustumPlanes {
    // Rows of the view_proj matrix (column-major → rows via row())
    let row0 = view_proj.row(0); // x
    let row1 = view_proj.row(1); // y
    let row2 = view_proj.row(2); // z
    let row3 = view_proj.row(3); // w

    let raw_planes = [
        row3 + row0, // left
        row3 - row0, // right
        row3 + row1, // bottom
        row3 - row1, // top
        row2,        // near — Vulkan z∈[0,w]: near plane = row2 only (MED-01)
        row3 - row2, // far
    ];

    let mut planes = [[0.0_f32; 4]; 6];
    for (i, p) in raw_planes.iter().enumerate() {
        let normal_len = (p.x * p.x + p.y * p.y + p.z * p.z).sqrt();
        if normal_len > 1e-10 {
            planes[i] = [
                p.x / normal_len,
                p.y / normal_len,
                p.z / normal_len,
                p.w / normal_len,
            ];
        } else {
            planes[i] = [p.x, p.y, p.z, p.w];
        }
    }

    FrustumPlanes { planes }
}
