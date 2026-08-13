// Docs: docs/spellcraft-engine/test_support.md

use std::path::PathBuf;

use souls_format::ParamdefLibrary;

use crate::source::{FixtureSource, ParamTable};

pub fn reference_dir() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/reference")
}

/// Builds a `FixtureSource` from one exported spell's CSVs, so a test needs no regulation.
///
/// Tables absent from `spell_dir` are skipped rather than failing: not every export includes
/// every table, and a fixture that only covers Magic/Bullet/AtkPc is still a valid one.
pub fn fixture_from_csvs(spell_dir: &str) -> FixtureSource {
    let defs = ParamdefLibrary::open_vendored().expect("paramdex should open");
    let dir = reference_dir().join(spell_dir);
    let mut src = FixtureSource::new();
    for (table, param_type, file) in [
        (ParamTable::Magic, "MAGIC_PARAM_ST", "Magic.csv"),
        (ParamTable::Bullet, "BULLET_PARAM_ST", "Bullet.csv"),
        (ParamTable::AtkPc, "ATK_PARAM_ST", "AtkParam_Pc.csv"),
        (
            ParamTable::SpEffect,
            "SP_EFFECT_PARAM_ST",
            "SpEffectParam.csv",
        ),
    ] {
        let path = dir.join(file);
        if !path.is_file() {
            continue; // not every export includes every table
        }
        let def = defs
            .by_param_type(param_type)
            .expect("paramdef should load");
        src.load_csv_file(table, def, &path)
            .unwrap_or_else(|e| panic!("{spell_dir}/{file}: {e}"));
    }
    src
}
