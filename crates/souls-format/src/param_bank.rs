// Docs: docs/souls-format/param_bank.md

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::paramdef::{decode_row, ParamRow};
use crate::paramdex::{ParamdefLibrary, ParamdexError};
use crate::regulation::{Regulation, RegulationError};

#[derive(Debug, Clone)]
pub enum RowResult {
    Found(Arc<ParamRow>),
    Missing,
    Undecodable {
        error: String,
    },
}

impl RowResult {
    pub fn found(&self) -> Option<&Arc<ParamRow>> {
        match self {
            RowResult::Found(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_found(&self) -> bool {
        matches!(self, RowResult::Found(_))
    }
}

#[derive(Debug, Clone)]
pub struct TableMeta {
    pub entry_name: String,
    pub param_type: String,
    pub data_version: i16,
    pub detected_size: i64,
    pub row_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ParamBankError {
    #[error(transparent)]
    Regulation(#[from] RegulationError),
    #[error(transparent)]
    Paramdex(#[from] ParamdexError),
}

struct TableCache {
    meta: TableMeta,
    paramdef: &'static crate::paramdef::Paramdef,
    raw: HashMap<i64, Vec<u8>>,
    decoded: HashMap<i64, RowResult>,
}

pub struct ParamBank {
    regulation: Regulation,
    defs: ParamdefLibrary,
    tables: Mutex<HashMap<String, Arc<Mutex<TableCache>>>>,
    table_parses: Mutex<usize>,
}

impl ParamBank {
    pub fn new(regulation: Regulation, defs: ParamdefLibrary) -> Self {
        ParamBank {
            regulation,
            defs,
            tables: Mutex::new(HashMap::new()),
            table_parses: Mutex::new(0),
        }
    }

    pub fn regulation(&self) -> &Regulation {
        &self.regulation
    }

    pub fn defs(&self) -> &ParamdefLibrary {
        &self.defs
    }

    pub fn table_parse_count(&self) -> usize {
        *self.table_parses.lock().expect("counter mutex poisoned")
    }

    fn table(&self, entry_suffix: &str) -> Result<Arc<Mutex<TableCache>>, ParamBankError> {
        let key = entry_suffix.to_lowercase();
        if let Some(hit) = self
            .tables
            .lock()
            .expect("table map mutex poisoned")
            .get(&key)
        {
            return Ok(Arc::clone(hit));
        }

        // Parse outside the map lock so a slow table parse doesn't block other tables.
        let table = self.regulation.param_table(entry_suffix)?;
        *self.table_parses.lock().expect("counter mutex poisoned") += 1;
        let paramdef = self.defs.by_param_type(&table.param_type)?;

        let meta = TableMeta {
            entry_name: table.name.clone(),
            param_type: table.param_type.clone(),
            data_version: table.data_version,
            detected_size: table.detected_size,
            row_count: table.rows.len(),
        };
        let raw: HashMap<i64, Vec<u8>> = table
            .rows
            .into_iter()
            .map(|r| (r.id as i64, r.bytes))
            .collect();

        let cache = Arc::new(Mutex::new(TableCache {
            meta,
            paramdef,
            raw,
            decoded: HashMap::new(),
        }));

        // Another thread may have raced us here; either entry is equally valid, so keep
        // whichever landed first rather than clobbering it.
        let mut tables = self.tables.lock().expect("table map mutex poisoned");
        Ok(Arc::clone(tables.entry(key).or_insert(cache)))
    }

    pub fn table_meta(&self, entry_suffix: &str) -> Result<TableMeta, ParamBankError> {
        let table = self.table(entry_suffix)?;
        let guard = table.lock().expect("table cache mutex poisoned");
        Ok(guard.meta.clone())
    }

    pub fn has_row(&self, entry_suffix: &str, id: i64) -> Result<bool, ParamBankError> {
        let table = self.table(entry_suffix)?;
        let guard = table.lock().expect("table cache mutex poisoned");
        Ok(guard.raw.contains_key(&id))
    }

    pub fn row(&self, entry_suffix: &str, id: i64) -> Result<RowResult, ParamBankError> {
        let table = self.table(entry_suffix)?;
        let mut guard = table.lock().expect("table cache mutex poisoned");

        if let Some(hit) = guard.decoded.get(&id) {
            return Ok(hit.clone());
        }

        let result = match guard.raw.get(&id) {
            None => RowResult::Missing,
            Some(bytes) => match decode_row(guard.paramdef, id, bytes) {
                Ok(row) => RowResult::Found(Arc::new(row)),
                Err(e) => RowResult::Undecodable {
                    error: e.to_string(),
                },
            },
        };
        guard.decoded.insert(id, result.clone());
        Ok(result)
    }

    pub fn row_ids(&self, entry_suffix: &str) -> Result<Vec<i64>, ParamBankError> {
        let table = self.table(entry_suffix)?;
        let guard = table.lock().expect("table cache mutex poisoned");
        Ok(guard.raw.keys().copied().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locate::game_regulation_bin;

    fn bank() -> Option<ParamBank> {
        let path = game_regulation_bin()?;
        let regulation = Regulation::open(&path).expect("regulation should open");
        let defs = ParamdefLibrary::open_vendored().expect("paramdex should open");
        Some(ParamBank::new(regulation, defs))
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn parses_each_table_exactly_once_regardless_of_row_count() {
        let Some(bank) = bank() else {
            eprintln!("skipping: no regulation.bin found");
            return;
        };

        // Many rows across four distinct tables.
        for _ in 0..3 {
            bank.row("Magic.param", 4000).unwrap();
            bank.row("Magic.param", 4720).unwrap();
            bank.row("Bullet.param", 10400000).unwrap();
            bank.row("AtkParam_Pc.param", 40000).unwrap();
            bank.row("SpEffectParam.param", 470).unwrap();
        }

        assert_eq!(
            bank.table_parse_count(),
            4,
            "each distinct table should be parsed exactly once, no matter how many rows are read"
        );
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn row_835_is_undecodable_but_not_an_error() {
        let Some(bank) = bank() else {
            eprintln!("skipping: no regulation.bin found");
            return;
        };

        // The known-bad row (docs/known-offsets.md): must degrade to a value, not an Err,
        // and must not poison neighbouring rows in the same table.
        let bad = bank
            .row("SpEffectParam.param", 835)
            .expect("table access should succeed");
        assert!(
            matches!(bad, RowResult::Undecodable { .. }),
            "row 835 should be Undecodable, got {bad:?}"
        );

        let good = bank
            .row("SpEffectParam.param", 470)
            .expect("table access should succeed");
        assert!(
            good.is_found(),
            "a healthy row in the same table must still decode"
        );
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn missing_row_is_missing_not_an_error() {
        let Some(bank) = bank() else {
            eprintln!("skipping: no regulation.bin found");
            return;
        };
        let r = bank
            .row("Magic.param", 99_999_999)
            .expect("table access should succeed");
        assert!(matches!(r, RowResult::Missing));
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn all_walker_tables_resolve_by_entry_suffix() {
        let Some(bank) = bank() else {
            eprintln!("skipping: no regulation.bin found");
            return;
        };

        for (suffix, expected_type) in [
            ("Magic.param", "MAGIC_PARAM_ST"),
            ("Bullet.param", "BULLET_PARAM_ST"),
            ("AtkParam_Pc.param", "ATK_PARAM_ST"),
            ("AtkParam_Npc.param", "ATK_PARAM_ST"),
            ("SpEffectParam.param", "SP_EFFECT_PARAM_ST"),
            ("SpEffectVfxParam.param", "SP_EFFECT_VFX_PARAM_ST"),
        ] {
            let meta = bank
                .table_meta(suffix)
                .unwrap_or_else(|e| panic!("{suffix} should resolve: {e}"));
            assert_eq!(
                meta.param_type, expected_type,
                "{suffix} has an unexpected param type"
            );
        }
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn speffect_vfx_bridge_reaches_real_sfx_ids() {
        let Some(bank) = bank() else {
            eprintln!("skipping: no regulation.bin found");
            return;
        };

        // Adula's Moonblade's SpEffect 1443101 has vfxId=260 / vfxId1=40260.
        let sp = bank.row("SpEffectParam.param", 1443101).unwrap();
        let sp = sp.found().expect("SpEffect 1443101 should decode");
        assert_eq!(sp.get_i64("vfxId").unwrap(), 260);
        assert_eq!(sp.get_i64("vfxId1").unwrap(), 40260);

        // ...and those index SpEffectVfxParam rows carrying real sfx ids.
        for (vfx_row, expected_midst) in [(260, 4270), (40260, 4020)] {
            let vfx = bank.row("SpEffectVfxParam.param", vfx_row).unwrap();
            let vfx = vfx
                .found()
                .unwrap_or_else(|| panic!("SpEffectVfx {vfx_row} should decode"));
            assert_eq!(
                vfx.get_i64("midstSfxId").unwrap(),
                expected_midst,
                "SpEffectVfx {vfx_row} midstSfxId"
            );
        }
    }
}
