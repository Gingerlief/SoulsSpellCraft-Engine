// Docs: docs/xtask/msg.md

use std::path::{ Path, PathBuf };
use std::process::ExitCode;

use souls_format::locate::{ generated_item_text_path, locate_item_msgbnd, locate_oodle_dll };

pub fn run(mut args: impl Iterator<Item = String>) -> ExitCode {
    match args.next().as_deref() {
        Some("dump") => {
            let rest: Vec<String> = args.collect();
            dispatch(dump(&rest))
        }
        Some("write") => {
            let rest: Vec<String> = args.collect();
            dispatch(write(&rest))
        }
        Some(other) => {
            eprintln!("unknown msg subcommand: {other}");
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
    eprintln!("usage: xtask msg <dump|write> [args]");
    eprintln!();
    eprintln!("  dump [--binder <msgbnd.dcx>] [--out <file>]");
    eprintln!("        extract GoodsName/GoodsInfo/GoodsCaption from the item msgbnd into JSON");
    eprintln!("        (default binder: located item_dlc02.msgbnd.dcx from the game install)");
    eprintln!("        (default out: data/reference/generated/item-text/item-text.json)");
    eprintln!();
    eprintln!("  write --out <msgbnd.dcx> --set <pool>:<id>=<text> [--set ...] [--binder <src>]");
    eprintln!("        write an item msgbnd carrying those edits");
    eprintln!("        (source defaults to the working copy's binder; --out may be that same");
    eprintln!("         file, which edits it in place)");
    eprintln!("        e.g. --set GoodsName:4000=\"Glintstone Pebble (test)\"");
}

/// Loads the game's Oodle DLL, or explains where to find it.
///
/// `announce` prints the resolved path — wanted by `dump`, which echoes every located
/// input, and noise inside a craft where this is one step of many.
pub fn load_oodle(announce: bool) -> Result<souls_format::oodle::Oodle, String> {
    let dll = locate_oodle_dll().ok_or(
        "oo2core_6_win64.dll not found in any known game install (set SSC_OODLE_DLL_PATH). \
         It ships with the game and cannot be vendored."
    )?;
    if announce {
        eprintln!("oodle:      {}", dll.display());
    }
    souls_format::oodle::Oodle::load(&dll).map_err(|e| e.to_string())
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

fn dump(args: &[String]) -> Result<(), String> {
    let mut binder: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--binder" => {
                i += 1;
                binder = Some(PathBuf::from(args.get(i).ok_or("--binder needs a file path")?));
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a file path")?));
            }
            other => {
                return Err(format!("unexpected argument: {other}"));
            }
        }
        i += 1;
    }

    let binder = match binder {
        Some(b) => b,
        None =>
            locate_item_msgbnd().ok_or(
                "no item msgbnd found (set SSC_ITEM_MSGBND_PATH, or pass --binder)"
            )?,
    };
    warn_if_base_only(&binder);

    let out = out.unwrap_or_else(generated_item_text_path);
    eprintln!("binder:     {}", binder.display());
    eprintln!("out:        {}", out.display());

    let oodle = load_oodle(true)?;

    souls_format::ItemText
        ::from_msgbnd(&binder, &oodle)
        .map_err(|e| format!("reading {}: {e}", binder.display()))?
        .write_json(&out)
        .map_err(|e| format!("writing {}: {e}", out.display()))?;

    // Read it straight back through the real consumer, so a broken dump fails here rather
    // than silently producing an all-None index later. `is_empty()` catches the case
    // `is_loaded()` alone misses: a dump with all three pools present but empty parses
    // fine (loaded = true) yet is exactly as useless as a missing file, and would
    // otherwise silently overwrite a good item-text.json with one no lookup can ever hit.
    let text = souls_format::ItemText::open(&out);
    if !text.is_loaded() {
        return Err(format!("wrote {} but it did not parse back", out.display()));
    }
    if text.is_empty() {
        return Err(
            format!(
                "wrote {} but every pool came back empty — the dump parsed but is useless; \
             check the binder passed to dump-item-text",
                out.display()
            )
        );
    }
    eprintln!(
        "verified:   GoodsName={} GoodsInfo={} GoodsCaption={}",
        text.len(souls_format::item_text::GOODS_NAME),
        text.len(souls_format::item_text::GOODS_INFO),
        text.len(souls_format::item_text::GOODS_CAPTION)
    );
    Ok(())
}

fn write(args: &[String]) -> Result<(), String> {
    let mut binder: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut edits: Vec<(String, i64, String)> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--binder" => {
                i += 1;
                binder = Some(PathBuf::from(args.get(i).ok_or("--binder needs a file path")?));
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a file path")?));
            }
            "--set" => {
                i += 1;
                let spec = args.get(i).ok_or("--set needs <pool>:<id>=<text>")?;
                let (lhs, text) = spec
                    .split_once('=')
                    .ok_or_else(|| format!("--set {spec}: expected <pool>:<id>=<text>"))?;
                let (pool, id) = lhs
                    .split_once(':')
                    .ok_or_else(|| format!("--set {spec}: expected <pool>:<id>=<text>"))?;
                let id: i64 = id
                    .trim()
                    .parse()
                    .map_err(|_| format!("--set {spec}: '{id}' is not an id"))?;
                edits.push((pool.trim().to_string(), id, text.to_string()));
            }
            other => {
                return Err(format!("unexpected argument: {other}"));
            }
        }
        i += 1;
    }

    let out = out.ok_or("--out is required: this command writes a binder")?;
    if edits.is_empty() {
        return Err("no --set given; refusing to write an unchanged binder".into());
    }

    let src = match binder {
        Some(b) => b,
        None =>
            locate_item_msgbnd().ok_or(
                "no item msgbnd found (set SSC_ITEM_MSGBND_PATH, or pass --binder)"
            )?,
    };
    warn_if_base_only(&src);
    // Editing the working copy in place is the normal case; only the game's own files are
    // off limits. `write_msgbnd` reads the whole binder before writing, so src == out is safe.
    if let Some(game) = souls_format::locate::game_dir() {
        if out.starts_with(&game) {
            return Err(format!("refusing to write inside the game install ({})", game.display()));
        }
    }

    let borrowed: Vec<(&str, i64, &str)> = edits
        .iter()
        .map(|(pool, id, text)| (pool.as_str(), *id, text.as_str()))
        .collect();

    let oodle = load_oodle(false)?;

    eprintln!("binder:     {}", src.display());
    eprintln!("out:        {}", out.display());
    for (pool, id, text) in &borrowed {
        eprintln!("  SET {pool}[{id}] = {}", text.lines().next().unwrap_or(""));
    }

    souls_format::item_text
        ::write_msgbnd(&src, &borrowed, &out, &oodle)
        .map_err(|e| format!("writing item text: {e}"))?;

    let bytes = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    eprintln!("verified:   wrote and read back {} ({bytes} bytes)", out.display());
    Ok(())
}

pub fn write_text_for_patch(
    edits: &[(&'static str, i64, &str)],
    out_binder: &Path
) -> Result<(), String> {
    if edits.is_empty() {
        return Ok(());
    }

    let src = locate_item_msgbnd().ok_or(
        "no item msgbnd in the working copy — run `xtask init` to seed it from the game install"
    )?;
    warn_if_base_only(&src);

    // Source and output are normally the same file: the working copy is read, edited, and
    // written back, so names from earlier crafts survive. `write_msgbnd` reads the whole
    // binder before writing a byte, which is what makes editing in place safe.
    //
    // The one thing to refuse is writing into the game install itself. `xtask init` only ever
    // copies out of it, and nothing else should touch it.
    if let Some(game) = souls_format::locate::game_dir() {
        if out_binder.starts_with(&game) {
            return Err(
                format!("refusing to write inside the game install ({})", game.display())
            );
        }
    }

    let oodle = load_oodle(false)?;

    eprintln!("text src:   {}", src.display());
    eprintln!("text out:   {}", out_binder.display());
    for (pool, id, text) in edits {
        eprintln!("  SET {pool}[{id}] = {}", text.lines().next().unwrap_or(""));
    }

    // `write_msgbnd` reads its own output back and re-checks every edit before returning,
    // so reaching Ok here means the binder on disk really names the spell.
    souls_format::item_text
        ::write_msgbnd(&src, edits, out_binder, &oodle)
        .map_err(|e| format!("writing item text: {e}"))
}

pub fn patched_binder_path_for(regulation_out: &Path) -> PathBuf {
    let dir = regulation_out.parent().unwrap_or_else(|| Path::new(""));
    dir.join("msg/engus/item_dlc02.msgbnd.dcx")
}

fn warn_if_base_only(binder: &Path) {
    let leaf = binder
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !leaf.contains("dlc") {
        eprintln!(
            "WARNING: {leaf} is the base binder — this dump will be missing the DLC's ~508 \
             item ids. item_dlc02.msgbnd.dcx is the complete one."
        );
    }
}
