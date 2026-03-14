#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};

use bytemuck::{Pod, Zeroable};

const INDEX_MASK: u32 = 0x00ff_ffff;
const WEIGHT_SHIFT: u32 = 24;
const INVALID_INDEX: u32 = INDEX_MASK;

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Reservoir {
    pub z_i: u32,
    pub w_sum: f32,
    pub m_i: f32,
    pub w_var: f32,
}

impl Reservoir {
    pub const fn empty() -> Self {
        Self {
            z_i: INVALID_INDEX,
            w_sum: 0.0,
            m_i: 0.0,
            w_var: 0.0,
        }
    }

    pub fn selected_index(self) -> Option<u32> {
        let idx = unpack_index(self.z_i);
        if idx == INVALID_INDEX {
            None
        } else {
            Some(idx)
        }
    }

    pub fn selected_weight_hint(self) -> f32 {
        unpack_weight_hint(self.z_i)
    }

    pub fn update(
        &mut self,
        sample_index: u32,
        sample_weight: f32,
        inv_target_pdf: f32,
        sample_count: f32,
        random_01: f32,
    ) {
        if sample_weight <= 0.0 || inv_target_pdf <= 0.0 || sample_count <= 0.0 {
            return;
        }

        let new_w_sum = self.w_sum + sample_weight;
        let acceptance = (sample_weight / new_w_sum.max(1.0e-6)).clamp(0.0, 1.0);
        self.w_sum = new_w_sum;
        self.m_i += sample_count;
        if random_01 < acceptance {
            self.z_i = pack_index_weight_hint(sample_index, sample_weight);
            self.w_var = inv_target_pdf;
        }
    }

    pub fn merge(self, other: Self, random_01: f32) -> Self {
        if other.w_sum <= 0.0 || other.m_i <= 0.0 || other.w_var <= 0.0 {
            return self;
        }
        if self.w_sum <= 0.0 || self.m_i <= 0.0 || self.w_var <= 0.0 {
            return other;
        }

        let merged_w_sum = self.w_sum + other.w_sum;
        let pick_other = random_01 < (other.w_sum / merged_w_sum.max(1.0e-6));
        if pick_other {
            Self {
                z_i: other.z_i,
                w_sum: merged_w_sum,
                m_i: self.m_i + other.m_i,
                w_var: other.w_var,
            }
        } else {
            Self {
                z_i: self.z_i,
                w_sum: merged_w_sum,
                m_i: self.m_i + other.m_i,
                w_var: self.w_var,
            }
        }
    }

    pub fn finalize(self) -> Option<(u32, f32)> {
        let index = self.selected_index()?;
        if self.w_sum <= 0.0 || self.m_i <= 0.0 || self.w_var <= 0.0 {
            return None;
        }
        Some((index, (self.w_sum * self.w_var) / self.m_i.max(1.0e-6)))
    }
}

#[derive(Debug, Default)]
pub struct AtomicReservoir {
    z_i: AtomicU32,
    w_sum_bits: AtomicU32,
    m_i_bits: AtomicU32,
    w_var_bits: AtomicU32,
}

impl AtomicReservoir {
    pub fn new_empty() -> Self {
        Self {
            z_i: AtomicU32::new(INVALID_INDEX),
            w_sum_bits: AtomicU32::new(0.0f32.to_bits()),
            m_i_bits: AtomicU32::new(0.0f32.to_bits()),
            w_var_bits: AtomicU32::new(0.0f32.to_bits()),
        }
    }

    pub fn load(&self) -> Reservoir {
        Reservoir {
            z_i: self.z_i.load(Ordering::Acquire),
            w_sum: f32::from_bits(self.w_sum_bits.load(Ordering::Acquire)),
            m_i: f32::from_bits(self.m_i_bits.load(Ordering::Acquire)),
            w_var: f32::from_bits(self.w_var_bits.load(Ordering::Acquire)),
        }
    }

    pub fn store(&self, value: Reservoir) {
        self.z_i.store(value.z_i, Ordering::Release);
        self.w_sum_bits
            .store(value.w_sum.to_bits(), Ordering::Release);
        self.m_i_bits.store(value.m_i.to_bits(), Ordering::Release);
        self.w_var_bits
            .store(value.w_var.to_bits(), Ordering::Release);
    }

    pub fn atomic_update(
        &self,
        sample_index: u32,
        sample_weight: f32,
        inv_target_pdf: f32,
        sample_count: f32,
        random_01: f32,
    ) {
        if sample_weight <= 0.0 || inv_target_pdf <= 0.0 || sample_count <= 0.0 {
            return;
        }

        let new_w_sum = atomic_add_f32(&self.w_sum_bits, sample_weight);
        let _new_m_i = atomic_add_f32(&self.m_i_bits, sample_count);
        let acceptance = (sample_weight / new_w_sum.max(1.0e-6)).clamp(0.0, 1.0);
        if random_01 < acceptance {
            self.z_i.store(
                pack_index_weight_hint(sample_index, sample_weight),
                Ordering::Release,
            );
            self.w_var_bits
                .store(inv_target_pdf.to_bits(), Ordering::Release);
        }
    }

    pub fn atomic_merge(&self, incoming: Reservoir, random_01: f32) {
        if incoming.w_sum <= 0.0 || incoming.m_i <= 0.0 || incoming.w_var <= 0.0 {
            return;
        }

        let base_w_sum = atomic_add_f32(&self.w_sum_bits, incoming.w_sum) - incoming.w_sum;
        let merged_w_sum = base_w_sum + incoming.w_sum;
        let _ = atomic_add_f32(&self.m_i_bits, incoming.m_i);

        let acceptance = (incoming.w_sum / merged_w_sum.max(1.0e-6)).clamp(0.0, 1.0);
        if random_01 < acceptance {
            self.z_i.store(incoming.z_i, Ordering::Release);
            self.w_var_bits
                .store(incoming.w_var.to_bits(), Ordering::Release);
        }
    }
}

pub fn pack_index_weight_hint(index: u32, weight_hint: f32) -> u32 {
    let clamped_index = index.min(INDEX_MASK - 1);
    let encoded = encode_weight_hint(weight_hint);
    (clamped_index & INDEX_MASK) | (u32::from(encoded) << WEIGHT_SHIFT)
}

pub fn unpack_index(z_i: u32) -> u32 {
    z_i & INDEX_MASK
}

pub fn unpack_weight_hint(z_i: u32) -> f32 {
    let encoded = ((z_i >> WEIGHT_SHIFT) & 0xff) as u8;
    decode_weight_hint(encoded)
}

fn encode_weight_hint(weight: f32) -> u8 {
    if !weight.is_finite() || weight <= 0.0 {
        return 0;
    }
    let log2_w = weight.log2().clamp(-20.0, 20.0);
    let normalized = (log2_w + 20.0) * (1.0 / 40.0);
    (normalized * 255.0 + 0.5).floor() as u8
}

fn decode_weight_hint(encoded: u8) -> f32 {
    let normalized = (encoded as f32) * (1.0 / 255.0);
    let log2_w = normalized * 40.0 - 20.0;
    2.0f32.powf(log2_w)
}

fn atomic_add_f32(target: &AtomicU32, delta: f32) -> f32 {
    let mut current = target.load(Ordering::Acquire);
    loop {
        let current_f = f32::from_bits(current);
        let next_f = current_f + delta;
        let next = next_f.to_bits();
        match target.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return next_f,
            Err(observed) => current = observed,
        }
    }
}
