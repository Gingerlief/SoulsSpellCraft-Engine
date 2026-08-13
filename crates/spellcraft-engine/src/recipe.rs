// Docs: docs/spellcraft-engine/recipe.md

use crate::fusion::FusionMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellRecipe {
    pub ingredients: Vec<SpellId>,
    pub fusion_mode: FusionMode,
    pub target_slot: i64,
    pub shell_source: SpellId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SpellId(pub i64);
