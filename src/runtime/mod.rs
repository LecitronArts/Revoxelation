pub mod boundaries;
pub mod observability;
pub mod scheduler;
pub mod stages;
pub mod systems;
pub mod trace;

pub use observability::{RuntimeHudOverlay, RuntimeOverlayStageProgress};
pub use scheduler::{run_frame, FrameExecution};
pub use stages::{Stage, STAGE_ORDER};
pub use trace::{TraceEntry, TransitionKind};