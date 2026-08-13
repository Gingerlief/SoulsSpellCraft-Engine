// Docs: docs/souls-format/tests/rebuild_insert_row.md

#![cfg(feature = "write")]

use souls_format::locate::locate_regulation_bin;
use souls_format::rebuild::{insert_row, rebuild_param_entry};
use souls_format::regulation::parse_param_table;
use souls_format::Regulation;

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn inserting_one_row_grows_the_table_and_leaves_others_untouched() {
    let Some(reg_path) = locate_regulation_bin() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };
    let regulation = Regulation::open(&reg_path).unwrap();
    let entry = regulation.find_entry("FaceRangeParam.param").unwrap();
    let mut table = regulation.param_table("FaceRangeParam.param").unwrap();

    // Baseline: this table's zero-edit rebuild must be byte-identical before we trust an
    // insertion into it. Task 2's byte-exactness gate only covered Magic.param/
    // EquipParamGoods.param; this confirms the "same header shape" claim actually holds
    // byte-for-byte here too, on a 2-row table where trailing alignment or a short
    // param_type string could plausibly differ.
    let baseline = rebuild_param_entry(&table).unwrap();
    assert_eq!(
        baseline.len(),
        entry.bytes.len(),
        "FaceRangeParam.param: zero-edit rebuild length differs"
    );
    assert_eq!(
        baseline, entry.bytes,
        "FaceRangeParam.param: zero-edit rebuild is not byte-identical"
    );

    let original_row_count = table.rows.len();
    let original_rows: Vec<(i32, Vec<u8>)> =
        table.rows.iter().map(|r| (r.id, r.bytes.clone())).collect();

    let new_id = table.rows.iter().map(|r| r.id).max().unwrap() + 1;
    // Start from an existing row's bytes (guarantees the right length) but perturb one byte
    // so the new row's payload is distinguishable from every existing row's. Otherwise the
    // round-trip and untouched-rows assertions below would pass even if rebuild_param_entry
    // duplicated or swapped a payload, since parse_param_table doesn't decode fields.
    let mut new_bytes = table.rows[0].bytes.clone();
    new_bytes[0] = new_bytes[0].wrapping_add(1);
    assert!(
        table.rows.iter().all(|r| r.bytes != new_bytes),
        "test setup: new row must not duplicate an existing payload"
    );

    insert_row(&mut table, new_id, new_bytes.clone()).unwrap();
    assert_eq!(table.rows.len(), original_row_count + 1);

    let rebuilt_entry = rebuild_param_entry(&table).unwrap();
    let reparsed = parse_param_table("FaceRangeParam.param", &rebuilt_entry).unwrap();

    assert_eq!(reparsed.rows.len(), original_row_count + 1);
    let ids: Vec<i32> = reparsed.rows.iter().map(|r| r.id).collect();
    let sorted = {
        let mut s = ids.clone();
        s.sort_unstable();
        s
    };
    assert_eq!(ids, sorted, "row directory must stay sorted by id");

    let new_row = reparsed.rows.iter().find(|r| r.id == new_id).unwrap();
    assert_eq!(
        new_row.bytes, new_bytes,
        "new row's bytes did not round-trip"
    );

    for (id, bytes) in &original_rows {
        let row = reparsed.rows.iter().find(|r| r.id == *id).unwrap();
        assert_eq!(&row.bytes, bytes, "row {id} changed but should not have");
    }
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn inserting_a_duplicate_id_is_refused() {
    let Some(reg_path) = locate_regulation_bin() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };
    let regulation = Regulation::open(&reg_path).unwrap();
    let mut table = regulation.param_table("FaceRangeParam.param").unwrap();
    let existing_id = table.rows[0].id;
    let bytes = table.rows[0].bytes.clone();

    let err = insert_row(&mut table, existing_id, bytes).unwrap_err();
    assert!(matches!(
        err,
        souls_format::rebuild::RebuildError::RowAlreadyExists { id } if id == existing_id
    ));
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn inserting_a_wrongly_sized_row_is_refused() {
    let Some(reg_path) = locate_regulation_bin() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };
    let regulation = Regulation::open(&reg_path).unwrap();
    let mut table = regulation.param_table("FaceRangeParam.param").unwrap();
    let new_id = table.rows.iter().map(|r| r.id).max().unwrap() + 1;

    let err = insert_row(&mut table, new_id, vec![0u8; 3]).unwrap_err();
    assert!(matches!(
        err,
        souls_format::rebuild::RebuildError::RowSizeMismatch { .. }
    ));
}
