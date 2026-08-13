// Docs: docs/souls-format/tests/rebuild_zero_change.md

#![cfg(feature = "write")]

use souls_format::locate::locate_regulation_bin;
use souls_format::rebuild::rebuild_param_entry;
use souls_format::Regulation;

fn regulation() -> Option<Regulation> {
    let path = locate_regulation_bin()?;
    Some(Regulation::open(&path).unwrap())
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn zero_change_rebuild_is_byte_identical() {
    let Some(regulation) = regulation() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };

    for suffix in ["Magic.param", "EquipParamGoods.param"] {
        let entry = regulation.find_entry(suffix).unwrap();
        let table = regulation.param_table(suffix).unwrap();
        let rebuilt = rebuild_param_entry(&table).unwrap();
        assert_eq!(
            rebuilt.len(),
            entry.bytes.len(),
            "{suffix}: rebuilt length differs"
        );
        assert_eq!(rebuilt, entry.bytes, "{suffix}: rebuilt bytes differ");
    }
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn subtree_tables_rebuild_byte_identically() {
    let Some(regulation) = regulation() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };

    for suffix in [
        "SpEffectParam.param",
        "SpEffectVfxParam.param",
        "AtkParam_Npc.param",
    ] {
        let entry = regulation.find_entry(suffix).unwrap();
        let table = regulation.param_table(suffix).unwrap();
        let rebuilt = rebuild_param_entry(&table).unwrap_or_else(|e| {
            panic!("{suffix}: rebuild failed — subtree crafting cannot insert here: {e}")
        });
        assert_eq!(
            rebuilt.len(),
            entry.bytes.len(),
            "{suffix}: rebuilt length differs"
        );
        assert_eq!(rebuilt, entry.bytes, "{suffix}: rebuilt bytes differ");
        eprintln!("{suffix}: {} rows rebuilt byte-identically", table.rows.len());
    }
}
