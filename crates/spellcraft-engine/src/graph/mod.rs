// Docs: docs/spellcraft-engine/graph.md

pub mod cast;
pub mod model;
pub mod walk;

pub use model::{
    CastType, Classification, Diagnostic, DiagnosticKind, Edge, EdgeField, EdgeResolution, Node,
    NodeKey, NodeKind, NodeStatus, Presentation, SpellGraph, TextSource, GRAPH_SCHEMA_VERSION,
    WALKER_VERSION,
};
pub use walk::{GraphError, NameSource, SfxSource, WalkOptions, Walker};

#[cfg(test)]
mod tests;
