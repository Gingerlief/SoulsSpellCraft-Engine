// Docs: docs/spellcraft-engine/fusion/onhit_trigger.md

use super::{Fuse, FusionError, FusionMode};
use crate::graph::SpellGraph;

pub struct OnHitTrigger;

impl Fuse for OnHitTrigger {
    fn fuse(&self, _ingredients: &[SpellGraph]) -> Result<SpellGraph, FusionError> {
        Err(FusionError::NotImplemented(FusionMode::OnHitTrigger))
    }
}
