// Docs: docs/spellcraft-engine/source.md

use std::collections::HashMap;
use std::sync::Arc;

use souls_format::param_bank::{ParamBank, ParamBankError};
use souls_format::paramdef::{DefType, Paramdef};
use souls_format::{ParamRow, RowResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ParamTable {
    Magic,
    Bullet,
    AtkPc,
    AtkNpc,
    SpEffect,
    SpEffectVfx,
    Goods,
}

impl ParamTable {
    pub fn entry_suffix(self) -> &'static str {
        match self {
            ParamTable::Magic => "Magic.param",
            ParamTable::Bullet => "Bullet.param",
            ParamTable::AtkPc => "AtkParam_Pc.param",
            ParamTable::AtkNpc => "AtkParam_Npc.param",
            ParamTable::SpEffect => "SpEffectParam.param",
            ParamTable::SpEffectVfx => "SpEffectVfxParam.param",
            ParamTable::Goods => "EquipParamGoods.param",
        }
    }

    pub fn name_list(self) -> &'static str {
        match self {
            ParamTable::Magic => "Magic",
            ParamTable::Bullet => "Bullet",
            ParamTable::AtkPc => "AtkParam_Pc",
            ParamTable::AtkNpc => "AtkParam_Npc",
            ParamTable::SpEffect => "SpEffectParam",
            ParamTable::SpEffectVfx => "SpEffectVfxParam",
            ParamTable::Goods => "EquipParamGoods",
        }
    }

    pub fn param_type(self) -> &'static str {
        match self {
            ParamTable::Magic => "MAGIC_PARAM_ST",
            ParamTable::Bullet => "BULLET_PARAM_ST",
            ParamTable::AtkPc | ParamTable::AtkNpc => "ATK_PARAM_ST",
            ParamTable::SpEffect => "SP_EFFECT_PARAM_ST",
            ParamTable::SpEffectVfx => "SP_EFFECT_VFX_PARAM_ST",
            ParamTable::Goods => "EQUIP_PARAM_GOODS_ST",
        }
    }
}

pub trait ParamSource {
    fn row(&self, table: ParamTable, id: i64) -> RowResult;

    fn has_row(&self, table: ParamTable, id: i64) -> bool {
        self.row(table, id).is_found()
    }
}

// --- Real data -------------------------------------------------------------------

pub struct RegulationSource {
    bank: ParamBank,
}

impl RegulationSource {
    pub fn new(bank: ParamBank) -> Self {
        RegulationSource { bank }
    }

    pub fn bank(&self) -> &ParamBank {
        &self.bank
    }

    fn lift(result: Result<RowResult, ParamBankError>) -> RowResult {
        match result {
            Ok(r) => r,
            Err(e) => RowResult::Undecodable {
                error: format!("table access failed: {e}"),
            },
        }
    }
}

impl ParamSource for RegulationSource {
    fn row(&self, table: ParamTable, id: i64) -> RowResult {
        Self::lift(self.bank.row(table.entry_suffix(), id))
    }

    fn has_row(&self, table: ParamTable, id: i64) -> bool {
        self.bank.has_row(table.entry_suffix(), id).unwrap_or(false)
    }
}

// --- Fixtures --------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("failed to read fixture '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("fixture CSV has no header row")]
    NoHeader,
    #[error("fixture CSV has no 'ID' column")]
    NoIdColumn,
    #[error("fixture CSV row {line} has {got} cells but the header has {want}")]
    ColumnCountMismatch {
        line: usize,
        got: usize,
        want: usize,
    },
    #[error("fixture CSV row {line} has an unparseable ID '{value}'")]
    BadId { line: usize, value: String },
}

#[derive(Default)]
pub struct FixtureSource {
    rows: HashMap<(ParamTable, i64), Arc<ParamRow>>,
}

impl FixtureSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, table: ParamTable, row: ParamRow) -> &mut Self {
        self.rows.insert((table, row.id), Arc::new(row));
        self
    }

    pub fn load_csv(
        &mut self,
        table: ParamTable,
        paramdef: &Paramdef,
        csv_text: &str,
    ) -> Result<usize, FixtureError> {
        let mut lines = csv_text.lines().filter(|l| !l.trim().is_empty());
        // The vendored exports end their header with a trailing comma, so a naive split
        // yields one more (empty) column than the data rows have. Drop trailing empties
        // from both sides rather than special-casing the header.
        let header = split_row(lines.next().ok_or(FixtureError::NoHeader)?);
        let id_col = header
            .iter()
            .position(|h| h.eq_ignore_ascii_case("ID"))
            .ok_or(FixtureError::NoIdColumn)?;

        let types: HashMap<&str, DefType> = paramdef
            .fields
            .iter()
            .map(|f| (f.internal_name.as_str(), f.display_type))
            .collect();

        let mut loaded = 0usize;
        for (offset, line) in lines.enumerate() {
            let line_no = offset + 2; // 1-based, past the header
            let cells = split_row(line);
            if cells.len() != header.len() {
                return Err(FixtureError::ColumnCountMismatch {
                    line: line_no,
                    got: cells.len(),
                    want: header.len(),
                });
            }

            let id: i64 = cells[id_col]
                .trim()
                .parse()
                .map_err(|_| FixtureError::BadId {
                    line: line_no,
                    value: cells[id_col].to_string(),
                })?;

            let mut fields = std::collections::BTreeMap::new();
            for (name, cell) in header.iter().zip(&cells) {
                let Some(ty) = types.get(name) else { continue };
                if let Some(value) = cell_to_value(*ty, cell.trim()) {
                    fields.insert((*name).to_string(), value);
                }
            }

            self.insert(table, ParamRow { id, fields });
            loaded += 1;
        }
        Ok(loaded)
    }

    pub fn load_csv_file(
        &mut self,
        table: ParamTable,
        paramdef: &Paramdef,
        path: &std::path::Path,
    ) -> Result<usize, FixtureError> {
        let text = std::fs::read_to_string(path).map_err(|source| FixtureError::Io {
            path: path.display().to_string(),
            source,
        })?;
        self.load_csv(table, paramdef, &text)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

fn split_row(line: &str) -> Vec<&str> {
    let mut cells: Vec<&str> = line.trim_end_matches(['\r', '\n']).split(',').collect();
    while cells.last().is_some_and(|c| c.trim().is_empty()) {
        cells.pop();
    }
    cells
}

fn cell_to_value(ty: DefType, cell: &str) -> Option<souls_format::paramdef::ParamValue> {
    use souls_format::paramdef::ParamValue;
    if cell.is_empty() {
        return None;
    }
    Some(match ty {
        DefType::F32 | DefType::Angle32 => ParamValue::F32(cell.parse().ok()?),
        DefType::F64 => ParamValue::F64(cell.parse().ok()?),
        DefType::FixStr | DefType::FixStrW => ParamValue::Str(cell.to_string()),
        // `dummy8` array cells look like `[0|0|0]`; they're padding and no edge is ever
        // read from one, so keeping the literal text is enough to preserve row shape.
        DefType::Dummy8 => ParamValue::Str(cell.to_string()),
        _ => ParamValue::I64(cell.parse().ok()?),
    })
}

impl ParamSource for FixtureSource {
    fn row(&self, table: ParamTable, id: i64) -> RowResult {
        match self.rows.get(&(table, id)) {
            Some(r) => RowResult::Found(Arc::clone(r)),
            None => RowResult::Missing,
        }
    }

    fn has_row(&self, table: ParamTable, id: i64) -> bool {
        self.rows.contains_key(&(table, id))
    }
}

pub fn int_row(id: i64, fields: &[(&str, i64)]) -> ParamRow {
    ParamRow {
        id,
        fields: fields
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_string(),
                    souls_format::paramdef::ParamValue::I64(*v),
                )
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use souls_format::ParamdefLibrary;
    #[test]
    fn loads_glintstone_pebble_from_vendored_csv() {
        let defs = ParamdefLibrary::open_vendored().expect("paramdex should open");
        let magic_def = defs.by_param_type("MAGIC_PARAM_ST").unwrap();
        let bullet_def = defs.by_param_type("BULLET_PARAM_ST").unwrap();
        let atk_def = defs.by_param_type("ATK_PARAM_ST").unwrap();

        let dir = crate::test_support::reference_dir().join("GlintstonePebble");
        let mut src = FixtureSource::new();
        src.load_csv_file(ParamTable::Magic, magic_def, &dir.join("Magic.csv"))
            .unwrap();
        src.load_csv_file(ParamTable::Bullet, bullet_def, &dir.join("Bullet.csv"))
            .unwrap();
        src.load_csv_file(ParamTable::AtkPc, atk_def, &dir.join("AtkParam_Pc.csv"))
            .unwrap();

        let magic = src.row(ParamTable::Magic, 4000);
        let magic = magic.found().expect("Magic 4000 should load");
        assert_eq!(magic.get_i64("refId1").unwrap(), 10400000);
        // The second root the prior spike ignored entirely.
        assert_eq!(magic.get_i64("refId2").unwrap(), 10400099);
        assert_eq!(magic.get_i64("refCategory1").unwrap(), 1);
        assert_eq!(magic.get_i64("castSfxId").unwrap(), 523000);

        let bullet = src.row(ParamTable::Bullet, 10400000);
        let bullet = bullet.found().expect("Bullet should load");
        assert_eq!(bullet.get_i64("atkId_Bullet").unwrap(), 40000);
        assert_eq!(bullet.get_i64("sfxId_Bullet").unwrap(), 523002);
        // Bullet free-slot sentinel is 0 (Atk's is -1) — see docs/known-offsets.md.
        assert_eq!(bullet.get_i64("spEffectId0").unwrap(), 0);

        let atk = src.row(ParamTable::AtkPc, 40000);
        let atk = atk.found().expect("Atk should load");
        assert_eq!(atk.get_i64("spEffectId0").unwrap(), -1);
        // f32 column, which the prior spike had to hand-roll a match for.
        assert!((atk.get_f32("knockbackDist").unwrap() - 0.7).abs() < 1e-6);

        // refId2's bullet was never exported — a genuinely absent row, which the walker
        // must record rather than treat as an error.
        assert!(matches!(
            src.row(ParamTable::Bullet, 10400099),
            RowResult::Missing
        ));
    }

    #[test]
    fn hand_authored_rows_work() {
        let mut src = FixtureSource::new();
        src.insert(
            ParamTable::Magic,
            int_row(1, &[("refId1", 100), ("refCategory1", 1)]),
        );
        let row = src.row(ParamTable::Magic, 1);
        assert_eq!(row.found().unwrap().get_i64("refId1").unwrap(), 100);
        assert!(matches!(src.row(ParamTable::Magic, 2), RowResult::Missing));
    }
}
