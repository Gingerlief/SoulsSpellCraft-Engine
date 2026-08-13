// Docs: docs/spellcraft-engine/graph/model.md

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use souls_format::{Fxr, ParamRow};

use crate::source::ParamTable;

pub const GRAPH_SCHEMA_VERSION: u32 = 1;

pub const WALKER_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NodeKind {
    Magic,
    Bullet,
    Atk,
    SpEffect,
    SpEffectVfx,
    Fxr,
    Goods,
}

impl NodeKind {
    pub fn table(self) -> Option<ParamTable> {
        Some(match self {
            NodeKind::Magic => ParamTable::Magic,
            NodeKind::Bullet => ParamTable::Bullet,
            NodeKind::Atk => ParamTable::AtkPc,
            NodeKind::SpEffect => ParamTable::SpEffect,
            NodeKind::SpEffectVfx => ParamTable::SpEffectVfx,
            NodeKind::Goods => ParamTable::Goods,
            NodeKind::Fxr => return None,
        })
    }

    pub fn name_list(self) -> Option<&'static str> {
        self.table().map(|t| t.name_list())
    }

    pub fn is_generated_row(self) -> bool {
        matches!(self, NodeKind::Bullet | NodeKind::Atk | NodeKind::SpEffect)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeKey {
    pub kind: NodeKind,
    pub id: i64,
}

impl NodeKey {
    pub fn new(kind: NodeKind, id: i64) -> Self {
        NodeKey { kind, id }
    }
}

impl std::fmt::Display for NodeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}:{}", self.kind, self.id)
    }
}

impl std::str::FromStr for NodeKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, id) = s
            .rsplit_once(':')
            .ok_or_else(|| format!("not a node key (expected 'Kind:id'): {s}"))?;
        let kind = match kind {
            "Magic" => NodeKind::Magic,
            "Bullet" => NodeKind::Bullet,
            "Atk" => NodeKind::Atk,
            "SpEffect" => NodeKind::SpEffect,
            "SpEffectVfx" => NodeKind::SpEffectVfx,
            "Fxr" => NodeKind::Fxr,
            "Goods" => NodeKind::Goods,
            other => return Err(format!("unknown node kind '{other}' in key '{s}'")),
        };
        let id = id
            .parse::<i64>()
            .map_err(|_| format!("not a row id: '{id}' in key '{s}'"))?;
        Ok(NodeKey::new(kind, id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeField {
    pub name: String,
    pub index: Option<u8>,
}

impl EdgeField {
    pub fn plain(name: &str) -> Self {
        EdgeField {
            name: name.to_string(),
            index: None,
        }
    }

    pub fn indexed(name: &str, index: u8) -> Self {
        EdgeField {
            name: name.to_string(),
            index: Some(index),
        }
    }
}

impl std::fmt::Display for EdgeField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.index {
            Some(i) => write!(f, "{}{}", self.name, i),
            None => write!(f, "{}", self.name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CastType {
    Default,
    Charged,
    None,
    Unknown(i32),
}

impl CastType {
    pub fn from_consume_type(consume_type: i32) -> Self {
        match consume_type {
            0 => CastType::Default,
            1 => CastType::Charged,
            2 => CastType::None,
            other => CastType::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeResolution {
    Declared,
    ProbedBullet,
    ProbedAtk,
    Unresolvable,
    IdConvention,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeKey,
    pub to: NodeKey,
    pub field: EdgeField,
    pub ref_category: Option<i32>,
    pub consume_type: Option<i32>,
    pub cast_type: CastType,
    pub resolution: EdgeResolution,
    pub source_action: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Resolved,
    RowMissing,
    RowUndecodable {
        error: String,
    },
    TableUnavailable {
        error: String,
    },
    FxrFileMissing,
    FxrUnparseable {
        error: String,
    },
}

impl NodeStatus {
    pub fn is_resolved(&self) -> bool {
        matches!(self, NodeStatus::Resolved)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub key: NodeKey,
    pub status: NodeStatus,
    #[serde(skip)]
    pub row: Option<Arc<ParamRow>>,
    #[serde(skip)]
    pub fxr: Option<Arc<Fxr>>,
}

impl Node {
    pub fn new(key: NodeKey, status: NodeStatus) -> Self {
        Node {
            key,
            status,
            row: None,
            fxr: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Classification {
    Projectile,
    DirectAttack,
    Charged,
    ChildBullet,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticKind {
    FieldMissing,
    FieldWrongType,
    RowMissing,
    RowUndecodable,
    TableUnavailable,
    FxrMissing,
    FxrUnparseable,
    UnresolvableReference,
    ResolvedFromNpcAtk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub node: Option<NodeKey>,
    pub field: Option<String>,
    pub kind: DiagnosticKind,
    pub detail: String,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextSource {
    #[default]
    None,
    ParamdexNames,
    GameText,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Presentation {
    pub goods: Option<NodeKey>,
    pub icon_id: Option<i64>,
    pub name: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub text_source: TextSource,
}

mod node_key_map {
    use super::{CastType, NodeKey};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::{BTreeMap, BTreeSet};

    pub fn serialize<S: Serializer>(
        map: &BTreeMap<NodeKey, BTreeSet<CastType>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter()
            .map(|(k, v)| (*k, v))
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<NodeKey, BTreeSet<CastType>>, D::Error> {
        Ok(Vec::<(NodeKey, BTreeSet<CastType>)>::deserialize(d)?
            .into_iter()
            .collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellGraph {
    pub magic_id: i64,
    pub schema_version: u32,
    pub walker_version: u32,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub classifications: BTreeSet<Classification>,
    #[serde(with = "node_key_map")]
    pub cast_reachability: BTreeMap<NodeKey, BTreeSet<CastType>>,
    #[serde(default)]
    pub presentation: Option<Presentation>,
    pub diagnostics: Vec<Diagnostic>,

    #[serde(skip)]
    node_index: HashMap<NodeKey, usize>,
    #[serde(skip)]
    out_edges: HashMap<NodeKey, Vec<usize>>,
    #[serde(skip)]
    in_edges: HashMap<NodeKey, Vec<usize>>,
}

impl SpellGraph {
    pub(crate) fn new(magic_id: i64) -> Self {
        SpellGraph {
            magic_id,
            schema_version: GRAPH_SCHEMA_VERSION,
            walker_version: WALKER_VERSION,
            nodes: Vec::new(),
            edges: Vec::new(),
            classifications: BTreeSet::new(),
            cast_reachability: BTreeMap::new(),
            presentation: None,
            diagnostics: Vec::new(),
            node_index: HashMap::new(),
            out_edges: HashMap::new(),
            in_edges: HashMap::new(),
        }
    }

    pub fn reindex(&mut self) {
        self.node_index = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.key, i))
            .collect();

        self.out_edges.clear();
        self.in_edges.clear();
        for (i, edge) in self.edges.iter().enumerate() {
            self.out_edges.entry(edge.from).or_default().push(i);
            self.in_edges.entry(edge.to).or_default().push(i);
        }
    }

    pub fn node(&self, key: NodeKey) -> Option<&Node> {
        self.node_index.get(&key).map(|&i| &self.nodes[i])
    }

    pub fn node_mut(&mut self, key: NodeKey) -> Option<&mut Node> {
        let i = *self.node_index.get(&key)?;
        Some(&mut self.nodes[i])
    }

    pub fn contains(&self, key: NodeKey) -> bool {
        self.node_index.contains_key(&key)
    }

    pub fn out_edges(&self, key: NodeKey) -> impl Iterator<Item = &Edge> {
        self.out_edges
            .get(&key)
            .into_iter()
            .flatten()
            .map(|&i| &self.edges[i])
    }

    pub fn in_edges(&self, key: NodeKey) -> impl Iterator<Item = &Edge> {
        self.in_edges
            .get(&key)
            .into_iter()
            .flatten()
            .map(|&i| &self.edges[i])
    }

    pub fn nodes_of_kind(&self, kind: NodeKind) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter(move |n| n.key.kind == kind)
    }

    pub fn root(&self) -> NodeKey {
        NodeKey::new(NodeKind::Magic, self.magic_id)
    }

    pub fn param_projection(&self) -> SpellGraph {
        let keep =
            |k: NodeKind| !matches!(k, NodeKind::Fxr | NodeKind::SpEffectVfx | NodeKind::Goods);

        let mut out = SpellGraph::new(self.magic_id);
        out.schema_version = self.schema_version;
        out.walker_version = self.walker_version;
        out.classifications = self.classifications.clone();
        out.presentation = self.presentation.clone();
        out.nodes = self
            .nodes
            .iter()
            .filter(|n| keep(n.key.kind))
            .cloned()
            .collect();
        out.edges = self
            .edges
            .iter()
            .filter(|e| keep(e.from.kind) && keep(e.to.kind))
            .cloned()
            .collect();
        out.cast_reachability = self
            .cast_reachability
            .iter()
            .filter(|(k, _)| keep(k.kind))
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        out.diagnostics = self
            .diagnostics
            .iter()
            .filter(|d| d.node.is_none_or(|n| keep(n.kind)))
            .cloned()
            .collect();
        out.reindex();
        out
    }

    pub fn generated_row_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| n.key.kind.is_generated_row())
            .count()
    }

    pub fn hydrate_rows(&mut self, source: &dyn crate::source::ParamSource) {
        for node in &mut self.nodes {
            if node.row.is_some() || !node.status.is_resolved() {
                continue;
            }
            let Some(table) = node.key.kind.table() else {
                continue;
            };
            if let Some(row) = source.row(table, node.key.id).found() {
                node.row = Some(Arc::clone(row));
            }
        }
    }

    pub(crate) fn add_node(&mut self, node: Node) {
        if self.node_index.contains_key(&node.key) {
            return;
        }
        self.node_index.insert(node.key, self.nodes.len());
        self.nodes.push(node);
    }

    pub(crate) fn add_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    pub fn push_node(&mut self, node: Node) {
        if self.node_index.contains_key(&node.key) {
            return;
        }
        self.nodes.push(node);
    }

    pub fn push_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    pub(crate) fn diagnose(&mut self, d: Diagnostic) {
        self.diagnostics.push(d);
    }
}
