// Docs: docs/xtask/spell.md

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use souls_format::locate::locate_regulation_bin;
use souls_format::paramdef::{ParamRow, ParamValue};
use souls_format::write::RegulationWriter;
use souls_format::{Regulation, RowResult};
use spellcraft_engine::cache::hash_file;
use spellcraft_engine::export::DocValue;
use spellcraft_engine::export::{
    apply_node_names, build_document, load_field_meta, DocumentSource, SpellDocument, SpellIndex,
    SpellIndexEntry, EXPORT_SCHEMA_VERSION,
};
use spellcraft_engine::graph::model::NodeKey;
use spellcraft_engine::graph::Walker;
use spellcraft_engine::patch::{FieldEdit, SpellPatch};
use spellcraft_engine::source::ParamSource;

use crate::env::{ids_from_arg, reference_dir, repo_root, walk_options, Env};

fn default_out_dir() -> PathBuf {
    // `souls_format::locate::export_dir` so SSC_EXPORT_DIR drives this and the browser
    // bridge from one value. They previously agreed only by repeating the same literal.
    souls_format::locate::export_dir()
}

pub fn run(mut args: impl Iterator<Item = String>) -> ExitCode {
    match args.next().as_deref() {
        Some("export") => {
            let rest: Vec<String> = args.collect();
            dispatch(export(&rest))
        }
        Some("types") => {
            let rest: Vec<String> = args.collect();
            dispatch(types(&rest))
        }
        Some("apply") => {
            let rest: Vec<String> = args.collect();
            dispatch(apply(&rest))
        }
        Some("repack") => {
            let rest: Vec<String> = args.collect();
            dispatch(repack(&rest))
        }
        Some(other) => {
            eprintln!("unknown spell subcommand: {other}");
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
    eprintln!("usage: xtask spell <export|types|apply|repack> [args]");
    eprintln!();
    eprintln!("  export [<magic_id>|--all] [--out <dir>]");
    eprintln!("        write self-contained spell documents for an external editor,");
    eprintln!("        plus an index.json cataloguing the directory");
    eprintln!("        (default out: data/reference/generated/spell-export/)");
    eprintln!("  types [--out <file>] [--check]");
    eprintln!("        write the TypeScript declarations for those documents");
    eprintln!("        (default out: generated/ts/spell-document.ts)");
    eprintln!("        --check: fail if the editor's copy is stale instead of writing");
    eprintln!("  apply <patch.json> [--out <regulation.bin>] [--force]");
    eprintln!("        apply a patch to the located regulation, writing a NEW file");
    eprintln!("        (default out: launch/patched/regulation.bin — the ME3 package,");
    eprintln!("         launched with: me3 launch --auto-detect -p launch/launch_er_patched.me3)");
    eprintln!("        never modifies the source regulation; refuses if the patch was");
    eprintln!("        authored against a different one");
    eprintln!("  repack [--out <regulation.bin>] [--level <1-22>] [--force]");
    eprintln!("        re-encode the regulation with NO edits, to test the container");
    eprintln!("        encoding on its own when the game rejects a written file");
}

fn repack(args: &[String]) -> Result<(), String> {
    let mut out: Option<PathBuf> = None;
    let mut force = false;
    let mut level: Option<i32> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a file path")?));
            }
            "--level" => {
                i += 1;
                level = Some(
                    args.get(i)
                        .ok_or("--level needs a zstd level (1-22)")?
                        .parse()
                        .map_err(|_| "--level must be a number")?,
                );
            }
            "--force" => force = true,
            other => return Err(format!("unexpected argument: {other}")),
        }
        i += 1;
    }
    let out = out.unwrap_or_else(|| repo_root().join("launch/patched/regulation.bin"));

    let source_path = locate_regulation_bin().ok_or("no regulation.bin found")?;
    if out == source_path {
        return Err("refusing to write over the source regulation".into());
    }
    if out.exists() && !force {
        return Err(format!(
            "{} already exists — pass --force to replace it",
            out.display()
        ));
    }
    eprintln!("source:     {}", source_path.display());
    eprintln!(
        "sha256:     {}",
        hash_file(&source_path).map_err(|e| e.to_string())?
    );

    let raw = std::fs::read(&source_path).map_err(|e| format!("reading regulation: {e}"))?;
    let mut writer =
        RegulationWriter::open(&raw).map_err(|e| format!("opening regulation: {e}"))?;
    if let Some(l) = level {
        writer = writer.with_zstd_level(l);
    }
    eprintln!(
        "zstd level: {}{}",
        writer.zstd_level(),
        if level.is_some() {
            " (overridden by --level)"
        } else if std::env::var(souls_format::write::ZSTD_LEVEL_ENV).is_ok() {
            " (from SSC_ZSTD_LEVEL)"
        } else {
            " (from the source header)"
        }
    );

    // Assert the premise rather than trusting it: if the payload already differed, this
    // command would not be testing what it claims to test.
    let original_payload = souls_format::regulation::decrypt_and_unwrap_regulation(&raw)
        .map_err(|e| format!("decoding source: {e}"))?;
    if writer.payload() != original_payload.as_slice() {
        return Err("payload changed on open — repack cannot isolate the container".into());
    }

    let bytes = writer.finish().map_err(|e| format!("re-encoding: {e}"))?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(&out, &bytes).map_err(|e| format!("writing {}: {e}", out.display()))?;

    let round_trip = souls_format::regulation::decrypt_and_unwrap_regulation(&bytes)
        .map_err(|e| format!("re-reading our own output: {e}"))?;
    if round_trip != original_payload {
        return Err("our own output does not read back identically — stop here".into());
    }

    eprintln!(
        "payload:    {} bytes, byte-identical to the source",
        round_trip.len()
    );
    eprintln!(
        "wrote {} ({:.1} MB vs source {:.1} MB)",
        out.display(),
        bytes.len() as f64 / 1_048_576.0,
        raw.len() as f64 / 1_048_576.0
    );
    eprintln!();
    eprintln!("This file contains ZERO edits — only our container encoding differs from the");
    eprintln!("original. Launch it:");
    eprintln!("    me3 launch --auto-detect -p launch/launch_er_patched.me3");
    eprintln!("  crashes  -> the fault is in souls-format::write, not in any patch");
    eprintln!("  loads    -> the container is fine; the problem is what a patch writes");
    Ok(())
}

fn apply(args: &[String]) -> Result<(), String> {
    let mut patch_path: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut force = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a file path")?));
            }
            "--force" => force = true,
            other => patch_path = Some(PathBuf::from(other)),
        }
        i += 1;
    }
    let patch_path = patch_path.ok_or("usage: xtask spell apply <patch.json> [--out <file>]")?;
    // Defaults into the ME3 package folder, so applying a patch and launching the game are
    // one step apart rather than three. `launch/launch_er_patched.me3` overlays this
    // directory onto the game's data root.
    let out = out.unwrap_or_else(|| repo_root().join("launch/patched/regulation.bin"));

    let text = std::fs::read_to_string(&patch_path)
        .map_err(|e| format!("reading {}: {e}", patch_path.display()))?;
    let patch: SpellPatch = serde_json::from_str(&text)
        .map_err(|e| format!("parsing {}: {e}", patch_path.display()))?;

    let env = Env::prepare()?;
    patch
        .check_applicable(&env.regulation_sha256)
        .map_err(|e| e.to_string())?;
    eprintln!(
        "patch:      {} ({} edit(s))",
        patch_path.display(),
        patch.edits.len()
    );
    if let Some(note) = &patch.note {
        eprintln!("note:       {note}");
    }
    if patch.edits.is_empty() {
        return Err("patch contains no edits".into());
    }

    let source_path = locate_regulation_bin().ok_or("no regulation.bin found")?;
    if out == source_path {
        return Err("refusing to write over the source regulation".into());
    }
    if out.exists() && !force {
        return Err(format!(
            "{} already exists — pass --force to replace it",
            out.display()
        ));
    }

    let raw = std::fs::read(&source_path).map_err(|e| format!("reading regulation: {e}"))?;
    let mut writer =
        RegulationWriter::open(&raw).map_err(|e| format!("opening regulation: {e}"))?;

    // Group edits by row: a row is decoded once, all its fields set, then encoded once.
    // Applying them one at a time would re-decode the row per field and, worse, would let a
    // later edit be written against a row state an earlier edit had already changed.
    let mut by_node: BTreeMap<String, Vec<&FieldEdit>> = BTreeMap::new();
    for edit in &patch.edits {
        by_node.entry(edit.node.clone()).or_default().push(edit);
    }

    let mut applied = 0usize;
    for (node, edits) in &by_node {
        let key: NodeKey = node.parse().map_err(|e: String| e)?;
        let table = key
            .kind
            .table()
            .ok_or_else(|| format!("{node} is not a param row and cannot be patched"))?;
        let param_type = table.param_type();
        let def = env
            .defs
            .by_param_type(param_type)
            .map_err(|e| format!("{node}: no paramdef for {param_type}: {e}"))?;

        let mut row = match env.source.row(table, key.id) {
            RowResult::Found(r) => (*r).clone(),
            RowResult::Missing => return Err(format!("{node}: no such row in the regulation")),
            RowResult::Undecodable { error } => {
                return Err(format!(
                    "{node}: row will not decode, so it cannot be rewritten: {error}"
                ))
            }
        };

        for edit in edits {
            let before = describe(&row, &edit.field);
            if let Some(expected) = &edit.expected {
                let actual = doc_value_of(&row, &edit.field)
                    .ok_or_else(|| format!("{node}.{}: no such field", edit.field))?;
                if !same_value(expected, &actual) {
                    return Err(format!(
                        "{node}.{}: expected {expected:?} but the regulation holds {actual:?} — \
                         the base moved under this patch. Re-export and redo the edit.",
                        edit.field
                    ));
                }
            }
            set_value(&mut row, &edit.field, &edit.value)
                .map_err(|e| format!("{node}.{}: {e}", edit.field))?;
            eprintln!(
                "  {node}.{}: {before} -> {}",
                edit.field,
                describe(&row, &edit.field)
            );
            applied += 1;
        }

        writer
            .patch_row(table.entry_suffix(), def, &row)
            .map_err(|e| format!("{node}: {e}"))?;
    }

    let bytes = writer
        .finish()
        .map_err(|e| format!("re-encoding regulation: {e}"))?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(&out, &bytes).map_err(|e| format!("writing {}: {e}", out.display()))?;

    // Read the written file back and confirm the edits are in it. Writing without checking
    // would mean the first evidence of a bad write is the game failing to start.
    verify(&out, &patch, &env)?;

    eprintln!();
    eprintln!("applied {applied} edit(s) to {} row(s)", by_node.len());
    eprintln!(
        "wrote {} ({:.1} MB)",
        out.display(),
        bytes.len() as f64 / 1_048_576.0
    );
    eprintln!("source regulation untouched: {}", source_path.display());
    eprintln!();
    eprintln!("Keep a backup of the original — it is the only known-good base. The write");
    eprintln!("mechanism is validated in-game, but whether these values are sane is not");
    eprintln!("something this command can check.");
    Ok(())
}

fn verify(out: &Path, patch: &SpellPatch, env: &Env) -> Result<(), String> {
    let regulation =
        Regulation::open(out).map_err(|e| format!("re-opening {}: {e}", out.display()))?;
    for edit in &patch.edits {
        let key: NodeKey = edit.node.parse().map_err(|e: String| e)?;
        let table = key.kind.table().expect("checked during apply");
        let raw = regulation
            .param_table(table.entry_suffix())
            .map_err(|e| format!("verify: {e}"))?;
        let row_bytes = raw
            .rows
            .iter()
            .find(|r| r.id as i64 == key.id)
            .ok_or_else(|| format!("verify: {} vanished from the written file", edit.node))?;
        let def = env
            .defs
            .by_param_type(table.param_type())
            .map_err(|e| e.to_string())?;
        let decoded = souls_format::paramdef::decode_row(def, key.id, &row_bytes.bytes)
            .map_err(|e| format!("verify: {} will not decode after write: {e}", edit.node))?;
        let actual = doc_value_of(&decoded, &edit.field)
            .ok_or_else(|| format!("verify: {}.{} missing", edit.node, edit.field))?;
        if !same_value(&edit.value, &actual) {
            return Err(format!(
                "verify: {}.{} is {actual:?} in the written file, expected {:?}",
                edit.node, edit.field, edit.value
            ));
        }
    }
    eprintln!(
        "verified:   all {} edit(s) present in the written file",
        patch.edits.len()
    );
    Ok(())
}

fn describe(row: &ParamRow, field: &str) -> String {
    match row.fields.get(field) {
        Some(ParamValue::I64(v)) => v.to_string(),
        Some(ParamValue::F32(v)) => format!("{v}"),
        Some(ParamValue::F64(v)) => format!("{v}"),
        Some(ParamValue::Str(s)) => format!("{s:?}"),
        Some(ParamValue::Bytes(b)) => format!("{} bytes", b.len()),
        None => "<missing>".into(),
    }
}

pub(crate) fn doc_value_of(row: &ParamRow, field: &str) -> Option<DocValue> {
    Some(match row.fields.get(field)? {
        ParamValue::I64(v) => DocValue::Int(*v),
        ParamValue::F32(v) => DocValue::Float(*v),
        ParamValue::F64(v) => DocValue::Double(*v),
        ParamValue::Str(s) => DocValue::Str(s.clone()),
        ParamValue::Bytes(b) => {
            DocValue::Bytes(b.iter().map(|byte| format!("{byte:02x}")).collect())
        }
    })
}

pub(crate) fn same_value(a: &DocValue, b: &DocValue) -> bool {
    match (a, b) {
        (DocValue::Int(x), DocValue::Int(y)) => x == y,
        (DocValue::Float(x), DocValue::Float(y)) => x.to_bits() == y.to_bits(),
        (DocValue::Double(x), DocValue::Double(y)) => x.to_bits() == y.to_bits(),
        (DocValue::Str(x), DocValue::Str(y)) => x == y,
        (DocValue::Bytes(x), DocValue::Bytes(y)) => x.eq_ignore_ascii_case(y),
        _ => false,
    }
}

pub(crate) fn set_value(row: &mut ParamRow, field: &str, value: &DocValue) -> Result<(), String> {
    let slot = row
        .fields
        .get_mut(field)
        .ok_or_else(|| "no such field on this row".to_string())?;
    match (slot, value) {
        (ParamValue::I64(s), DocValue::Int(v)) => *s = *v,
        (ParamValue::F32(s), DocValue::Float(v)) => *s = *v,
        (ParamValue::F64(s), DocValue::Double(v)) => *s = *v,
        (ParamValue::Str(s), DocValue::Str(v)) => s.clone_from(v),
        (slot, v) => {
            return Err(format!(
                "field is {}, but the patch supplies {}",
                slot.variant_name(),
                match v {
                    DocValue::Int(_) => "an integer",
                    DocValue::Float(_) => "an f32",
                    DocValue::Double(_) => "an f64",
                    DocValue::Str(_) => "a string",
                    DocValue::Bytes(_) => "bytes",
                }
            ))
        }
    }
    Ok(())
}

fn ui_declarations_path() -> PathBuf {
    repo_root().join("../SoulsSpellCraft/src/spell-document.ts")
}

fn types(args: &[String]) -> Result<(), String> {
    let mut out: Option<PathBuf> = None;
    let mut check = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a file path")?));
            }
            "--check" => check = true,
            other => return Err(format!("unexpected argument: {other}")),
        }
        i += 1;
    }

    let ts = spellcraft_engine::export::typescript_declarations();

    if check {
        let target = out.unwrap_or_else(ui_declarations_path);
        let Ok(existing) = std::fs::read_to_string(&target) else {
            eprintln!("no declarations at {} — nothing to check", target.display());
            eprintln!("(clone SoulsSpellCraft alongside this repo, or pass --out)");
            return Ok(());
        };
        // Line endings are a checkout artifact, not drift.
        if existing.replace("\r\n", "\n") == ts.replace("\r\n", "\n") {
            eprintln!("up to date: {}", target.display());
            return Ok(());
        }
        return Err(format!(
            "{} is stale — regenerate it:\n    cargo run -p xtask -- spell types --out {}\n\
             \nThe editor's types no longer match this engine's export document. Its \
             fixture likely needs re-exporting too.",
            target.display(),
            target.display()
        ));
    }

    let target = out.unwrap_or_else(|| repo_root().join("generated/ts/spell-document.ts"));
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(&target, &ts).map_err(|e| format!("writing {}: {e}", target.display()))?;
    eprintln!("wrote {} ({} bytes)", target.display(), ts.len());
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

pub fn document_for(env: &Env, magic_id: i64) -> Result<SpellDocument, String> {
    let mut walker = Walker::new(&env.source).with_options(walk_options());
    if let Some(sfx) = &env.sfx {
        walker = walker.with_sfx(sfx);
    }
    if let Some(names) = &env.names {
        walker = walker.with_names(names);
    }

    let graph = walker
        .walk(magic_id)
        .map_err(|e| format!("walking spell {magic_id}: {e}"))?;

    let mut doc = build_document(
        &graph,
        &env.defs,
        load_field_meta(&reference_dir()),
        document_source(env),
    );
    if let Some(names) = &env.names {
        apply_node_names(&mut doc, &graph, names);
    }
    Ok(doc)
}

fn export(args: &[String]) -> Result<(), String> {
    let mut target: Option<&str> = None;
    let mut out_dir = default_out_dir();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out_dir = PathBuf::from(args.get(i).ok_or("--out needs a directory")?);
            }
            other => target = Some(other),
        }
        i += 1;
    }

    let env = Env::prepare()?;
    let ids = ids_from_arg(&env, target)?;

    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("creating {}: {e}", out_dir.display()))?;
    eprintln!("out dir:    {}", out_dir.display());
    eprintln!("exporting {} spell(s)...", ids.len());

    let (mut ok, mut failed) = (0usize, 0usize);
    let mut total_bytes = 0u64;
    let mut largest: Option<(i64, u64)> = None;
    let mut entries: Vec<SpellIndexEntry> = Vec::new();

    for id in &ids {
        let built = document_for(&env, *id).and_then(|doc| {
            let bytes = write_document(&out_dir, &doc)?;
            Ok((SpellIndexEntry::of(&doc, format!("{id}.json")), bytes))
        });
        match built {
            Ok((entry, bytes)) => {
                ok += 1;
                total_bytes += bytes;
                if largest.is_none_or(|(_, b)| bytes > b) {
                    largest = Some((*id, bytes));
                }
                entries.push(entry);
            }
            Err(e) => {
                failed += 1;
                eprintln!("  {id}: {e}");
            }
        }
    }

    eprintln!("exported {ok}, failed {failed}");
    if ok > 0 {
        eprintln!(
            "total {:.1} MB, mean {:.0} KB/spell",
            total_bytes as f64 / 1_048_576.0,
            total_bytes as f64 / ok as f64 / 1024.0
        );
    }
    if let Some((id, bytes)) = largest {
        eprintln!("largest: spell {id} at {:.0} KB", bytes as f64 / 1024.0);
    }

    // A directory of documents is only usable if something says what is in it. Merged
    // rather than overwritten so exporting one spell into an existing directory updates
    // that entry instead of reducing the index to a single row.
    if ok > 0 {
        let bytes = write_index(&out_dir, &env, entries)?;
        eprintln!(
            "index:      {} ({:.0} KB)",
            out_dir.join(INDEX_FILE).display(),
            bytes as f64 / 1024.0
        );
    }
    Ok(())
}

const INDEX_FILE: &str = "index.json";

fn document_source(env: &Env) -> DocumentSource {
    DocumentSource {
        regulation_sha256: Some(env.regulation_sha256.clone()),
        paramdex_fingerprint: Some(env.paramdex_fingerprint.clone()),
    }
}

fn write_index(out_dir: &Path, env: &Env, fresh: Vec<SpellIndexEntry>) -> Result<u64, String> {
    let source = document_source(env);
    let mut merged: BTreeMap<i64, SpellIndexEntry> = BTreeMap::new();

    if let Ok(text) = std::fs::read_to_string(out_dir.join(INDEX_FILE)) {
        if let Ok(existing) = serde_json::from_str::<SpellIndex>(&text) {
            let same_base = existing.schema_version == EXPORT_SCHEMA_VERSION
                && existing.source.regulation_sha256 == source.regulation_sha256;
            if same_base {
                for entry in existing.spells {
                    merged.insert(entry.magic_id, entry);
                }
            } else {
                eprintln!(
                    "index:      existing index is from another regulation or schema — replacing"
                );
            }
        }
    }
    for entry in fresh {
        merged.insert(entry.magic_id, entry);
    }

    let index = SpellIndex::new(source, merged.into_values().collect());
    let json = serde_json::to_vec_pretty(&index).map_err(|e| format!("serializing index: {e}"))?;
    let path = out_dir.join(INDEX_FILE);
    std::fs::write(&path, &json).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(json.len() as u64)
}

fn write_document(out_dir: &Path, doc: &SpellDocument) -> Result<u64, String> {
    let path = out_dir.join(format!("{}.json", doc.spell.magic_id));
    let json = serde_json::to_vec_pretty(doc).map_err(|e| format!("serializing: {e}"))?;
    std::fs::write(&path, &json).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(json.len() as u64)
}
