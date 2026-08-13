// Docs: docs/spellcraft-engine/fusion.md

mod fan;
mod onhit_merge;
mod onhit_trigger;
mod parallel;
mod sequential;
mod style_payload;

use crate::graph::SpellGraph;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FusionMode {
    Sequential,
    Fan,
    OnHitMerge,
    OnHitTrigger,
    Parallel,
    StylePayload,
}

#[derive(Debug, thiserror::Error)]
pub enum FusionError {
    #[error("fusion mode {0:?} is not implemented yet")]
    NotImplemented(FusionMode),
}

pub trait Fuse {
    fn fuse(&self, ingredients: &[SpellGraph]) -> Result<SpellGraph, FusionError>;
}

pub fn strategy_for(mode: FusionMode) -> Box<dyn Fuse> {
    match mode {
        FusionMode::Sequential => Box::new(sequential::Sequential),
        FusionMode::Fan => Box::new(fan::Fan),
        FusionMode::OnHitMerge => Box::new(onhit_merge::OnHitMerge),
        FusionMode::OnHitTrigger => Box::new(onhit_trigger::OnHitTrigger),
        FusionMode::Parallel => Box::new(parallel::Parallel),
        FusionMode::StylePayload => Box::new(style_payload::StylePayload),
    }
}
