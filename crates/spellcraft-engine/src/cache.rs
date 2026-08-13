// Docs: docs/spellcraft-engine/cache.md

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::graph::model::{GRAPH_SCHEMA_VERSION, WALKER_VERSION};
use crate::graph::{GraphError, NameSource, SfxSource, SpellGraph, WalkOptions, Walker};
use crate::source::ParamSource;

pub const MANIFEST_VERSION: u32 = 1;
const MANIFEST_FILE: &str = "manifest.json";

const FINGERPRINTED_DEFS: &[&str] = &[
    "MagicParam.xml",
    "BulletParam.xml",
    "AtkParam.xml",
    "SpEffect.xml",
    "SpEffectVfx.xml",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheManifest {
    pub manifest_version: u32,
    pub graph_schema_version: u32,
    pub walker_version: u32,
    pub regulation_sha256: String,
    pub paramdex_fingerprint: String,
    pub follow_sfx: bool,
    pub follow_speffect_vfx: bool,
    pub atk_tables: Vec<String>,
    pub crate_version: String,
}

impl CacheManifest {
    pub fn new_for(
        regulation_sha256: String,
        paramdex_fingerprint: String,
        options: &WalkOptions,
    ) -> Self {
        Self::new(regulation_sha256, paramdex_fingerprint, options)
    }

    fn new(regulation_sha256: String, paramdex_fingerprint: String, options: &WalkOptions) -> Self {
        CacheManifest {
            manifest_version: MANIFEST_VERSION,
            graph_schema_version: GRAPH_SCHEMA_VERSION,
            walker_version: WALKER_VERSION,
            regulation_sha256,
            paramdex_fingerprint,
            follow_sfx: options.follow_sfx,
            follow_speffect_vfx: options.follow_speffect_vfx,
            atk_tables: options
                .atk_tables
                .iter()
                .map(|t| format!("{t:?}"))
                .collect(),
            crate_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn paramdex_fingerprint(defs_dir: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    for name in FINGERPRINTED_DEFS {
        hasher.update(name.as_bytes());
        match std::fs::read(defs_dir.join(name)) {
            Ok(bytes) => hasher.update(&bytes),
            // A missing def is itself a distinguishing fact — fold it in rather than
            // erroring, so a partial paramdex still produces a stable, distinct key.
            Err(_) => hasher.update(b"<missing>"),
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub struct DiskCache {
    dir: PathBuf,
    manifest: CacheManifest,
}

impl DiskCache {
    pub fn open(root: &Path, manifest: CacheManifest) -> std::io::Result<Self> {
        let dir = root.join(&manifest.regulation_sha256);
        std::fs::create_dir_all(&dir)?;

        let manifest_path = dir.join(MANIFEST_FILE);
        let existing = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|t| serde_json::from_str::<CacheManifest>(&t).ok());

        if existing.as_ref() != Some(&manifest) {
            // Stale or absent: clear any graphs written under the old assumptions.
            if existing.is_some() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
            write_atomic(&manifest_path, &serde_json::to_vec_pretty(&manifest)?)?;
        }

        Ok(DiskCache { dir, manifest })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn manifest(&self) -> &CacheManifest {
        &self.manifest
    }

    fn graph_path(&self, magic_id: i64) -> PathBuf {
        self.dir.join(format!("{magic_id}.json"))
    }

    pub fn read(&self, magic_id: i64) -> Option<SpellGraph> {
        let path = self.graph_path(magic_id);
        let text = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<SpellGraph>(&text) {
            Ok(mut graph) => {
                if graph.schema_version != GRAPH_SCHEMA_VERSION
                    || graph.walker_version != WALKER_VERSION
                {
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
                graph.reindex();
                Some(graph)
            }
            Err(_) => {
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    }

    pub fn write(&self, graph: &SpellGraph) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(graph)?;
        write_atomic(&self.graph_path(graph.magic_id), &bytes)
    }

    pub fn cached_ids(&self) -> Vec<i64> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut ids: Vec<i64> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) != Some("json") {
                    return None;
                }
                path.file_stem()?.to_str()?.parse().ok()
            })
            .collect();
        ids.sort_unstable();
        ids
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub struct GraphCache<'a> {
    source: &'a dyn ParamSource,
    sfx: Option<&'a dyn SfxSource>,
    names: Option<&'a dyn NameSource>,
    options: WalkOptions,
    disk: Option<DiskCache>,
    memo: Mutex<HashMap<i64, Arc<SpellGraph>>>,
}

impl<'a> GraphCache<'a> {
    pub fn in_memory(source: &'a dyn ParamSource, options: WalkOptions) -> Self {
        GraphCache {
            source,
            sfx: None,
            names: None,
            options,
            disk: None,
            memo: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_names(mut self, names: &'a dyn NameSource) -> Self {
        self.names = Some(names);
        self
    }

    pub fn with_disk(mut self, disk: DiskCache) -> Self {
        self.disk = Some(disk);
        self
    }

    pub fn with_sfx(mut self, sfx: &'a dyn SfxSource) -> Self {
        self.sfx = Some(sfx);
        self
    }

    pub fn disk(&self) -> Option<&DiskCache> {
        self.disk.as_ref()
    }

    fn walker(&self) -> Walker<'_> {
        let mut w = Walker::new(self.source).with_options(self.options.clone());
        if let Some(sfx) = self.sfx {
            w = w.with_sfx(sfx);
        }
        if let Some(names) = self.names {
            w = w.with_names(names);
        }
        w
    }

    pub fn get(&self, magic_id: i64) -> Result<Arc<SpellGraph>, GraphError> {
        if let Some(hit) = self
            .memo
            .lock()
            .expect("memo mutex poisoned")
            .get(&magic_id)
        {
            return Ok(Arc::clone(hit));
        }

        if let Some(disk) = &self.disk {
            if let Some(mut graph) = disk.read(magic_id) {
                // Structure came from disk; payloads and FXR edges are re-derived so the
                // result is indistinguishable from a fresh walk.
                graph.hydrate_rows(self.source);
                self.rehydrate_fxr(&mut graph);
                let graph = Arc::new(graph);
                self.memo
                    .lock()
                    .expect("memo mutex poisoned")
                    .insert(magic_id, Arc::clone(&graph));
                return Ok(graph);
            }
        }

        let graph = self.walker().walk(magic_id)?;
        if let Some(disk) = &self.disk {
            // A failed write is not a failed walk.
            let _ = disk.write(&graph);
        }
        let graph = Arc::new(graph);
        self.memo
            .lock()
            .expect("memo mutex poisoned")
            .insert(magic_id, Arc::clone(&graph));
        Ok(graph)
    }

    fn rehydrate_fxr(&self, graph: &mut SpellGraph) {
        let Some(sfx) = self.sfx else { return };
        use crate::graph::model::{
            CastType, Edge, EdgeField, EdgeResolution, Node, NodeKey, NodeKind, NodeStatus,
        };
        use souls_format::sfx_dir::FxrResult;

        let mut frontier: Vec<NodeKey> = graph
            .nodes
            .iter()
            .filter(|n| n.key.kind == NodeKind::Fxr)
            .map(|n| n.key)
            .collect();

        while let Some(key) = frontier.pop() {
            let result = sfx.fxr(key.id as i32);
            let fxr = match &result {
                FxrResult::Found(f) => Some(f.clone()),
                _ => None,
            };
            let status = match &result {
                FxrResult::Found(_) => NodeStatus::Resolved,
                FxrResult::Missing => NodeStatus::FxrFileMissing,
                FxrResult::Unparseable { error } => NodeStatus::FxrUnparseable {
                    error: error.clone(),
                },
            };

            if let Some(node) = graph.node_mut(key) {
                node.status = status;
                node.fxr = fxr.clone();
            }

            let Some(fxr) = fxr else { continue };
            for (ordinal, target) in fxr.proxy_targets().into_iter().enumerate() {
                let to = NodeKey::new(NodeKind::Fxr, target as i64);
                let edge = Edge {
                    from: key,
                    to,
                    field: EdgeField::plain("SFXReference.fields1[0]"),
                    ref_category: None,
                    consume_type: None,
                    cast_type: CastType::Default,
                    resolution: EdgeResolution::Declared,
                    source_action: Some(ordinal as u32),
                };
                if graph.edges.contains(&edge) {
                    continue;
                }
                let is_new = !graph.contains(to);
                graph.push_edge(edge);
                if is_new {
                    graph.push_node(Node::new(to, NodeStatus::FxrFileMissing));
                    frontier.push(to);
                }
            }
        }
        graph.reindex();
    }

    pub fn verify(&self, magic_id: i64) -> Result<bool, GraphError> {
        let Some(disk) = &self.disk else {
            return Ok(true);
        };
        let Some(cached) = disk.read(magic_id) else {
            return Ok(true); // nothing cached is not a mismatch
        };
        let fresh = self.walker().walk(magic_id)?;
        Ok(structurally_equal(&cached, &fresh))
    }
}

pub fn structurally_equal(a: &SpellGraph, b: &SpellGraph) -> bool {
    use crate::graph::model::NodeKind;
    let keys = |g: &SpellGraph| -> Vec<_> {
        g.nodes
            .iter()
            .filter(|n| n.key.kind != NodeKind::Fxr)
            .map(|n| n.key)
            .collect()
    };
    let edges = |g: &SpellGraph| -> Vec<_> {
        g.edges
            .iter()
            .filter(|e| e.from.kind != NodeKind::Fxr)
            .cloned()
            .collect()
    };
    let presentation = |g: &SpellGraph| {
        g.presentation
            .as_ref()
            .map(|p| (p.goods, p.icon_id, p.name.clone(), p.text_source))
    };

    a.magic_id == b.magic_id
        && keys(a) == keys(b)
        && edges(a) == edges(b)
        && a.classifications == b.classifications
        && a.cast_reachability == b.cast_reachability
        && presentation(a) == presentation(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("morro-graph-cache-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest(hash: &str) -> CacheManifest {
        CacheManifest::new(
            hash.to_string(),
            "fingerprint".to_string(),
            &WalkOptions::default(),
        )
    }

    #[test]
    fn cached_graph_matches_a_fresh_walk() {
        let root = temp_root("roundtrip");
        let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
        let samples = souls_format::SfxDirectory::new(vec![crate::test_support::reference_dir().join("fxr-samples")]);

        let fresh = Walker::new(&src).with_sfx(&samples).walk(4000).unwrap();

        // First get walks and writes through to disk.
        let disk = DiskCache::open(&root, manifest("abc123")).unwrap();
        let cache = GraphCache::in_memory(&src, WalkOptions::default())
            .with_sfx(&samples)
            .with_disk(disk);
        let first = cache.get(4000).unwrap();
        assert!(structurally_equal(&first, &fresh));

        // A second cache over the same directory reads from disk and must rebuild an
        // equivalent graph — including re-hydrated FXR payloads that were never persisted.
        let disk2 = DiskCache::open(&root, manifest("abc123")).unwrap();
        assert!(disk2.cached_ids().contains(&4000));
        let cache2 = GraphCache::in_memory(&src, WalkOptions::default())
            .with_sfx(&samples)
            .with_disk(disk2);
        let loaded = cache2.get(4000).unwrap();

        assert!(
            structurally_equal(&loaded, &fresh),
            "cached structure diverged"
        );
        assert!(
            cache2.verify(4000).unwrap(),
            "verify should confirm the cache"
        );

        // Payloads and FXR hydration survive the round trip even though neither is stored.
        let fxr_key = crate::graph::NodeKey::new(crate::graph::NodeKind::Fxr, 523002);
        assert_eq!(
            loaded.node(fxr_key).unwrap().status,
            crate::graph::NodeStatus::Resolved,
            "FXR payload should be re-hydrated on load"
        );
        assert!(loaded
            .nodes
            .iter()
            .any(|n| n.key.kind == crate::graph::NodeKind::Bullet && n.row.is_some()));
    }

    #[test]
    fn presentation_survives_the_cache_round_trip() {
        let root = temp_root("presentation");
        let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
        let names = souls_format::NameIndex::open_vendored().unwrap();
        let options = WalkOptions::everything();

        let cache = GraphCache::in_memory(&src, options.clone())
            .with_names(&names)
            .with_disk(DiskCache::open(&root, manifest("pres-hash")).unwrap());
        let first = cache.get(4000).unwrap();
        assert_eq!(
            first.presentation.as_ref().unwrap().name.as_deref(),
            Some("[Sorcery] Glintstone Pebble")
        );

        // Reload from disk in a fresh cache.
        let cache2 = GraphCache::in_memory(&src, options)
            .with_names(&names)
            .with_disk(DiskCache::open(&root, manifest("pres-hash")).unwrap());
        let loaded = cache2.get(4000).unwrap();
        assert_eq!(
            loaded.presentation.as_ref().unwrap().name.as_deref(),
            Some("[Sorcery] Glintstone Pebble")
        );
        assert!(structurally_equal(&loaded, &first));
        assert!(cache2.verify(4000).unwrap());
    }

    #[test]
    fn memoizes_within_a_session() {
        let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
        let cache = GraphCache::in_memory(&src, WalkOptions::default());
        let a = cache.get(4000).unwrap();
        let b = cache.get(4000).unwrap();
        assert!(
            Arc::ptr_eq(&a, &b),
            "second get should hit the in-memory cache"
        );
    }

    #[test]
    fn a_changed_regulation_hash_uses_a_separate_directory() {
        let root = temp_root("hashkey");
        let src = crate::test_support::fixture_from_csvs("GlintstonePebble");

        let cache_a = GraphCache::in_memory(&src, WalkOptions::default())
            .with_disk(DiskCache::open(&root, manifest("hash-a")).unwrap());
        cache_a.get(4000).unwrap();

        let disk_b = DiskCache::open(&root, manifest("hash-b")).unwrap();
        assert!(
            disk_b.cached_ids().is_empty(),
            "a different regulation hash must not see the old cache"
        );
        assert_ne!(cache_a.disk().unwrap().dir(), disk_b.dir());
    }

    #[test]
    fn stale_manifest_discards_cached_graphs() {
        let root = temp_root("stale");
        let src = crate::test_support::fixture_from_csvs("GlintstonePebble");

        let disk = DiskCache::open(&root, manifest("same-hash")).unwrap();
        let cache = GraphCache::in_memory(&src, WalkOptions::default()).with_disk(disk);
        cache.get(4000).unwrap();
        assert!(cache.disk().unwrap().cached_ids().contains(&4000));

        // Same regulation, but a different paramdex fingerprint => same directory, stale
        // contents, which must be cleared rather than silently reused.
        let mut changed = manifest("same-hash");
        changed.paramdex_fingerprint = "different".to_string();
        let disk2 = DiskCache::open(&root, changed).unwrap();
        assert!(
            disk2.cached_ids().is_empty(),
            "a paramdex change must invalidate cached graphs"
        );
    }

    #[test]
    fn corrupt_entry_is_discarded_and_rebuilt() {
        let root = temp_root("corrupt");
        let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
        let disk = DiskCache::open(&root, manifest("hash")).unwrap();
        let dir = disk.dir().to_path_buf();

        std::fs::write(dir.join("4000.json"), b"{ this is not json").unwrap();
        assert!(disk.read(4000).is_none(), "corrupt entry should not parse");
        assert!(
            !dir.join("4000.json").exists(),
            "corrupt entry should be deleted"
        );

        // ...and a get() still succeeds by walking.
        let cache = GraphCache::in_memory(&src, WalkOptions::default())
            .with_disk(DiskCache::open(&root, manifest("hash")).unwrap());
        assert_eq!(cache.get(4000).unwrap().magic_id, 4000);
    }

    #[test]
    fn hashing_is_content_based() {
        let dir = temp_root("hashing");
        let a = dir.join("a.bin");
        let b = dir.join("b.bin");
        std::fs::write(&a, b"hello").unwrap();
        std::fs::write(&b, b"hello").unwrap();
        assert_eq!(hash_file(&a).unwrap(), hash_file(&b).unwrap());

        std::fs::write(&b, b"hello!").unwrap();
        assert_ne!(hash_file(&a).unwrap(), hash_file(&b).unwrap());
    }

    #[test]
    fn paramdex_fingerprint_is_stable_and_real() {
        let defs = souls_format::locate::locate_paramdex_defs().unwrap();
        let a = paramdex_fingerprint(&defs).unwrap();
        let b = paramdex_fingerprint(&defs).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }
}
