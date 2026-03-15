#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Input,
    Simulation,
    WorldUpdate,
    MeshSync,
    RenderSubmit,
}

pub const STAGE_ORDER: [Stage; 5] = [
    Stage::Input,
    Stage::Simulation,
    Stage::WorldUpdate,
    Stage::MeshSync,
    Stage::RenderSubmit,
];

impl Stage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Stage::Input => "Input",
            Stage::Simulation => "Simulation",
            Stage::WorldUpdate => "WorldUpdate",
            Stage::MeshSync => "MeshSync",
            Stage::RenderSubmit => "RenderSubmit",
        }
    }
}