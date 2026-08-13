// Docs: docs/spellcraft-engine/fusion/fan.md

use super::{Fuse, FusionError, FusionMode};
use crate::graph::SpellGraph;

pub struct Fan;

impl Fuse for Fan {
    fn fuse(&self, _ingredients: &[SpellGraph]) -> Result<SpellGraph, FusionError> {
        Err(FusionError::NotImplemented(FusionMode::Fan))
    }
}
