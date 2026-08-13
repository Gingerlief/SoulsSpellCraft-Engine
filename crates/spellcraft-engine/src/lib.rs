// Docs: docs/spellcraft-engine/lib.md

pub mod allocator;
pub mod cache;
pub mod craft_patch;
pub mod export;
pub mod fusion;
pub mod graph;
pub mod link_fields;
pub mod patch;
pub mod recipe;
pub mod source;
/// Shared CSV-fixture builders for tests. Compiled only under `cfg(test)`.
#[cfg(test)]
mod test_support;

pub use recipe::SpellRecipe;
