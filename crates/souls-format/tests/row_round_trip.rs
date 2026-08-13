// Docs: docs/souls-format/tests/row_round_trip.md

use souls_format::locate::{locate_paramdex_defs, locate_regulation_bin};
use souls_format::paramdef::{decode_row, encode_row};
use souls_format::{ParamdefLibrary, Regulation};

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn every_row_in_every_table_round_trips() {
    let Some(path) = locate_regulation_bin() else {
        eprintln!("skipping: no regulation.bin found");
        return;
    };
    let regulation = Regulation::open(&path).unwrap();
    let defs = ParamdefLibrary::open(&locate_paramdex_defs().unwrap()).unwrap();

    const DEF_SHORTER_THAN_ROW: &[&str] = &[
        "BUDDY_PARAM_ST",
        "CHR_MODEL_PARAM_ST",
        "POSTURE_CONTROL_PARAM_WEP_RIGHT_ST",
        "SIGN_PUDDLE_PARAM_ST",
        "SOUND_CUTSCENE_PARAM_ST",
    ];

    const OURS: &[&str] = &[
        "MAGIC_PARAM_ST",
        "BULLET_PARAM_ST",
        "ATK_PARAM_ST",
        "SP_EFFECT_PARAM_ST",
        "SP_EFFECT_VFX_PARAM_ST",
        "EQUIP_PARAM_GOODS_ST",
    ];

    let (mut tables, mut rows, mut skipped_tables) = (0usize, 0usize, 0usize);
    let mut our_types: std::collections::BTreeSet<String> = Default::default();
    let (mut our_tables, mut our_rows) = (0usize, 0usize);
    let mut refused_ambiguous = 0usize;
    let mut undecodable = Vec::new();
    let mut mismatched: Vec<String> = Vec::new();
    let mut size_mismatch: Vec<String> = Vec::new();

    for entry in regulation.entries() {
        let Ok(table) = regulation.param_table(&entry.name) else {
            skipped_tables += 1; // not every BND4 entry is a PARAM
            continue;
        };
        let Ok(def) = defs.by_param_type(&table.param_type) else {
            skipped_tables += 1; // no vendored paramdef for this table
            continue;
        };
        tables += 1;
        let ours = OURS.contains(&table.param_type.as_str());
        if ours {
            our_tables += 1;
            our_types.insert(table.param_type.clone());
        }

        if def.has_duplicate_field_names {
            // Encoding is refused for these; prove the refusal rather than skipping quietly.
            let decoded = decode_row(def, table.rows[0].id as i64, &table.rows[0].bytes).unwrap();
            assert!(
                encode_row(def, &decoded).is_err(),
                "{} repeats field names, so encoding must be refused",
                table.param_type
            );
            assert!(
                !ours,
                "a table we write must never have duplicate field names"
            );
            refused_ambiguous += 1;
            continue;
        }

        if def.describes_bytes() != table.detected_size as usize {
            assert!(
                DEF_SHORTER_THAN_ROW.contains(&table.param_type.as_str()),
                "{} : def describes {} bytes but rows are {} — new paramdef gap, investigate",
                table.param_type,
                def.describes_bytes(),
                table.detected_size
            );
            assert!(
                !ours,
                "a table we write must have a def covering the whole row"
            );
            continue;
        }

        for raw in &table.rows {
            let decoded = match decode_row(def, raw.id as i64, &raw.bytes) {
                Ok(d) => d,
                Err(e) => {
                    // A row that never decoded can never re-encode. The splice path simply
                    // never touches such a row; recording them keeps the count honest.
                    undecodable.push(format!("{}:{} {e}", table.param_type, raw.id));
                    continue;
                }
            };
            let reencoded = match encode_row(def, &decoded) {
                Ok(b) => b,
                Err(e) => {
                    mismatched.push(format!(
                        "{}:{} encode failed: {e}",
                        table.param_type, raw.id
                    ));
                    continue;
                }
            };
            rows += 1;
            if ours {
                our_rows += 1;
            }

            // The row's slot is `detected_size`; encode must fill exactly it.
            if reencoded.len() != raw.bytes.len() {
                if size_mismatch.len() < 10 {
                    // (kept as a nested if for the early `continue` below)
                    size_mismatch.push(format!(
                        "{}:{} encoded {} bytes, slot is {}",
                        table.param_type,
                        raw.id,
                        reencoded.len(),
                        raw.bytes.len()
                    ));
                }
                continue;
            }
            if reencoded != raw.bytes && mismatched.len() < 10 {
                {
                    let first = reencoded
                        .iter()
                        .zip(&raw.bytes)
                        .position(|(a, b)| a != b)
                        .unwrap();
                    mismatched.push(format!(
                        "{}:{} differs at +0x{first:x} (got {:#04x}, want {:#04x})",
                        table.param_type, raw.id, reencoded[first], raw.bytes[first]
                    ));
                }
            }
        }
    }

    println!("tables checked: {tables}  (skipped {skipped_tables}: not PARAM, or no vendored def)");
    println!("  of those, entries we would ever write: {our_tables} ({our_rows} rows) across {} param types", our_types.len());
    println!("  refused as ambiguous (duplicate field names): {refused_ambiguous}");
    println!("rows round-tripped: {rows}");
    println!("rows that never decoded: {}", undecodable.len());
    for u in undecodable.iter().take(5) {
        println!("    {u}");
    }
    for m in &size_mismatch {
        println!("  SIZE  {m}");
    }
    for m in &mismatched {
        println!("  DIFF  {m}");
    }

    // ATK_PARAM_ST lives in two BND4 entries (AtkParam_Pc and AtkParam_Npc), so entries
    // outnumber param types. It is the types that must all be covered.
    assert_eq!(
        our_types.len(),
        OURS.len(),
        "every spell param type must be present and checked, got {our_types:?}"
    );
    assert!(
        our_rows > 10_000,
        "expected thousands of rows in our tables, got {our_rows}"
    );
    assert!(rows > 100_000, "expected a large corpus, got {rows} rows");
    assert!(
        size_mismatch.is_empty(),
        "{} rows encoded to the wrong length",
        size_mismatch.len()
    );
    assert!(
        mismatched.is_empty(),
        "{} rows did not round-trip",
        mismatched.len()
    );
}
