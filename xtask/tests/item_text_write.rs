// Docs: docs/xtask/tests/item_text_write.md

use std::path::PathBuf;

use souls_format::item_text::{write_msgbnd, GOODS_CAPTION, GOODS_INFO, GOODS_NAME};
use souls_format::locate::{game_item_msgbnd, locate_oodle_dll};
use souls_format::oodle::Oodle;
use souls_format::ItemText;

fn scratch_dir() -> PathBuf {
    let base = std::env::var("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join("morro-item-text-write-test");
    std::fs::create_dir_all(&dir).expect("creating scratch dir");
    dir
}

#[test]
#[ignore = "reads the real game install and its Oodle DLL — see docs/known-offsets.md"]
fn write_then_read_back_survives_and_leaves_retail_ids_untouched() {
    // Absent is legitimate: the game msgbnd this needs is never committed, and may not be
    // on this machine at all. Skip, don't fail.
    let Some(src) = game_item_msgbnd() else {
        eprintln!("skipping: no item msgbnd found (set SSC_ITEM_MSGBND_PATH)");
        return;
    };

    // Present-but-wrong is not legitimate for the *specific* binder this test needs: the
    // pool counts below are calibrated to item_dlc02.msgbnd.dcx (base + both DLC). A
    // no-DLC install falls back to item.msgbnd.dcx (game_item_msgbnd's own documented
    // fallback), which is a real, non-broken machine difference rather than a defect — so
    // this one case skips too, same as the absent case, rather than hard-failing on
    // counts that were never going to match on this machine.
    let leaf = src
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !leaf.contains("dlc") {
        eprintln!(
            "skipping: located binder {} is base-only (no DLC), not item_dlc02.msgbnd.dcx \
             — the calibrated pool counts below don't apply on this machine",
            src.display()
        );
        return;
    }

    let Some(dll) = locate_oodle_dll() else {
        eprintln!("skipping: oo2core_6_win64.dll not found (set SSC_OODLE_DLL_PATH)");
        return;
    };
    let oodle = Oodle::load(&dll).expect("Oodle should load — the DLL was located above");

    let dir = scratch_dir();
    let out_binder = dir.join("item_dlc02.msgbnd.dcx");

    // Same id and text the design spec's own offline shape probe used ("The write
    // round-trip is proven offline") — 4002 is Gravity Pebble's real crafted target id, so
    // a regression here shows up against the craft this project has actually verified
    // in-game, not an arbitrary synthetic id.
    const CRAFTED_ID: i64 = 4002;
    let edits: &[(&str, i64, &str)] = &[
        (GOODS_NAME, CRAFTED_ID, "Gravity Pebble"),
        (
            GOODS_INFO,
            CRAFTED_ID,
            "Fires a gravity-infused glintstone pebble",
        ),
        (GOODS_CAPTION, CRAFTED_ID, "A crafted sorcery."),
    ];

    // Write: base pools only, reading from the pristine binder, writing into the scratch
    // dir. `write_msgbnd` already refuses if any edit fails its internal read-back, so a
    // non-error return here already means something — the assertions below are the
    // independent confirmation of what survived.
    write_msgbnd(&src, edits, &out_binder, &oodle)
        .expect("write_msgbnd failed — this is a real defect, not an absent-data case");

    // Read back through the binder itself rather than a JSON dump: that is now the same
    // path every real caller takes, with no intermediate file to hide a defect.
    let text = ItemText::from_msgbnd(&out_binder, &oodle)
        .expect("reading back the binder we just wrote");

    // The injected text survives.
    assert_eq!(text.name(CRAFTED_ID).as_deref(), Some("Gravity Pebble"));
    assert_eq!(
        text.info(CRAFTED_ID).as_deref(),
        Some("Fires a gravity-infused glintstone pebble")
    );
    assert_eq!(text.caption(CRAFTED_ID).as_deref(), Some("A crafted sorcery."));

    // The regression guard that actually matters: writing the crafted id must not have
    // corrupted or displaced retail entries. 4000 = Glintstone Pebble (the shell), 4720 =
    // Gravity Well (the payload) — the same two ids item_text.rs::resolves_real_game_text
    // checks against the unmodified dump.
    assert_eq!(text.name(4000).as_deref(), Some("Glintstone Pebble"));
    assert_eq!(text.name(4720).as_deref(), Some("Gravity Well"));

    // Pool counts after writing exactly one craft: retail (2338/2180/2177, measured
    // 2026-08-05 against base + both DLC — see item_text.rs::resolves_real_game_text) plus
    // the one crafted id, which existed as a null-text slot in every pool beforehand.
    assert_eq!(text.len(GOODS_NAME), 2339);
    assert_eq!(text.len(GOODS_INFO), 2181);
    assert_eq!(text.len(GOODS_CAPTION), 2178);
}
