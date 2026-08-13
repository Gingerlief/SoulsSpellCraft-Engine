// Docs: docs/xtask/spike/gravity_pebble.md

use std::path::PathBuf;

use souls_format::paramdef::{ParamRow, ParamValue, Paramdef};
use souls_format::regulation::Regulation;

const SHELL_MAGIC_ID: i64 = 4000; // Glintstone Pebble
const PAYLOAD_MAGIC_ID: i64 = 4720; // Gravity Well
const TARGET_SLOT: i64 = 4004; // matches design.md's historical craft, for direct comparison

use souls_format::locate::{locate_paramdex_defs, locate_regulation_bin as find_regulation_bin};

fn paramdex_dir() -> PathBuf {
    locate_paramdex_defs().expect("vendored paramdex defs should be present")
}

fn field_i64(row: &ParamRow, name: &str) -> i64 {
    match row.fields.get(name) {
        Some(ParamValue::I64(v)) => *v,
        Some(other) => panic!(
            "field '{name}' on row {} is not an integer: {other:?}",
            row.id
        ),
        None => panic!(
            "field '{name}' not found on row {} (fields present: {:?})",
            row.id,
            row.fields.keys().collect::<Vec<_>>()
        ),
    }
}

fn set_field_i64(row: &mut ParamRow, name: &str, value: i64) {
    match row.fields.get_mut(name) {
        Some(ParamValue::I64(v)) => *v = value,
        Some(other) => panic!(
            "field '{name}' on row {} is not an integer: {other:?}",
            row.id
        ),
        None => panic!("field '{name}' not found on row {}", row.id),
    }
}

struct Ingredient {
    magic: ParamRow,
    bullet: ParamRow,
    atk: ParamRow,
}

fn load_ingredient(
    regulation: &Regulation,
    magic_def: &Paramdef,
    bullet_def: &Paramdef,
    atk_def: &Paramdef,
    magic_id: i64,
) -> Ingredient {
    let magic_table = regulation
        .param_table("Magic.param")
        .expect("Magic.param should parse");
    let magic_raw = magic_table
        .rows
        .iter()
        .find(|r| r.id as i64 == magic_id)
        .unwrap_or_else(|| panic!("Magic row {magic_id} not found"));
    let magic = souls_format::paramdef::decode_row(magic_def, magic_id, &magic_raw.bytes)
        .unwrap_or_else(|e| panic!("Magic row {magic_id} failed to decode: {e}"));

    let bullet_id = field_i64(&magic, "refId1");
    let bullet_table = regulation
        .param_table("Bullet.param")
        .expect("Bullet.param should parse");
    let bullet_raw = bullet_table
        .rows
        .iter()
        .find(|r| r.id as i64 == bullet_id)
        .unwrap_or_else(|| panic!("Bullet row {bullet_id} not found"));
    let bullet = souls_format::paramdef::decode_row(bullet_def, bullet_id, &bullet_raw.bytes)
        .unwrap_or_else(|e| panic!("Bullet row {bullet_id} failed to decode: {e}"));

    let atk_id = field_i64(&bullet, "atkId_Bullet");
    let atk_table = regulation
        .param_table("AtkParam_Pc.param")
        .expect("AtkParam_Pc.param should parse");
    let atk_raw = atk_table
        .rows
        .iter()
        .find(|r| r.id as i64 == atk_id)
        .unwrap_or_else(|| panic!("AtkParam_Pc row {atk_id} not found"));
    let atk = souls_format::paramdef::decode_row(atk_def, atk_id, &atk_raw.bytes)
        .unwrap_or_else(|e| panic!("AtkParam_Pc row {atk_id} failed to decode: {e}"));

    Ingredient { magic, bullet, atk }
}

const SP_EFFECT_SLOT_FIELDS: [&str; 5] = [
    "spEffectId0",
    "spEffectId1",
    "spEffectId2",
    "spEffectId3",
    "spEffectId4",
];

pub fn run() -> bool {
    println!("=== xtask spike gravity-pebble ===");

    let Some(regulation_bin) = find_regulation_bin() else {
        eprintln!("SKIP: no regulation.bin found (set SSC_REGULATION_BIN_PATH)");
        return true;
    };

    let regulation = match Regulation::open(&regulation_bin) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("FAIL: could not open regulation.bin: {e}");
            return false;
        }
    };

    let defs = paramdex_dir();
    let magic_def =
        Paramdef::load(&defs.join("MagicParam.xml")).expect("MagicParam.xml should load");
    let bullet_def =
        Paramdef::load(&defs.join("BulletParam.xml")).expect("BulletParam.xml should load");
    let atk_def = Paramdef::load(&defs.join("AtkParam.xml")).expect("AtkParam.xml should load");

    let shell = load_ingredient(
        &regulation,
        &magic_def,
        &bullet_def,
        &atk_def,
        SHELL_MAGIC_ID,
    );
    let payload = load_ingredient(
        &regulation,
        &magic_def,
        &bullet_def,
        &atk_def,
        PAYLOAD_MAGIC_ID,
    );

    let payload_knockback_preview = match payload.atk.fields.get("knockbackDist") {
        Some(ParamValue::F32(v)) => *v,
        Some(ParamValue::I64(v)) => *v as f32,
        other => panic!("payload atk.knockbackDist has unexpected type: {other:?}"),
    };
    println!(
        "shell (Glintstone Pebble {SHELL_MAGIC_ID}): bullet={} atk={} mp={} requirementIntellect={}",
        shell.bullet.id,
        shell.atk.id,
        field_i64(&shell.magic, "mp"),
        field_i64(&shell.magic, "requirementIntellect"),
    );
    println!(
        "payload (Gravity Well {PAYLOAD_MAGIC_ID}): bullet={} atk={} bullet.spEffectId0={} atk.knockbackDist={}",
        payload.bullet.id,
        payload.atk.id,
        field_i64(&payload.bullet, "spEffectId0"),
        payload_knockback_preview,
    );

    // --- Minimal OnHitMerge fusion slice ---
    // ID reallocation: new_bullet_id = target_slot * 100000; new_atk_id = new_bullet_id + 1
    // (confirmed against design.md's historical craft: slot 4004 -> refId1=400400000,
    // atkId_Bullet=400400001).
    let new_bullet_id = TARGET_SLOT * 100_000;
    let new_atk_id = new_bullet_id + 1;

    let mut crafted_magic = shell.magic.clone();
    set_field_i64(&mut crafted_magic, "refId1", new_bullet_id);
    // mp_charge cleared: no consume-1 ref exists for this craft (both ingredients are
    // simple tap-casts) -- see BasicSpellLiveEditService_v2.cs's "Clear unchargeable cast
    // costs" step.
    set_field_i64(&mut crafted_magic, "mp_charge", 0);

    let mut crafted_bullet = shell.bullet.clone();
    set_field_i64(&mut crafted_bullet, "atkId_Bullet", new_atk_id);

    let mut crafted_atk = shell.atk.clone();

    // Bullet-level status carry: payload's bullet spEffectId0 (470) into the first free
    // spEffectId* slot on the carrier's terminal bullet (bullet free-slot sentinel is 0,
    // confirmed from the shell's own bullet row).
    let payload_bullet_sp_effect = field_i64(&payload.bullet, "spEffectId0");
    let mut bullet_carry_slot: Option<&str> = None;
    if payload_bullet_sp_effect > 0 {
        for field in SP_EFFECT_SLOT_FIELDS {
            if field_i64(&crafted_bullet, field) <= 0 {
                set_field_i64(&mut crafted_bullet, field, payload_bullet_sp_effect);
                bullet_carry_slot = Some(field);
                break;
            }
        }
    }

    // Attack-level knockback carry: payload's *attack* knockbackDist, only if negative
    // (a positive payload knockback is ignored -- the carrier keeps its own hit reaction).
    let payload_knockback = payload_knockback_preview;
    if payload_knockback < 0.0 {
        match crafted_atk.fields.get_mut("knockbackDist") {
            Some(ParamValue::F32(v)) => *v = payload_knockback,
            Some(ParamValue::I64(v)) => *v = payload_knockback as i64,
            other => panic!("crafted atk.knockbackDist has unexpected type: {other:?}"),
        }
    }

    // --- Assertions, split by provenance (see module doc) ---
    let mut ok = true;
    let mut check_i64 = |label: &str, actual: i64, expected: i64| {
        let pass = actual == expected;
        ok &= pass;
        println!(
            "{} {label}: expected={expected} actual={actual}",
            if pass { "PASS" } else { "FAIL" }
        );
    };

    check_i64(
        "refId1 (new bullet id)",
        field_i64(&crafted_magic, "refId1"),
        new_bullet_id,
    );
    check_i64(
        "atkId_Bullet (new atk id)",
        field_i64(&crafted_bullet, "atkId_Bullet"),
        new_atk_id,
    );
    check_i64(
        "bullet spEffectId0 (carried pull)",
        field_i64(&crafted_bullet, "spEffectId0"),
        470,
    );
    check_i64(
        "mp_charge (cleared, no consume-1 ref)",
        field_i64(&crafted_magic, "mp_charge"),
        0,
    );

    let knockback_actual = match crafted_atk.fields.get("knockbackDist") {
        Some(ParamValue::F32(v)) => *v,
        Some(ParamValue::I64(v)) => *v as f32,
        other => panic!("unexpected type: {other:?}"),
    };
    let knockback_pass = (knockback_actual - (-3.0)).abs() < 0.01;
    ok &= knockback_pass;
    println!(
        "{} atk knockbackDist (carried): expected=-3 actual={knockback_actual}",
        if knockback_pass { "PASS" } else { "FAIL" }
    );

    println!(
        "bullet spEffectId carry landed in slot: {}",
        bullet_carry_slot.unwrap_or("<none -- no free slot or payload had nothing to carry>")
    );

    // mp / requirementIntellect: engine-deterministic baseline is the shell's own values,
    // NOT design.md's historical 19/17 (request-driven -- see module doc). Reported, not a
    // pass/fail gate, since there is no engine-computed expectation to check against.
    println!(
        "INFO mp (shell-copy baseline, engine-deterministic): {} -- design.md's historical 19 was request-driven, not reproduced here",
        field_i64(&crafted_magic, "mp")
    );
    println!(
        "INFO requirementIntellect (shell-copy baseline, engine-deterministic): {} -- design.md's historical 17 was request-driven, not reproduced here",
        field_i64(&crafted_magic, "requirementIntellect")
    );

    println!("=== {} ===", if ok { "PASS" } else { "FAIL" });
    ok
}
