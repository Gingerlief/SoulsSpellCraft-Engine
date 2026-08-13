// Docs: docs/spellcraft-engine/fusion/onhit_merge.md

use super::{Fuse, FusionError, FusionMode};
use crate::graph::SpellGraph;

pub struct OnHitMerge;

impl Fuse for OnHitMerge {
    fn fuse(&self, _ingredients: &[SpellGraph]) -> Result<SpellGraph, FusionError> {
        Err(FusionError::NotImplemented(FusionMode::OnHitMerge))
    }
}
