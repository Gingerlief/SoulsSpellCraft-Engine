// Docs: docs/spellcraft-engine/graph/cast.md

use std::collections::{BTreeSet, VecDeque};

use super::model::{CastType, NodeKey, NodeKind, SpellGraph};

pub fn annotate_cast_reachability(graph: &mut SpellGraph) {
    let root = graph.root();
    graph.cast_reachability.clear();

    // Seeds: the Magic row's own outgoing edges, each carrying its slot's cast type. The
    // root itself is reachable under every cast type its slots declare.
    let mut queue: VecDeque<(NodeKey, CastType)> = VecDeque::new();
    let mut seen: BTreeSet<(NodeKey, CastType)> = BTreeSet::new();

    let seeds: Vec<(NodeKey, CastType)> =
        graph.out_edges(root).map(|e| (e.to, e.cast_type)).collect();

    for (node, cast) in seeds {
        graph
            .cast_reachability
            .entry(root)
            .or_default()
            .insert(cast);
        if seen.insert((node, cast)) {
            queue.push_back((node, cast));
        }
    }

    while let Some((node, cast)) = queue.pop_front() {
        graph
            .cast_reachability
            .entry(node)
            .or_default()
            .insert(cast);

        let next: Vec<NodeKey> = graph.out_edges(node).map(|e| e.to).collect();
        for child in next {
            if seen.insert((child, cast)) {
                queue.push_back((child, cast));
            }
        }
    }
}

impl SpellGraph {
    pub fn nodes_reachable_under(&self, cast: CastType) -> impl Iterator<Item = NodeKey> + '_ {
        self.cast_reachability
            .iter()
            .filter(move |(_, casts)| casts.contains(&cast))
            .map(|(key, _)| *key)
    }

    pub fn attack_targets_under(&self, cast: CastType) -> Vec<NodeKey> {
        let mut out: Vec<NodeKey> = self
            .nodes_reachable_under(cast)
            .filter(|k| k.kind == NodeKind::Atk)
            .collect();
        out.sort();
        out
    }

    pub fn cast_types(&self) -> BTreeSet<CastType> {
        self.cast_reachability
            .get(&self.root())
            .cloned()
            .unwrap_or_default()
    }
}
