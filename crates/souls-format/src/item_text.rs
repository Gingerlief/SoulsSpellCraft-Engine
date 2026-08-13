// Docs: docs/souls-format/item_text.md

use std::collections::HashMap;
use std::path::{ Path, PathBuf };
use std::collections::BTreeMap;
use crate::oodle::Oodle;
use serde::Deserialize;


pub const GOODS_NAME: &str = "GoodsName";
pub const GOODS_INFO: &str = "GoodsInfo";
pub const GOODS_CAPTION: &str = "GoodsCaption";
const POOLS: [&str; 3] = [GOODS_NAME, GOODS_INFO, GOODS_CAPTION];

const SUFFIXES: [&str; 3] = ["", "_dlc01", "_dlc02"];

#[derive(Debug, thiserror::Error)]
pub enum MsgbndError {
    #[error("failed to read '{path}': {source}")] Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)] Dcx(#[from] crate::dcx::DcxError),

    #[error(transparent)] Binder(#[from] crate::regulation::RegulationError),

    #[error("{file}: {source}")] Fmg {
        file: String,
        #[source]
        source: crate::fmg::FmgError,
    },

    #[error(transparent)]
    Bnd4Write(#[from] crate::bnd4::Bnd4WriteError),

    #[error("pool '{0}' is not present in the source binder — refusing to invent it")]
    PoolMissing(String),

    #[error("wrote the binder but {pool}[{id}] read back as {got:?}, not the text asked for")]
    WriteNotVerified {
        pool: String,
        id: i64,
        got: Option<String>,
    },
}

pub fn write_msgbnd(
    src: &Path,
    edits: &[(&str, i64, &str)],
    out: &Path,
    oodle: &Oodle,
) -> Result<(), MsgbndError> {
    let raw = std::fs::read(src).map_err(|source| MsgbndError::Io {
        path: src.display().to_string(),
        source,
    })?;
    let payload = crate::dcx::unwrap_krak(&raw, oodle)?;
    let (header, mut entries) = crate::regulation::parse_bnd4_full(&payload)?;

    // Group by pool so each FMG is parsed and rewritten exactly once.
    let mut by_pool: BTreeMap<&str, Vec<(i64, &str)>> = BTreeMap::new();
    for (pool, id, text) in edits {
        by_pool.entry(pool).or_default().push((*id, *text));
    }

    for (pool, pool_edits) in &by_pool {
        let file = format!("{pool}.fmg");
        let entry = entries
            .iter_mut()
            .find(|e| e.leaf_name().eq_ignore_ascii_case(&file))
            .ok_or_else(|| MsgbndError::PoolMissing(file.clone()))?;

        let mut fmg = crate::fmg::parse_entries(&entry.bytes).map_err(|source| {
            MsgbndError::Fmg {
                file: file.clone(),
                source,
            }
        })?;

        for (id, text) in pool_edits {
            let id32 = *id as i32;
            match fmg.iter_mut().find(|(existing, _)| *existing == id32) {
                Some((_, slot)) => *slot = Some((*text).to_string()),
                None => fmg.push((id32, Some((*text).to_string()))),
            }
        }

        entry.bytes = crate::fmg::write(&fmg);
    }

    let rebuilt = crate::dcx::wrap_krak(
        &crate::bnd4::write_bnd4(&header, &entries)?,
        oodle,
        crate::oodle::LEVEL_OPTIMAL2,
    )?;

    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|source| MsgbndError::Io {
            path: dir.display().to_string(),
            source,
        })?;
    }
    std::fs::write(out, &rebuilt).map_err(|source| MsgbndError::Io {
        path: out.display().to_string(),
        source,
    })?;

    // Read back through the ordinary reader before claiming success.
    let check = ItemText::from_msgbnd(out, oodle)?;
    for (pool, id, text) in edits {
        let got = check.get(pool, *id);
        if got.as_deref() != Some(*text) {
            return Err(MsgbndError::WriteNotVerified {
                pool: (*pool).to_string(),
                id: *id,
                got,
            });
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct ItemTextDoc {
    #[allow(dead_code)]
    source: String,
    pools: HashMap<String, HashMap<String, String>>,
}

pub struct ItemText {
    path: PathBuf,
    pools: HashMap<String, HashMap<i64, String>>,
    loaded: bool,
}

impl ItemText {
    pub fn open(path: &Path) -> Self {
        let pools = std::fs
            ::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<ItemTextDoc>(&text).ok())
            .map(|doc| {
                doc.pools
                    .into_iter()
                    .map(|(pool, entries)| {
                        let parsed = entries
                            .into_iter()
                            .filter_map(|(id, text)|
                                id
                                    .parse::<i64>()
                                    .ok()
                                    .map(|i| (i, text))
                            )
                            .collect();
                        (pool, parsed)
                    })
                    .collect::<HashMap<String, HashMap<i64, String>>>()
            });

        ItemText {
            path: path.to_path_buf(),
            loaded: pools.is_some(),
            pools: pools.unwrap_or_default(),
        }
    }

    pub fn from_msgbnd(binder: &Path, oodle: &Oodle) -> Result<Self, MsgbndError> {
    let raw = std::fs::read(binder).map_err(|source| MsgbndError::Io {
        path: binder.display().to_string(),
        source,
    })?;
    let payload = crate::dcx::unwrap_krak(&raw, oodle)?;
    let entries = crate::regulation::parse_bnd4(&payload)?;

    let mut pools: HashMap<String, HashMap<i64, String>> = HashMap::new();
    for family in POOLS {
        let mut merged: HashMap<i64, String> = HashMap::new();
        for suffix in SUFFIXES {
            let file = format!("{family}{suffix}.fmg");
            let Some(entry) = entries
                .iter()
                .find(|e| e.leaf_name().eq_ignore_ascii_case(&file)) else {
                continue;
            };
            let parsed = crate::fmg::parse(&entry.bytes).map_err(|source| {
                MsgbndError::Fmg {
                    file: file.clone(),
                    source,
                }
            })?;
            merged.extend(parsed.into_iter().map(|(id, text)| (id as i64, text)));
        }
        pools.insert(family.to_string(), merged);
    }

    Ok(ItemText {
        path: binder.to_path_buf(),
        loaded: true,
        pools,
    })
}
    pub fn write_json(&self, out: &Path) -> std::io::Result<()> {
        let pools: BTreeMap<&str, BTreeMap<i64, &str>> = self
            .pools
            .iter()
            .map(|(pool, entries)| {
                let sorted = entries.iter().map(|(id, text)| (*id, text.as_str())).collect();
                (pool.as_str(), sorted)
            })
            .collect();

        let doc = serde_json::json!({
            "source": self.path.display().to_string(),
            "pools": pools,
        });

        let json = serde_json::to_string(&doc)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(out, json)
    }

    pub fn open_generated() -> Self {
        Self::open(&crate::locate::generated_item_text_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn get(&self, pool: &str, id: i64) -> Option<String> {
        self.pools.get(pool)?.get(&id).cloned()
    }

    pub fn name(&self, id: i64) -> Option<String> {
        self.get(GOODS_NAME, id)
    }

    pub fn info(&self, id: i64) -> Option<String> {
        self.get(GOODS_INFO, id)
    }

    pub fn caption(&self, id: i64) -> Option<String> {
        self.get(GOODS_CAPTION, id)
    }

    pub fn len(&self, pool: &str) -> usize {
        self.pools.get(pool).map(HashMap::len).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.pools.values().all(HashMap::is_empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/item-text-sample.json")
    }

    #[test]
    #[ignore = "reads an external game install — see docs/known-offsets.md"]
    fn writes_and_reads_back_a_new_name() {
        let src = crate::test_support::msgbnd_path();
        let oodle = crate::test_support::oodle();

        let before = ItemText::from_msgbnd(&src, &oodle).expect("source should read");
        let out = std::env::temp_dir().join("ssc-item-text-write-test/item_dlc02.msgbnd.dcx");

        const NEW_ID: i64 = 9_990_001;
        let edits: &[(&str, i64, &str)] = &[
            (GOODS_NAME, 4000, "Glintstone Pebble (edited)"),
            (GOODS_NAME, NEW_ID, "Crafted Test Spell"),
            (GOODS_CAPTION, NEW_ID, "A spell that exists only in this test."),
        ];
        write_msgbnd(&src, edits, &out, &oodle).expect("write should succeed and self-verify");

        let after = ItemText::from_msgbnd(&out, &oodle).expect("output should read back");
        assert_eq!(after.name(4000).as_deref(), Some("Glintstone Pebble (edited)"));
        assert_eq!(after.name(NEW_ID).as_deref(), Some("Crafted Test Spell"));
        assert_eq!(
            after.caption(NEW_ID).as_deref(),
            Some("A spell that exists only in this test.")
        );

        // An edit must not cost any other entry: one id added to each touched pool, and the
        // untouched pool identical.
        assert_eq!(after.len(GOODS_NAME), before.len(GOODS_NAME) + 1);
        assert_eq!(after.len(GOODS_CAPTION), before.len(GOODS_CAPTION) + 1);
        assert_eq!(after.len(GOODS_INFO), before.len(GOODS_INFO));
        assert_eq!(after.name(4020).as_deref(), before.name(4020).as_deref());

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn resolves_all_three_pools() {
        let text = ItemText::open(&fixture());
        assert!(text.is_loaded());
        assert_eq!(text.name(4000).as_deref(), Some("Glintstone Pebble"));
        assert_eq!(text.name(4720).as_deref(), Some("Gravity Well"));
        assert_eq!(
            text.info(4720).as_deref(),
            Some("Pulls foes toward caster with gravity projectile")
        );
        assert_eq!(text.caption(4000).as_deref(), Some("The most basic glintstone sorcery."));
    }

    #[test]
    fn absent_id_and_absent_pool_are_none_not_errors() {
        let text = ItemText::open(&fixture());
        assert_eq!(text.name(999_999), None);
        // 4000 has a name and caption in the fixture but no info entry.
        assert_eq!(text.info(4000), None);
    }

    #[test]
    fn missing_file_yields_all_none() {
        let text = ItemText::open(&PathBuf::from("definitely/not/here.json"));
        assert!(!text.is_loaded());
        assert_eq!(text.name(4000), None);
        assert_eq!(text.info(4000), None);
        assert_eq!(text.caption(4000), None);
    }

    #[test]
    fn malformed_json_yields_all_none() {
        let dir = std::env::temp_dir().join("morro-item-text-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, b"{not json").unwrap();
        let text = ItemText::open(&path);
        assert!(!text.is_loaded());
        assert_eq!(text.name(4000), None);
    }

    #[test]
    fn len_reports_pool_sizes() {
        let text = ItemText::open(&fixture());
        assert_eq!(text.len("GoodsName"), 2);
        assert_eq!(text.len("GoodsInfo"), 1);
        assert_eq!(text.len("GoodsCaption"), 1);
        assert_eq!(text.len("NoSuchPool"), 0);
    }

    #[test]
    #[ignore = "needs a generated item-text dump — run `cargo run -p xtask -- msg dump`"]
    fn resolves_real_game_text() {
        let path = crate::locate::generated_item_text_path();
        if !path.is_file() {
            eprintln!("skipping: no dump at {}", path.display());
            return; // absent is legitimate
        }
        let text = ItemText::open(&path);
        assert!(
            text.is_loaded(),
            "dump exists at {} but did not parse - regenerate with `xtask msg dump`",
            path.display()
        );

        assert_eq!(text.name(4000).as_deref(), Some("Glintstone Pebble"));
        assert_eq!(text.name(4720).as_deref(), Some("Gravity Well"));
        assert_eq!(
            text.info(4720).as_deref(),
            Some("Pulls foes toward caster with gravity projectile")
        );

        // Measured 2026-08-05 against base + both DLC. A drop to the base-only counts
        // (1829/1676/1673) means the _dlc01 merge regressed.
        assert_eq!(text.len(GOODS_NAME), 2338);
        assert_eq!(text.len(GOODS_INFO), 2180);
        assert_eq!(text.len(GOODS_CAPTION), 2177);
    }
        #[test]
    #[ignore = "reads an external game install — see docs/known-offsets.md"]
    fn matches_the_csharp_baseline() {
        let binder = crate::test_support::msgbnd_path();
        let oodle = crate::test_support::oodle();

        let native = ItemText::from_msgbnd(&binder, &oodle).expect("native read should succeed");
        let oracle = ItemText::open_generated();
        assert!(
            oracle.is_loaded(),
            "no baseline at {} — run `xtask msg dump` first",
            oracle.path().display()
        );

        for pool in POOLS {
            let n = native.pools.get(pool).expect("native pool should exist");
            let o = oracle.pools.get(pool).expect("oracle pool should exist");
            eprintln!("{pool}: native {} / oracle {}", n.len(), o.len());

            let mut missing = Vec::new();
            let mut differing = Vec::new();
            for (id, text) in o {
                match n.get(id) {
                    None => missing.push(*id),
                    Some(t) if t != text => differing.push(*id),
                    Some(_) => {}
                }
            }
            let extra: Vec<i64> = n.keys().filter(|id| !o.contains_key(id)).copied().collect();

            let sample = |v: &[i64]| v[..v.len().min(5)].to_vec();
            assert!(missing.is_empty(), "{pool}: {} missing, e.g. {:?}", missing.len(), sample(&missing));
            assert!(differing.is_empty(), "{pool}: {} differ, e.g. {:?}", differing.len(), sample(&differing));
            assert!(extra.is_empty(), "{pool}: {} extra, e.g. {:?}", extra.len(), sample(&extra));
        }
    }
}
