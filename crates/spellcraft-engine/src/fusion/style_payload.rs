// Docs: docs/spellcraft-engine/fusion/style_payload.md

use super::{Fuse, FusionError, FusionMode};
use crate::graph::SpellGraph;

pub struct StylePayload;

impl Fuse for StylePayload {
    fn fuse(&self, _ingredients: &[SpellGraph]) -> Result<SpellGraph, FusionError> {
        Err(FusionError::NotImplemented(FusionMode::StylePayload))
    }
}
