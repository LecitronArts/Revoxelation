use wgpu::util::DeviceExt;

use crate::renderer::protocol::{GiReservoirGpu, ReservoirGpu, SurfaceSampleGpu};

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameBridge {
    frame_index: u32,
}

impl FrameBridge {
    pub fn frame_index(self) -> u32 {
        self.frame_index
    }

    pub fn reset(&mut self) {
        self.frame_index = 0;
    }

    pub fn advance(&mut self) {
        self.frame_index = self.frame_index.saturating_add(1);
    }
}

#[derive(Debug)]
pub struct PingPong<T> {
    pub a: T,
    pub b: T,
}

impl<T> PingPong<T> {
    pub const fn new(a: T, b: T) -> Self {
        Self { a, b }
    }
}

#[derive(Debug)]
pub struct RestirStorage {
    pub di_reservoirs: PingPong<wgpu::Buffer>,
    pub gi_reservoirs: PingPong<wgpu::Buffer>,
    pub surface_history: wgpu::Buffer,
}

impl RestirStorage {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let di_reservoirs = PingPong::new(
            create_reservoir_buffer(device, width, height),
            create_reservoir_buffer(device, width, height),
        );
        let gi_reservoirs = PingPong::new(
            create_gi_reservoir_buffer(device, width, height),
            create_gi_reservoir_buffer(device, width, height),
        );
        let surface_history = create_surface_history_buffer(device, width, height);

        Self {
            di_reservoirs,
            gi_reservoirs,
            surface_history,
        }
    }

    pub fn bindings(&self) -> RestirBindings<'_> {
        RestirBindings {
            di_a: &self.di_reservoirs.a,
            di_b: &self.di_reservoirs.b,
            gi_a: &self.gi_reservoirs.a,
            gi_b: &self.gi_reservoirs.b,
            surface_history: &self.surface_history,
        }
    }
}

#[derive(Clone, Copy)]
pub struct RestirBindings<'a> {
    pub di_a: &'a wgpu::Buffer,
    pub di_b: &'a wgpu::Buffer,
    pub gi_a: &'a wgpu::Buffer,
    pub gi_b: &'a wgpu::Buffer,
    pub surface_history: &'a wgpu::Buffer,
}

fn create_reservoir_buffer(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Buffer {
    let byte_size = storage_byte_size::<ReservoirGpu>(width, height).max(16);
    create_zeroed_storage_buffer(device, "restir-reservoir-buffer", byte_size)
}

fn create_gi_reservoir_buffer(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Buffer {
    let byte_size = storage_byte_size::<GiReservoirGpu>(width, height).max(16);
    create_zeroed_storage_buffer(device, "restir-gi-reservoir-buffer", byte_size)
}

fn create_surface_history_buffer(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Buffer {
    let pixel_count = (width.max(1) as u64) * (height.max(1) as u64);
    let byte_size = (pixel_count * 2 * std::mem::size_of::<SurfaceSampleGpu>() as u64).max(16);
    create_zeroed_storage_buffer(device, "restir-surface-buffer", byte_size)
}

fn storage_byte_size<T>(width: u32, height: u32) -> u64 {
    let pixel_count = (width.max(1) as u64) * (height.max(1) as u64);
    pixel_count * std::mem::size_of::<T>() as u64
}

fn create_zeroed_storage_buffer(
    device: &wgpu::Device,
    label: &str,
    byte_size: u64,
) -> wgpu::Buffer {
    let zeroes = vec![0_u8; byte_size as usize];
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: &zeroes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    })
}
