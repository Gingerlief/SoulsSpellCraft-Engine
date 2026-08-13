// Docs: docs/souls-format/tests/regulation_write.md

#![cfg(feature = "write")]

use souls_format::locate::{locate_paramdex_defs, locate_regulation_bin};
use souls_format::paramdef::{decode_row, encode_row};
use souls_format::write::RegulationWriter;
use souls_format::{ParamdefLibrary, Regulation};

fn original() -> Option<Vec<u8>> {
    let path = locate_regulation_bin()?;
    Some(std::fs::read(path).unwrap())
}

fn align_up_16(n: usize) -> usize {
    n.div_ceil(16) * 16
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn unmodified_write_reproduces_the_payload_exactly() {
    let Some(raw) = original() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };
    let before = souls_format::regulation::decrypt_and_unwrap_regulation(&raw).unwrap();

    let writer = RegulationWriter::open(&raw).unwrap();
    assert_eq!(
        writer.payload(),
        &before[..],
        "opening must not perturb the payload"
    );

    let rewritten = writer.finish().unwrap();
    let after = souls_format::regulation::decrypt_and_unwrap_regulation(&rewritten).unwrap();

    assert_eq!(after.len(), before.len(), "payload length changed");
    assert_eq!(
        after, before,
        "payload bytes changed with zero patches applied"
    );

    // Not byte-identical to the original file — we compress harder than FromSoft did — so
    // state the relationship rather than implying more.
    println!(
        "original {} bytes -> rewritten {} bytes ({:+.1}%)",
        raw.len(),
        rewritten.len(),
        (rewritten.len() as f64 / raw.len() as f64 - 1.0) * 100.0
    );
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn patching_a_field_changes_that_row_and_nothing_else() {
    let Some(raw) = original() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };
    let defs = ParamdefLibrary::open(&locate_paramdex_defs().unwrap()).unwrap();
    let def = defs.by_param_type("MAGIC_PARAM_ST").unwrap();

    // Glintstone Pebble's stamina cost — an innocuous scalar with an obvious value.
    let regulation = Regulation::open(&locate_regulation_bin().unwrap()).unwrap();
    let table = regulation.param_table("Magic.param").unwrap();
    let target = table.rows.iter().find(|r| r.id == 4000).unwrap();
    let mut row = decode_row(def, 4000, &target.bytes).unwrap();

    let field = "sortId";
    let before_value = row.get_i64(field).unwrap();
    row.set_i64(field, before_value + 1).unwrap();
    let expected_bytes = encode_row(def, &row).unwrap();
    assert_ne!(
        expected_bytes, target.bytes,
        "the edit must change the row's bytes"
    );

    let payload_before = souls_format::regulation::decrypt_and_unwrap_regulation(&raw).unwrap();
    let mut writer = RegulationWriter::open(&raw).unwrap();
    writer.patch_row("Magic.param", def, &row).unwrap();
    assert_eq!(writer.patch_count(), 1);

    // Exactly the row's slot differs, and it differs to precisely what we encoded.
    let payload_after = writer.payload();
    let differing: Vec<usize> = payload_before
        .iter()
        .zip(payload_after.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(!differing.is_empty(), "patch changed nothing");
    let (lo, hi) = (differing[0], *differing.last().unwrap());
    println!(
        "bytes changed: {}, spanning +0x{lo:x}..=+0x{hi:x}",
        differing.len()
    );
    assert!(
        hi - lo < target.bytes.len(),
        "changes spilled outside a single row slot"
    );

    // Round-trip the whole file and read the edited value back through the normal path.
    let rewritten = writer.finish().unwrap();
    let tmp = std::env::temp_dir().join("morro-write-test-regulation.bin");
    std::fs::write(&tmp, &rewritten).unwrap();
    let reopened = Regulation::open(&tmp).unwrap();
    let table2 = reopened.param_table("Magic.param").unwrap();
    let row2 = table2.rows.iter().find(|r| r.id == 4000).unwrap();
    assert_eq!(
        row2.bytes, expected_bytes,
        "row did not survive the round trip"
    );

    let decoded = decode_row(def, 4000, &row2.bytes).unwrap();
    assert_eq!(decoded.get_i64(field).unwrap(), before_value + 1);

    // Every other row in the table is untouched.
    let mut checked = 0;
    for (a, b) in table.rows.iter().zip(table2.rows.iter()) {
        assert_eq!(a.id, b.id, "row order changed");
        if a.id != 4000 {
            assert_eq!(a.bytes, b.bytes, "row {} changed but should not have", a.id);
            checked += 1;
        }
    }
    println!("{checked} sibling rows verified unchanged");
    std::fs::remove_file(&tmp).ok();
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn a_row_that_does_not_fit_is_refused() {
    let Some(raw) = original() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };
    let mut writer = RegulationWriter::open(&raw).unwrap();
    // One byte short of any real Magic row.
    let err = writer
        .patch_row_bytes("Magic.param", 4000, &[0u8; 8])
        .unwrap_err();
    println!("{err}");
    assert!(matches!(
        err,
        souls_format::write::WriteError::RowSizeMismatch { .. }
    ));
    assert_eq!(writer.patch_count(), 0, "a refused patch must not count");
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn unknown_table_and_row_are_refused() {
    let Some(raw) = original() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };
    let mut writer = RegulationWriter::open(&raw).unwrap();
    assert!(matches!(
        writer
            .patch_row_bytes("NoSuchTable.param", 1, &[0u8; 4])
            .unwrap_err(),
        souls_format::write::WriteError::NoSuchEntry(_)
    ));
    // Derive an absent id rather than assume one: Magic.param really does contain a row
    // 999999999 (a sentinel), and guessing cost a confusing failure.
    let regulation = Regulation::open(&locate_regulation_bin().unwrap()).unwrap();
    let table = regulation.param_table("Magic.param").unwrap();
    let absent = table.rows.iter().map(|r| r.id as i64).max().unwrap() + 1;
    let err = writer
        .patch_row_bytes("Magic.param", absent, &[0u8; 4])
        .unwrap_err();
    println!("missing row {absent} -> {err}");
    assert!(matches!(
        err,
        souls_format::write::WriteError::NoSuchRow { .. }
    ));
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn inserting_a_row_through_the_writer_survives_the_full_round_trip() {
    let Some(raw) = original() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };

    let regulation = Regulation::open(&locate_regulation_bin().unwrap()).unwrap();
    let table_before = regulation.param_table("FaceRangeParam.param").unwrap();
    let new_id = table_before.rows.iter().map(|r| r.id).max().unwrap() + 1;
    let new_bytes = table_before.rows[0].bytes.clone();

    // Every BND4 entry, in header order, before the insertion. Snapshotting the whole slice
    // (rather than looking each sibling up again later by name) sidesteps a real ambiguity in
    // `find_entry`'s substring-suffix matching: the real regulation.bin contains both
    // `WeatherParam.param` and `CutsceneGparamWeatherParam.param`, and the latter's name is
    // itself a string-suffix of the former's, so `find_entry("WeatherParam.param")` resolves
    // to the wrong entry (the first `ends_with` match in header order). Comparing by position
    // is both unambiguous and exactly the invariant that matters: insertion is defined to
    // never reorder or add/remove BND4 entries, only grow one and shift what follows it.
    let entries_before: Vec<(String, Vec<u8>)> = regulation
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.bytes.clone()))
        .collect();
    let modified_index = regulation
        .entries()
        .iter()
        .position(|e| e.name.to_lowercase().ends_with("facerangeparam.param"))
        .unwrap();

    let mut writer = RegulationWriter::open(&raw).unwrap();
    writer
        .insert_row_bytes("FaceRangeParam.param", new_id as i64, &new_bytes)
        .unwrap();

    let rewritten = writer.finish().unwrap();
    let tmp = std::env::temp_dir().join("morro-write-test-insert-regulation.bin");
    std::fs::write(&tmp, &rewritten).unwrap();
    let reopened = Regulation::open(&tmp).unwrap();

    let table_after = reopened.param_table("FaceRangeParam.param").unwrap();
    assert_eq!(table_after.rows.len(), table_before.rows.len() + 1);
    let new_row = table_after.rows.iter().find(|r| r.id == new_id).unwrap();
    assert_eq!(new_row.bytes, new_bytes);

    // Every BND4 entry other than the modified one, compared by position: same name, same
    // bytes, byte-identical to before the insertion.
    let entries_after = reopened.entries();
    assert_eq!(
        entries_after.len(),
        entries_before.len(),
        "BND4 entry count changed"
    );
    let mut checked = 0;
    for (i, ((name_before, bytes_before), after)) in
        entries_before.iter().zip(entries_after.iter()).enumerate()
    {
        assert_eq!(&after.name, name_before, "entry order changed at index {i}");
        if i == modified_index {
            continue;
        }
        assert_eq!(
            &after.bytes, bytes_before,
            "{} (index {i}) changed but should not have",
            after.name
        );
        checked += 1;
    }
    println!("{checked} sibling BND4 entries verified byte-identical");

    // The offset shift itself: every entry after the modified one moved by exactly
    // `align_up_16(new_len) - align_up_16(old_len)`; every entry before it is untouched.
    //
    // NEITHER this check NOR the container-wide alignment scan below can catch a
    // raw-length-delta bug on THIS table, and that is provable, not just unlucky test data:
    // `FaceRangeParam.param`'s growth is exactly `24 (new directory entry) + 824 (new row) =
    // 848` bytes, and 848 is itself a multiple of 16 (53*16). Whenever growth is a multiple
    // of 16, `align_up_16(old_len + growth) == align_up_16(old_len) + growth` identically
    // (adding a multiple of the alignment can't change the ceiling remainder), so the aligned
    // formula and the raw formula (`new_len - old_len`, which always equals `growth`) produce
    // the exact same shift -- the correct and buggy implementations write byte-identical
    // containers here, so no assertion on the output can tell them apart. See
    // `inserting_into_a_table_whose_growth_is_not_16_aligned_uses_the_aligned_shift` below for
    // the test that actually discriminates the two formulas (on `EquipParamGoods.param`,
    // whose growth of 200 bytes is not 16-aligned). What this block and the alignment scan
    // below DO prove: the rewrite is self-consistent and matches the 16-byte-aligned layout
    // the real file requires -- necessary, just not sufficient to pin down which formula
    // produced it, for this particular table.
    let modified_before = &regulation.entries()[modified_index];
    let modified_after = &entries_after[modified_index];
    let delta = modified_after.bytes.len() as i64 - modified_before.bytes.len() as i64;
    // Sanity: the entry itself did grow, and by the amount the table's own row math predicts
    // (one new 24-byte directory entry + one new `detected_size`-byte row payload).
    assert!(delta > 0, "the modified entry did not grow");
    for (i, (before, after)) in regulation
        .entries()
        .iter()
        .zip(entries_after.iter())
        .enumerate()
    {
        let expected_offset = if i <= modified_index {
            before.data_offset as i64
        } else {
            before.data_offset as i64
                + (align_up_16(modified_after.bytes.len())
                    - align_up_16(modified_before.bytes.len())) as i64
        };
        assert_eq!(
            after.data_offset as i64, expected_offset,
            "entry {i} ({}) data_offset shifted by the wrong amount",
            after.name
        );
    }

    // The property the game actually depends on: every entry's data starts at the
    // next-16-byte-aligned offset after the previous entry's data ends -- not just for the
    // modified entry's immediate neighbor, but across the whole rewritten container. As noted
    // above, this cannot distinguish the aligned formula from the raw one on THIS table (both
    // produce an already-aligned result here) -- it's a general self-consistency check, not
    // the discriminating one.
    assert_eq!(
        entries_after[0].data_offset % 16,
        0,
        "first entry's data_offset must stay 16-byte aligned"
    );
    for i in 0..entries_after.len() - 1 {
        let this_end = entries_after[i].data_offset + entries_after[i].bytes.len();
        let expected_next = align_up_16(this_end);
        assert_eq!(
            entries_after[i + 1].data_offset,
            expected_next,
            "entries {i}->{} are not 16-byte aligned after the rewrite",
            i + 1
        );
    }

    std::fs::remove_file(&tmp).ok();
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn inserting_into_a_table_whose_growth_is_not_16_aligned_uses_the_aligned_shift() {
    let Some(raw) = original() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };

    let regulation = Regulation::open(&locate_regulation_bin().unwrap()).unwrap();
    let table_before = regulation.param_table("EquipParamGoods.param").unwrap();
    let new_id = table_before.rows.iter().map(|r| r.id).max().unwrap() + 1;
    let new_bytes = table_before.rows[0].bytes.clone();

    let modified_index = regulation
        .entries()
        .iter()
        .position(|e| e.name.to_lowercase().ends_with("equipparamgoods.param"))
        .unwrap();
    let modified_before = &regulation.entries()[modified_index];
    let next_before = regulation.entries().get(modified_index + 1).expect(
        "EquipParamGoods.param must not be the last BND4 entry for this test to check a shift",
    );

    // Setup check: this table's growth must NOT be 16-aligned, or this test degenerates into
    // the same non-discriminating case as the `FaceRangeParam.param` test above.
    let growth = 24 + table_before.detected_size as usize;
    assert_ne!(
        growth % 16,
        0,
        "EquipParamGoods.param's growth ({growth}) is now 16-aligned -- this test no longer \
         discriminates the aligned-delta formula from a raw-length-delta bug; pick a \
         different table with detected_size % 16 != 8"
    );

    let mut writer = RegulationWriter::open(&raw).unwrap();
    writer
        .insert_row_bytes("EquipParamGoods.param", new_id as i64, &new_bytes)
        .unwrap();

    let rewritten = writer.finish().unwrap();
    let tmp = std::env::temp_dir().join("morro-write-test-insert-goods-regulation.bin");
    std::fs::write(&tmp, &rewritten).unwrap();
    let reopened = Regulation::open(&tmp).unwrap();

    let table_after = reopened.param_table("EquipParamGoods.param").unwrap();
    assert_eq!(table_after.rows.len(), table_before.rows.len() + 1);
    let new_row = table_after.rows.iter().find(|r| r.id == new_id).unwrap();
    assert_eq!(new_row.bytes, new_bytes);

    let modified_after = &reopened.entries()[modified_index];
    let next_after = &reopened.entries()[modified_index + 1];

    let raw_delta = modified_after.bytes.len() as i64 - modified_before.bytes.len() as i64;
    let aligned_delta = align_up_16(modified_after.bytes.len()) as i64
        - align_up_16(modified_before.bytes.len()) as i64;
    println!(
        "EquipParamGoods.param: old_len={} new_len={} raw_delta={raw_delta} aligned_delta={aligned_delta}",
        modified_before.bytes.len(),
        modified_after.bytes.len()
    );
    // Setup check: the whole point of using this table is that these two differ.
    assert_ne!(
        raw_delta, aligned_delta,
        "raw and aligned deltas coincide for this table too -- this test no longer \
         discriminates the two formulas"
    );

    let actual_shift = next_after.data_offset as i64 - next_before.data_offset as i64;
    assert_eq!(
        actual_shift, aligned_delta,
        "the entry after EquipParamGoods.param shifted by {actual_shift}, not the aligned \
         delta {aligned_delta} -- insert_row_bytes must use align_up_16(new_len) - \
         align_up_16(old_len), not the raw length delta"
    );
    assert_ne!(
        actual_shift, raw_delta,
        "the shift matches the raw (unaligned) length delta ({raw_delta}) -- this is exactly \
         the bug insert_row_bytes must avoid"
    );

    std::fs::remove_file(&tmp).ok();
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn inserting_into_two_tables_on_the_same_writer_composes_correctly() {
    let Some(raw) = original() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };

    let regulation = Regulation::open(&locate_regulation_bin().unwrap()).unwrap();
    let magic_before = regulation.param_table("Magic.param").unwrap();
    let goods_before = regulation.param_table("EquipParamGoods.param").unwrap();
    let new_magic_id = magic_before.rows.iter().map(|r| r.id).max().unwrap() + 1;
    let new_goods_id = goods_before.rows.iter().map(|r| r.id).max().unwrap() + 1;
    let new_magic_bytes = magic_before.rows[0].bytes.clone();
    let new_goods_bytes = goods_before.rows[0].bytes.clone();

    // Every BND4 entry, in header order, before either insertion. Compared by position rather
    // than looked up again by name afterward — see the single-insert test above for why
    // (`find_entry`'s suffix-match ambiguity between `WeatherParam.param` and
    // `CutsceneGparamWeatherParam.param`).
    let entries_before: Vec<(String, Vec<u8>)> = regulation
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.bytes.clone()))
        .collect();
    let modified_index_magic = regulation
        .entries()
        .iter()
        .position(|e| e.name.to_lowercase().ends_with("magic.param"))
        .unwrap();
    let modified_index_goods = regulation
        .entries()
        .iter()
        .position(|e| e.name.to_lowercase().ends_with("equipparamgoods.param"))
        .unwrap();

    let mut writer = RegulationWriter::open(&raw).unwrap();
    writer
        .insert_row_bytes("Magic.param", new_magic_id as i64, &new_magic_bytes)
        .unwrap();
    // The seam under test: this second call's internal `parse_bnd4` reads `self.payload` as
    // rewritten by the Magic insertion above, not the original file.
    writer
        .insert_row_bytes(
            "EquipParamGoods.param",
            new_goods_id as i64,
            &new_goods_bytes,
        )
        .unwrap();
    assert_eq!(writer.patch_count(), 2);

    let rewritten = writer.finish().unwrap();
    let tmp = std::env::temp_dir().join("morro-write-test-insert-two-tables-regulation.bin");
    std::fs::write(&tmp, &rewritten).unwrap();
    let reopened = Regulation::open(&tmp).unwrap();

    let magic_after = reopened.param_table("Magic.param").unwrap();
    assert_eq!(magic_after.rows.len(), magic_before.rows.len() + 1);
    let new_magic_row = magic_after
        .rows
        .iter()
        .find(|r| r.id == new_magic_id)
        .unwrap();
    assert_eq!(
        new_magic_row.bytes, new_magic_bytes,
        "new Magic row's bytes did not round-trip"
    );
    for row_before in &magic_before.rows {
        let row_after = magic_after
            .rows
            .iter()
            .find(|r| r.id == row_before.id)
            .unwrap();
        assert_eq!(
            row_after.bytes, row_before.bytes,
            "Magic row {} changed but should not have",
            row_before.id
        );
    }

    let goods_after = reopened.param_table("EquipParamGoods.param").unwrap();
    assert_eq!(goods_after.rows.len(), goods_before.rows.len() + 1);
    let new_goods_row = goods_after
        .rows
        .iter()
        .find(|r| r.id == new_goods_id)
        .unwrap();
    assert_eq!(
        new_goods_row.bytes, new_goods_bytes,
        "new Goods row's bytes did not round-trip"
    );
    for row_before in &goods_before.rows {
        let row_after = goods_after
            .rows
            .iter()
            .find(|r| r.id == row_before.id)
            .unwrap();
        assert_eq!(
            row_after.bytes, row_before.bytes,
            "Goods row {} changed but should not have",
            row_before.id
        );
    }

    // Every BND4 entry other than the two modified ones: same name, same bytes, byte-identical
    // to before either insertion.
    let entries_after = reopened.entries();
    assert_eq!(
        entries_after.len(),
        entries_before.len(),
        "BND4 entry count changed"
    );
    let mut checked = 0;
    for (i, ((name_before, bytes_before), after)) in
        entries_before.iter().zip(entries_after.iter()).enumerate()
    {
        assert_eq!(&after.name, name_before, "entry order changed at index {i}");
        if i == modified_index_magic || i == modified_index_goods {
            continue;
        }
        assert_eq!(
            &after.bytes, bytes_before,
            "{} (index {i}) changed but should not have",
            after.name
        );
        checked += 1;
    }
    println!(
        "{checked} sibling BND4 entries verified byte-identical across two sequential inserts \
         on the same writer"
    );

    std::fs::remove_file(&tmp).ok();
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn inserting_a_duplicate_row_through_the_writer_is_refused() {
    let Some(raw) = original() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };
    let mut writer = RegulationWriter::open(&raw).unwrap();
    let regulation = Regulation::open(&locate_regulation_bin().unwrap()).unwrap();
    let table = regulation.param_table("FaceRangeParam.param").unwrap();
    let existing_id = table.rows[0].id as i64;

    let err = writer
        .insert_row_bytes("FaceRangeParam.param", existing_id, &table.rows[0].bytes)
        .unwrap_err();
    println!("{err}");
    assert!(matches!(err, souls_format::write::WriteError::Rebuild(_)));
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn the_emitted_zstd_frame_matches_the_reference_writer() {
    let Some(raw) = original() else {
        eprintln!("skipping: no regulation.bin");
        return;
    };
    let written = RegulationWriter::open(&raw).unwrap().finish().unwrap();
    let decrypted = souls_format::regulation::decrypt_er_regulation(&written);

    let frame = &decrypted[0x4C..];
    assert_eq!(&frame[..4], &[0x28, 0xb5, 0x2f, 0xfd], "not a zstd frame");

    // Frame_Header_Descriptor: no content size, no checksum, no dictionary.
    let fhd = frame[4];
    assert_eq!(
        fhd, 0x00,
        "frame header descriptor must be 0x00, got {fhd:#04x}"
    );
    assert_eq!(fhd >> 6, 0, "contentSizeFlag must be off");
    assert_eq!(
        (fhd >> 5) & 1,
        0,
        "single-segment must be off, or no window byte follows"
    );

    // Window_Descriptor: windowLog = 10 + (byte >> 3).
    let window_log = 10 + (frame[5] >> 3) as u32;
    assert_eq!(
        window_log, 16,
        "windowLog must be pinned to 16 (descriptor 0x30); got {window_log} \
         (descriptor {:#04x}). A level-derived window is what the game rejects.",
        frame[5]
    );

    // And the declared level must agree with the frame we actually wrote.
    assert_eq!(
        decrypted[0x30] as i32,
        RegulationWriter::open(&raw).unwrap().zstd_level(),
        "the DCX header's level byte must match the frame"
    );
}
