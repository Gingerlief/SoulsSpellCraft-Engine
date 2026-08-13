// Docs: docs/xtask/craft.md

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use souls_format::locate::locate_regulation_bin;
use souls_format::paramdef::ParamValue;
use souls_format::write::RegulationWriter;
use souls_format::{ParamBank, ParamRow, Regulation, RowResult};
use spellcraft_engine::allocator::{
    is_empty_dummy, IdAllocator, PRIMARY_SLOT_RANGE, REF_ID_FIELDS, SECONDARY_SLOT_RANGE,
};
use spellcraft_engine::craft_patch::{CraftPatch, CraftRow};
use spellcraft_engine::export::{DocValue, DocumentSource};
use spellcraft_engine::graph::model::NodeKey;
use spellcraft_engine::patch::FieldEdit;
use spellcraft_engine::source::ParamSource;

use crate::env::{generated_dir, Env};
use crate::spell::{doc_value_of, same_value, set_value};

const SHELL_MAGIC_ID: i64 = 4000; // Glintstone Pebble, same shell craft_dummy.rs used.

// Gravity Pebble recipe (docs/planning/known-facts-carryover.md): Glintstone Pebble shell +
// Gravity Well payload, OnHitMerge fusion. Same ids `xtask/src/spike/gravity_pebble.rs` proved
// the field-carry rule against, in memory, on 2026-07-24.
const GP_SHELL_MAGIC_ID: i64 = 4000; // Glintstone Pebble
const GP_PAYLOAD_MAGIC_ID: i64 = 4720; // Gravity Well
const SP_EFFECT_SLOT_FIELDS: [&str; 5] = [
    "spEffectId0",
    "spEffectId1",
    "spEffectId2",
    "spEffectId3",
    "spEffectId4",
];

pub fn run(mut args: impl Iterator<Item = String>) -> ExitCode {
    match args.next().as_deref() {
        Some("new-dummy") => {
            let rest: Vec<String> = args.collect();
            dispatch(new_dummy(&rest))
        }
        Some("gravity-pebble") => {
            let rest: Vec<String> = args.collect();
            dispatch(gravity_pebble(&rest))
        }
        Some("list-dummies") => {
            let rest: Vec<String> = args.collect();
            dispatch(list_dummies(&rest))
        }
        Some("apply") => {
            let rest: Vec<String> = args.collect();
            dispatch(apply(&rest))
        }
        Some(other) => {
            eprintln!("unknown craft subcommand: {other}");
            usage();
            ExitCode::FAILURE
        }
        None => {
            usage();
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!("usage: xtask craft <new-dummy|list-dummies|gravity-pebble|apply> [args]");
    eprintln!();
    eprintln!("  new-dummy [--count <n>] [--out <file>]");
    eprintln!("        author a CraftPatch pre-allocating empty spell slots: allocates --count");
    eprintln!("        free slots (default 1), clones Glintstone Pebble's Magic/Goods rows for");
    eprintln!("        each, forces refId1..10 to -1 — one patch, so a batch costs one apply");
    eprintln!("        no item_dlc02 text is written: FMG addresses text by id into ranges that");
    eprintln!("        already cover these slots, so there is nothing to reserve ahead of a craft");
    eprintln!("        (default out: data/reference/generated/craft-patch/dummy.json)");
    eprintln!("  list-dummies");
    eprintln!("        list pre-allocated slots nobody has crafted into yet, derived live from");
    eprintln!("        the regulation (refId1..10 all -1) — no registry file to drift");
    eprintln!("  gravity-pebble [--out <file>] [--name <s>] [--info <s>] [--caption <s>]");
    eprintln!("        author a CraftPatch for the Gravity Pebble recipe (Glintstone Pebble");
    eprintln!("        shell + Gravity Well payload, OnHitMerge): allocates a free slot, derives");
    eprintln!("        bullet/atk ids from it, and carries the payload's bullet spEffectId0 and");
    eprintln!("        attack knockbackDist onto new Bullet/AtkParam_Pc rows — promotes the");
    eprintln!("        2026-07-24 gravity_pebble spike's proven field-carry rule into a real,");
    eprintln!("        reviewable document (default out:");
    eprintln!("        data/reference/generated/craft-patch/gravity-pebble.json)");
    eprintln!("        --name defaults to \"Gravity Pebble\"; the text is written to the");
    eprintln!("        msgbnd by `craft apply`, so the spell is named in-game");
    eprintln!("  apply <patch.json> [--out <regulation.bin>] [--force]");
    eprintln!("        insert a CraftPatch's rows into the located regulation, writing a NEW file");
    eprintln!("        (default out: launch/patched/regulation.bin — same file/profile");
    eprintln!("         `spell apply`/`spell repack` use: me3 launch --auto-detect -p");
    eprintln!("         launch/launch_er_patched.me3)");
    eprintln!("        never modifies the source regulation; refuses if the patch was authored");
    eprintln!("        against a different one, or if any row's id is already taken");
    eprintln!("        the msgbnd always travels beside the regulation, at");
    eprintln!("        <out's directory>/msg/engus/item_dlc02.msgbnd.dcx — so --out redirects");
    eprintln!("        both halves together, never just the regulation");
    eprintln!("        if the patch carries name/info/caption, writes that msgbnd so the spell");
    eprintln!("        is named in-game; a text-write failure fails the whole command. If the");
    eprintln!("        patch carries no text, any stale msgbnd left at that path from a prior,");
    eprintln!("        unrelated craft is removed instead, so the regulation and msgbnd can");
    eprintln!("        never disagree about which spell lives at a given id");
}

fn default_dummy_patch_path() -> PathBuf {
    generated_dir().join("craft-patch/dummy.json")
}

fn new_dummy(args: &[String]) -> Result<(), String> {
    let mut out: Option<PathBuf> = None;
    let mut count: usize = 1;
    let mut apply_now = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a file path")?));
            }
            "--count" => {
                i += 1;
                count = args
                    .get(i)
                    .ok_or("--count needs a number")?
                    .parse()
                    .map_err(|_| format!("--count is not a number: {}", args[i]))?;
                if count == 0 {
                    return Err("--count must be at least 1".into());
                }
            }
            "--apply" => apply_now = true,
            other => return Err(format!("unexpected argument: {other}")),
        }
        i += 1;
    }
    let out = out.unwrap_or_else(default_dummy_patch_path);

    let env = Env::prepare()?;
    let mut allocator = IdAllocator::from_param_bank(env.source.bank())
        .map_err(|e| format!("seeding allocator: {e}"))?;

    // One patch holding every slot's rows, rather than N patches: `craft apply` rewrites the
    // whole regulation per run, so N patches would mean N full rewrites for no benefit.
    // Every row carries an explicit id — `target_id` can only name one slot, and here it is
    // provenance for the batch rather than the id the rows use.
    let mut slots = Vec::with_capacity(count);
    let mut rows = Vec::with_capacity(count * 2);
    for _ in 0..count {
        let slot = allocator
            .allocate_slot(0)
            .map_err(|e| format!("allocating a slot: {e}"))?;
        slots.push(slot);

        let mut magic_overrides = BTreeMap::new();
        magic_overrides.insert("sortId".to_string(), DocValue::Int(slot));
        for field in REF_ID_FIELDS {
            magic_overrides.insert(field.to_string(), DocValue::Int(-1));
        }

        rows.push(CraftRow {
            table: "Magic.param".to_string(),
            clone_from: SHELL_MAGIC_ID,
            id: Some(slot),
            overrides: magic_overrides,
        });
        rows.push(CraftRow {
            table: "EquipParamGoods.param".to_string(),
            clone_from: SHELL_MAGIC_ID,
            id: Some(slot),
            overrides: BTreeMap::new(),
        });
    }
    eprintln!(
        "allocated {} slot(s): {}",
        slots.len(),
        slots
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut patch = CraftPatch::new(
        DocumentSource {
            regulation_sha256: Some(env.regulation_sha256.clone()),
            paramdex_fingerprint: Some(env.paramdex_fingerprint.clone()),
        },
        slots[0],
        rows,
    );
    if count > 1 {
        // Text stays unset: one CraftPatch names one id, and a batch has no single subject.
        // Dummies are identified through `craft list-dummies`, not by an in-game name.
        patch.note = Some(format!(
            "{count} empty spell slots pre-allocated for crafting: {}",
            slots
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(&patch).map_err(|e| format!("serializing: {e}"))?;
    std::fs::write(&out, &json).map_err(|e| format!("writing {}: {e}", out.display()))?;
    eprintln!("wrote {} ({} bytes)", out.display(), json.len());
    if apply_now {
        // A patch nobody applies leaves the slots existing only on paper — and
        // `list-dummies` reads the regulation, so a slot is not real until it is in one.
        // That gap is exactly where this command used to strand people.
        eprintln!();
        apply(&[out.display().to_string(), "--in-place".to_string()])?;
    }
    Ok(())
}

fn list_dummies(args: &[String]) -> Result<(), String> {
    if let Some(other) = args.first() {
        return Err(format!("unexpected argument: {other}"));
    }

    let env = Env::prepare()?;
    let bank = env.source.bank();
    let mut ids = bank
        .row_ids("Magic.param")
        .map_err(|e| format!("listing Magic rows: {e}"))?;
    ids.sort_unstable();

    let mut free = Vec::new();
    let mut unreadable = Vec::new();
    for id in ids {
        if !PRIMARY_SLOT_RANGE.contains(&id) && !SECONDARY_SLOT_RANGE.contains(&id) {
            continue;
        }
        match decoded_row(bank, "Magic.param", id) {
            Ok(row) => {
                if is_empty_dummy(&row) {
                    free.push(id);
                }
            }
            // A row that will not decode is reported, never silently treated as free —
            // "can I safely overwrite this" must not answer yes on missing information.
            Err(e) => unreadable.push((id, e)),
        }
    }

    if free.is_empty() {
        eprintln!("no free dummy slots — run `xtask craft new-dummy --count <n>` first");
    } else {
        eprintln!("{} free dummy slot(s):", free.len());
        for id in &free {
            println!("{id}");
        }
    }
    for (id, e) in &unreadable {
        eprintln!("warning: Magic row {id} in a slot range will not decode ({e}) — not offered");
    }
    Ok(())
}

fn default_gravity_pebble_patch_path() -> PathBuf {
    generated_dir().join("craft-patch/gravity-pebble.json")
}

fn decoded_row(bank: &ParamBank, table: &str, id: i64) -> Result<ParamRow, String> {
    match bank
        .row(table, id)
        .map_err(|e| format!("{table}: {e}"))?
    {
        RowResult::Found(r) => Ok((*r).clone()),
        RowResult::Missing => Err(format!("{table}: row {id} not found")),
        RowResult::Undecodable { error } => {
            Err(format!("{table}: row {id} will not decode: {error}"))
        }
    }
}

fn field_i64(row: &ParamRow, field: &str) -> Result<i64, String> {
    match row.fields.get(field) {
        Some(ParamValue::I64(v)) => Ok(*v),
        Some(other) => Err(format!(
            "field '{field}' is {}, not an integer",
            other.variant_name()
        )),
        None => Err(format!("no field '{field}' on this row")),
    }
}

fn gravity_pebble(args: &[String]) -> Result<(), String> {
    let mut out: Option<PathBuf> = None;
    let mut name: Option<String> = None;
    let mut info: Option<String> = None;
    let mut caption: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a file path")?));
            }
            "--name" => {
                i += 1;
                name = Some(args.get(i).ok_or("--name needs a value")?.clone());
            }
            "--info" => {
                i += 1;
                info = Some(args.get(i).ok_or("--info needs a value")?.clone());
            }
            "--caption" => {
                i += 1;
                caption = Some(args.get(i).ok_or("--caption needs a value")?.clone());
            }
            other => return Err(format!("unexpected argument: {other}")),
        }
        i += 1;
    }
    let out = out.unwrap_or_else(default_gravity_pebble_patch_path);

    let env = Env::prepare()?;
    let bank = env.source.bank();

    // This craft generates two rows beyond Magic/Goods (Bullet, Atk) — pass that through to
    // the row-budget check rather than the 0 `new_dummy` uses (which generates none).
    let mut allocator = IdAllocator::from_param_bank(bank)
        .map_err(|e| format!("seeding allocator: {e}"))?;
    let target_id = allocator
        .allocate_slot(2)
        .map_err(|e| format!("allocating a slot: {e}"))?;
    eprintln!("allocated slot: {target_id}");

    // Derived-id formula confirmed against real data (docs/known-offsets.md, and this
    // session's shape-gate test): target_slot * 100_000 for the bullet, +1 for the atk.
    let new_bullet_id = target_id * 100_000;
    let new_atk_id = new_bullet_id + 1;

    let shell_magic = decoded_row(bank, "Magic.param", GP_SHELL_MAGIC_ID)?;
    let shell_bullet_id = field_i64(&shell_magic, "refId1")?;
    let shell_bullet = decoded_row(bank, "Bullet.param", shell_bullet_id)?;
    let shell_atk_id = field_i64(&shell_bullet, "atkId_Bullet")?;

    let payload_magic = decoded_row(bank, "Magic.param", GP_PAYLOAD_MAGIC_ID)?;
    let payload_bullet_id = field_i64(&payload_magic, "refId1")?;
    let payload_bullet = decoded_row(bank, "Bullet.param", payload_bullet_id)?;
    let payload_atk_id = field_i64(&payload_bullet, "atkId_Bullet")?;
    let payload_atk = decoded_row(bank, "AtkParam_Pc.param", payload_atk_id)?;

    // Bullet-level carry: payload's bullet spEffectId0 into the shell bullet's first free
    // spEffectId* slot (free-slot sentinel for Bullet.param is 0/negative, matching the spike).
    let payload_speffect = field_i64(&payload_bullet, "spEffectId0")?;
    let mut bullet_overrides = BTreeMap::new();
    bullet_overrides.insert("atkId_Bullet".to_string(), DocValue::Int(new_atk_id));
    let mut bullet_carry_slot: Option<&str> = None;
    if payload_speffect > 0 {
        for field in SP_EFFECT_SLOT_FIELDS {
            if field_i64(&shell_bullet, field)? <= 0 {
                bullet_overrides.insert(field.to_string(), DocValue::Int(payload_speffect));
                bullet_carry_slot = Some(field);
                break;
            }
        }
    }

    // Attack-level carry: payload's attack knockbackDist, only if negative (a positive payload
    // knockback is ignored — the carrier keeps its own hit reaction). Uses `doc_value_of` so
    // the override carries whatever numeric type the field actually decodes as, rather than
    // the spike's manual F32/I64 branching.
    let mut atk_overrides = BTreeMap::new();
    let knockback_is_negative = match payload_atk.fields.get("knockbackDist") {
        Some(ParamValue::F32(v)) => *v < 0.0,
        Some(ParamValue::I64(v)) => *v < 0,
        other => {
            return Err(format!(
                "payload atk.knockbackDist has unexpected type: {other:?}"
            ))
        }
    };
    if knockback_is_negative {
        let value = doc_value_of(&payload_atk, "knockbackDist")
            .ok_or("payload atk row has no knockbackDist field")?;
        atk_overrides.insert("knockbackDist".to_string(), value);
    }

    // Magic-level overrides: refId1 to the new bullet, mp_charge cleared (this recipe's two
    // ingredients are both simple tap-casts, per the spike). sortId is new here, not something
    // the spike validated — added for menu sanity, matching new_dummy's existing practice.
    let mut magic_overrides = BTreeMap::new();
    magic_overrides.insert("sortId".to_string(), DocValue::Int(target_id));
    magic_overrides.insert("refId1".to_string(), DocValue::Int(new_bullet_id));
    magic_overrides.insert("mp_charge".to_string(), DocValue::Int(0));

    let rows = vec![
        CraftRow {
            table: "Magic.param".to_string(),
            clone_from: GP_SHELL_MAGIC_ID,
            id: None,
            overrides: magic_overrides,
        },
        CraftRow {
            table: "EquipParamGoods.param".to_string(),
            clone_from: GP_SHELL_MAGIC_ID,
            id: None,
            overrides: BTreeMap::new(),
        },
        CraftRow {
            table: "Bullet.param".to_string(),
            clone_from: shell_bullet_id,
            id: Some(new_bullet_id),
            overrides: bullet_overrides,
        },
        CraftRow {
            table: "AtkParam_Pc.param".to_string(),
            clone_from: shell_atk_id,
            id: Some(new_atk_id),
            overrides: atk_overrides,
        },
    ];

    let mut patch = CraftPatch::new(
        DocumentSource {
            regulation_sha256: Some(env.regulation_sha256.clone()),
            paramdex_fingerprint: Some(env.paramdex_fingerprint.clone()),
        },
        target_id,
        rows,
    );
    patch.note = Some(
        "Gravity Pebble: Glintstone Pebble shell + Gravity Well OnHitMerge \
         (bullet spEffectId0 carry, atk knockbackDist carry)"
            .to_string(),
    );
    // Default the name so a craft is never anonymous in-game; --name overrides it.
    patch.name = Some(name.unwrap_or_else(|| "Gravity Pebble".to_string()));
    patch.info = info;
    patch.caption = caption;
    eprintln!("text:       name={:?}", patch.name);

    eprintln!(
        "derived: bullet={new_bullet_id} atk={new_atk_id} (shell bullet={shell_bullet_id} atk={shell_atk_id})"
    );
    eprintln!(
        "carried: bullet spEffectId slot={} atk knockbackDist negative={knockback_is_negative}",
        bullet_carry_slot.unwrap_or("<none>")
    );

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(&patch).map_err(|e| format!("serializing: {e}"))?;
    std::fs::write(&out, &json).map_err(|e| format!("writing {}: {e}", out.display()))?;
    eprintln!("wrote {} ({} bytes)", out.display(), json.len());
    Ok(())
}

fn is_game_install(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase().replace('\\', "/");
    lower.contains("/elden ring/game/") || lower.ends_with("/elden ring/game/regulation.bin")
}

fn apply(args: &[String]) -> Result<(), String> {
    let mut patch_path: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut force = false;
    let mut in_place = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a file path")?));
            }
            "--force" => force = true,
            "--in-place" => in_place = true,
            other => patch_path = Some(PathBuf::from(other)),
        }
        i += 1;
    }
    let patch_path = patch_path
        .ok_or("usage: xtask craft apply <patch.json> [--out <file>] [--force] [--in-place]")?;
    if in_place && out.is_some() {
        return Err("--in-place and --out contradict each other — pick one".into());
    }
    // In place means "advance the working regulation": read it, and write the result back
    // over it. That is what lets a craft build on a slot an earlier apply allocated, which
    // reading pristine every time cannot do. Guarded below so it can never touch the game's
    // own file.
    let out = match (in_place, out) {
        (true, _) => locate_regulation_bin().ok_or("no regulation.bin found")?,
        (false, Some(o)) => o,
        (false, None) => crate::env::default_patched_regulation_path(),
    };

    let text = std::fs::read_to_string(&patch_path)
        .map_err(|e| format!("reading {}: {e}", patch_path.display()))?;
    let patch: CraftPatch = serde_json::from_str(&text)
        .map_err(|e| format!("parsing {}: {e}", patch_path.display()))?;

    let env = Env::prepare()?;
    let bank = env.source.bank();
    patch
        .check_applicable(&env.regulation_sha256, |table, id| {
            bank.has_row(table, id).unwrap_or(false)
        })
        .map_err(|e| e.to_string())?;

    eprintln!(
        "patch:      {} (target id {}, {} row(s))",
        patch_path.display(),
        patch.target_id,
        patch.rows.len()
    );
    eprintln!(
        "            {} edit(s), {} text pool(s)",
        patch.edits.len(),
        patch.text_edits().len()
    );
    if let Some(note) = &patch.note {
        eprintln!("note:       {note}");
    }
    if patch.is_noop() {
        return Err(
            "patch does nothing: no rows to insert, no edits, and no name/info/caption".into(),
        );
    }

    let source_path = locate_regulation_bin().ok_or("no regulation.bin found")?;
    if out == source_path && !in_place {
        return Err(
            "refusing to write over the source regulation — pass --in-place if you meant to \
             advance a working regulation"
                .into(),
        );
    }
    // The one file that is never a working regulation. Advancing a copy is ordinary; writing
    // into the player's install is not something a flag should be able to authorise.
    if in_place && is_game_install(&source_path) {
        return Err(format!(
            "--in-place refuses to modify the game's own regulation at {} — point \
             SSC_REGULATION_BIN_PATH at a working copy first",
            source_path.display()
        ));
    }
    if out.exists() && !force && !in_place {
        return Err(format!(
            "{} already exists — pass --force to replace it",
            out.display()
        ));
    }

    let raw = std::fs::read(&source_path).map_err(|e| format!("reading regulation: {e}"))?;
    let mut writer =
        RegulationWriter::open(&raw).map_err(|e| format!("opening regulation: {e}"))?;
    // Printed because it dominates this command's runtime and is settable from outside it —
    // a slow run should say why, rather than leaving the reader to guess.
    eprintln!(
        "zstd level: {} ({})",
        writer.zstd_level(),
        if std::env::var(souls_format::write::ZSTD_LEVEL_ENV).is_ok() {
            "from SSC_ZSTD_LEVEL"
        } else {
            "from the source header"
        }
    );

    for row_spec in &patch.rows {
        let target_id = row_spec.id.unwrap_or(patch.target_id);
        let meta = bank
            .table_meta(&row_spec.table)
            .map_err(|e| format!("{}: {e}", row_spec.table))?;
        let def = env
            .defs
            .by_param_type(&meta.param_type)
            .map_err(|e| format!("{}: {e}", row_spec.table))?;

        let shell: ParamRow = match bank
            .row(&row_spec.table, row_spec.clone_from)
            .map_err(|e| format!("{}: {e}", row_spec.table))?
        {
            RowResult::Found(r) => (*r).clone(),
            RowResult::Missing => {
                return Err(format!(
                    "{}: clone source {} not found",
                    row_spec.table, row_spec.clone_from
                ))
            }
            RowResult::Undecodable { error } => {
                return Err(format!(
                    "{}: clone source {} will not decode: {error}",
                    row_spec.table, row_spec.clone_from
                ))
            }
        };

        let mut new_row = shell;
        new_row.id = target_id;
        for (field, value) in &row_spec.overrides {
            set_value(&mut new_row, field, value)
                .map_err(|e| format!("{}.{field}: {e}", row_spec.table))?;
        }

        writer
            .insert_row(&row_spec.table, def, &new_row)
            .map_err(|e| format!("{}: {e}", row_spec.table))?;
        eprintln!(
            "  {}: cloned {} -> {target_id} ({} override(s))",
            row_spec.table,
            row_spec.clone_from,
            row_spec.overrides.len()
        );
    }

    apply_edits(&patch, &mut writer, &env)?;

    let bytes = writer
        .finish()
        .map_err(|e| format!("re-encoding regulation: {e}"))?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    if in_place {
        // Write beside the target and rename over it, so an interrupted write cannot leave
        // the working regulation truncated. Overwriting it directly would destroy the only
        // copy of every craft applied before this one.
        let staging = out.with_extension("bin.new");
        std::fs::write(&staging, &bytes)
            .map_err(|e| format!("writing {}: {e}", staging.display()))?;
        std::fs::rename(&staging, &out)
            .map_err(|e| format!("replacing {}: {e}", out.display()))?;
    } else {
        std::fs::write(&out, &bytes).map_err(|e| format!("writing {}: {e}", out.display()))?;
    }

    verify(&out, &patch, &env)?;

    // Text is written after the regulation verifies, so a failed row insert never leaves a
    // named-but-absent spell behind. A text failure is a hard error: silently degrading to
    // an unnamed craft is exactly the "was it applied?" ambiguity CraftPatch removed.
    //
    // The binder path is derived from `out`'s own directory, never hardcoded, so the two
    // halves of a craft can never be separated by `--out` — see Important 4, final review
    // 2026-08-05: a hardcoded path meant `--out` redirected the regulation but silently
    // left the msgbnd in the ME3-served directory the caller didn't ask to touch.
    let binder_path = crate::msg::patched_binder_path_for(&out);
    let text_edits = patch.text_edits();
    if text_edits.is_empty() {
        // Deliberately leaves the msgbnd alone.
        //
        // This used to delete it. That was right when the binder was regenerated from the
        // game's pristine copy on every craft: a leftover from a *previous, unrelated* craft
        // would name a different spell at the same id (Important 3, final review 2026-08-05,
        // and the Spell_Making_UI drift incident `docs/planning/working-agreement.md` names).
        //
        // The binder at `binder_path` is now the working copy itself — seeded by `xtask init`
        // and edited in place, so it holds all of the game's item text plus every name from
        // earlier crafts. Deleting it would throw all of that away to avoid a staleness that
        // can no longer happen, because a craft that names nothing changes no text. Re-running
        // `xtask init --force` is the reset.
        eprintln!(
            "text:       none in this patch — leaving {} as it is",
            binder_path.display()
        );
    } else {
        crate::msg::write_text_for_patch(&text_edits, &binder_path)?;
    }

    eprintln!();
    eprintln!(
        "inserted {} row(s), edited {} field(s), at target id {}",
        patch.rows.len(),
        patch.edits.len(),
        patch.target_id
    );
    eprintln!(
        "wrote {} ({:.1} MB)",
        out.display(),
        bytes.len() as f64 / 1_048_576.0
    );
    if in_place {
        eprintln!("advanced in place: {}", source_path.display());
    } else {
        eprintln!("source regulation untouched: {}", source_path.display());
    }
    Ok(())
}

fn apply_edits(
    patch: &CraftPatch,
    writer: &mut RegulationWriter,
    env: &Env,
) -> Result<(), String> {
    if patch.edits.is_empty() {
        return Ok(());
    }

    let mut by_node: BTreeMap<String, Vec<&FieldEdit>> = BTreeMap::new();
    for edit in &patch.edits {
        by_node.entry(edit.node.clone()).or_default().push(edit);
    }

    for (node, edits) in &by_node {
        let key: NodeKey = node.parse().map_err(|e: String| e)?;
        let table = key
            .kind
            .table()
            .ok_or_else(|| format!("{node} is not a param row and cannot be edited"))?;
        let def = env
            .defs
            .by_param_type(table.param_type())
            .map_err(|e| format!("{node}: {e}"))?;

        let mut row = match env.source.row(table, key.id) {
            RowResult::Found(r) => (*r).clone(),
            RowResult::Missing => return Err(format!("{node}: no such row in the regulation")),
            RowResult::Undecodable { error } => {
                return Err(format!(
                    "{node}: row will not decode, so it cannot be edited: {error}"
                ))
            }
        };

        for edit in edits {
            if let Some(expected) = &edit.expected {
                let actual = doc_value_of(&row, &edit.field)
                    .ok_or_else(|| format!("{node}.{}: no such field", edit.field))?;
                if !same_value(expected, &actual) {
                    return Err(format!(
                        "{node}.{}: expected {expected:?} but the regulation holds {actual:?} — \
                         the base moved under this patch. Re-export and redo the craft.",
                        edit.field
                    ));
                }
            }
            set_value(&mut row, &edit.field, &edit.value)
                .map_err(|e| format!("{node}.{}: {e}", edit.field))?;
        }

        writer
            .patch_row(table.entry_suffix(), def, &row)
            .map_err(|e| format!("{node}: {e}"))?;
        eprintln!("  {node}: {} field(s) edited", edits.len());
    }
    Ok(())
}

fn verify(out: &Path, patch: &CraftPatch, env: &Env) -> Result<(), String> {
    let regulation =
        Regulation::open(out).map_err(|e| format!("re-opening {}: {e}", out.display()))?;
    for row_spec in &patch.rows {
        let target_id = row_spec.id.unwrap_or(patch.target_id);
        let raw = regulation
            .param_table(&row_spec.table)
            .map_err(|e| format!("verify: {e}"))?;
        let row_bytes = raw
            .rows
            .iter()
            .find(|r| r.id as i64 == target_id)
            .ok_or_else(|| {
                format!(
                    "verify: {}'s row {target_id} vanished from the written file",
                    row_spec.table
                )
            })?;
        let def = env
            .defs
            .by_param_type(&raw.param_type)
            .map_err(|e| e.to_string())?;
        let decoded = souls_format::paramdef::decode_row(def, target_id, &row_bytes.bytes)
            .map_err(|e| {
                format!(
                    "verify: {}'s row {target_id} will not decode after write: {e}",
                    row_spec.table
                )
            })?;
        for (field, expected) in &row_spec.overrides {
            let actual = doc_value_of(&decoded, field)
                .ok_or_else(|| format!("verify: {}.{field} missing", row_spec.table))?;
            if !same_value(expected, &actual) {
                return Err(format!(
                    "verify: {}.{field} is {actual:?} in the written file, expected {expected:?}",
                    row_spec.table
                ));
            }
        }
    }
    eprintln!(
        "verified:   all {} row(s) present with their overrides applied",
        patch.rows.len()
    );
    Ok(())
}

fn dispatch(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
