// Docs: docs/spellcraft-engine/fusion/parallel.md

use super::{Fuse, FusionError, FusionMode};
use crate::graph::SpellGraph;

pub struct Parallel;

impl Fuse for Parallel {
    fn fuse(&self, _ingredients: &[SpellGraph]) -> Result<SpellGraph, FusionError> {
        Err(FusionError::NotImplemented(FusionMode::Parallel))
    }
}
