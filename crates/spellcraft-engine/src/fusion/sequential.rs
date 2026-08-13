// Docs: docs/spellcraft-engine/fusion/sequential.md

use super::{Fuse, FusionError, FusionMode};
use crate::graph::SpellGraph;

pub struct Sequential;

impl Fuse for Sequential {
    fn fuse(&self, _ingredients: &[SpellGraph]) -> Result<SpellGraph, FusionError> {
        Err(FusionError::NotImplemented(FusionMode::Sequential))
    }
}
