// Docs: docs/xtask/sfx.md

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use souls_format::locate::{locate_sfx_dirs, patch_dir};
use souls_format::regulation::BinderEntry;

pub fn run(mut args: impl Iterator<Item = String>) -> ExitCode {
    let sub = args.next();
    let rest: Vec<String> = args.collect();
    match sub.as_deref() {
        Some("unpack") => dispatch(unpack(&rest)),
        Some("pack") => dispatch(pack(&rest)),
        Some(other) => {
            eprintln!("unknown sfx subcommand: {other}");
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
    eprintln!("usage: xtask sfx <unpack|pack> [args]");
    eprintln!();
    eprintln!("  unpack [--binder <ffxbnd.dcx>] [--out <dir>] [--force]");
    eprintln!("        explode an SFX binder into loose files the editor can read");
    eprintln!("  pack [--dir <unpacked>] [--out <ffxbnd.dcx>] [--level <0-9>]");
    eprintln!("        rebuild the binder from the files on disk");
    eprintln!("        (--level: Oodle effort, default 4/Normal; 6 is what the game ships)");
    eprintln!();
    eprintln!("  Both default to the working copy's sfxbnd_commoneffects. This replaces");
    eprintln!("  WitchyBND for these binders — same layout, no external tool.");
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

const DEFAULT_BINDER: &str = "sfx/sfxbnd_commoneffects.ffxbnd.dcx";

/// Oodle compression level for repacking.
///
/// FromSoft ships these binders at Optimal2 (6), which is an encoder tuned for a one-time
/// build, not for a 357 MB rebuild every time you nudge a colour. The game decompresses any
/// valid Kraken stream, so the level is purely a size/time trade on our side — and
/// `seekChunkReset`, the setting the game actually cares about, is applied at every level.
const DEFAULT_LEVEL: i32 = 4; // Normal

/// The prefix every entry name carries, from WitchyBND's own `<root>` field.
const ROOT: &str = r"N:\GR\data\INTERROOT_win64\sfx\";

/// Entry groups in binder order, with the base each group's ids count up from.
///
/// Measured from the shipped binder: effect 0..7137, tex 100000.., model 200000..,
/// hkx 300000.., ResourceList 400000.. — grouped in this order, id = base + index.
const TYPES: &[(&str, i32)] = &[
    ("effect", 0),
    ("tex", 100_000),
    ("model", 200_000),
    ("hkx", 300_000),
    ("ResourceList", 400_000),
];

/// `sfxbnd_commoneffects.ffxbnd.dcx` -> `sfxbnd_commoneffects-ffxbnd-dcx-wffxbnd`
///
/// WitchyBND's naming, kept deliberately: `locate_sfx_dirs` already derives the effect
/// directory from it, and matching means an existing unpack stays readable either way.
fn unpacked_dir_for(binder: &Path) -> PathBuf {
    let name = binder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .replace(".ffxbnd.dcx", "-ffxbnd-dcx-wffxbnd");
    binder.with_file_name(name)
}

fn binder_for(unpacked: &Path) -> PathBuf {
    let name = unpacked
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .replace("-ffxbnd-dcx-wffxbnd", ".ffxbnd.dcx");
    unpacked.with_file_name(name)
}

/// The path an entry occupies inside the unpacked directory.
///
/// Entry names are full game paths — `N:\GR\data\INTERROOT_win64\sfx\effect\f000000001.fxr`
/// — and everything after the `sfx` component is the layout WitchyBND produces
/// (`effect/`, `hkx/`, `model/`, `tex/`, `ResourceList/`).
fn relative_path(entry: &BinderEntry) -> PathBuf {
    let normalised = entry.name.replace('\\', "/");
    let tail = normalised
        .rsplit_once("/sfx/")
        .map(|(_, rest)| rest)
        .unwrap_or_else(|| entry.leaf_name());
    tail.split('/').collect()
}

fn unpack(args: &[String]) -> Result<(), String> {
    let mut binder: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut force = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--binder" => {
                i += 1;
                binder = Some(PathBuf::from(args.get(i).ok_or("--binder needs a path")?));
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a directory")?));
            }
            "--force" => force = true,
            other => {
                usage();
                return Err(format!("unexpected argument: {other}"));
            }
        }
        i += 1;
    }

    let binder = binder.unwrap_or_else(|| patch_dir().join(DEFAULT_BINDER));
    if !binder.is_file() {
        return Err(format!(
            "{} not found — run `xtask init` to seed the working copy",
            binder.display()
        ));
    }
    let out = out.unwrap_or_else(|| unpacked_dir_for(&binder));

    eprintln!("binder:     {}", binder.display());
    eprintln!("out:        {}", out.display());

    let oodle = crate::msg::load_oodle(true)?;
    let raw = std::fs::read(&binder).map_err(|e| format!("reading {}: {e}", binder.display()))?;
    let payload = souls_format::dcx::unwrap_krak(&raw, &oodle)
        .map_err(|e| format!("unwrapping {}: {e}", binder.display()))?;
    let entries = souls_format::regulation::parse_bnd4(&payload)
        .map_err(|e| format!("parsing {}: {e}", binder.display()))?;

    let mut written = 0usize;
    let mut kept = 0usize;
    for entry in &entries {
        let dst = out.join(relative_path(entry));
        if dst.is_file() && !force {
            kept += 1;
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::write(&dst, &entry.bytes)
            .map_err(|e| format!("writing {}: {e}", dst.display()))?;
        written += 1;
    }

    eprintln!("unpacked {written} file(s) from {} entries", entries.len());
    if kept > 0 {
        eprintln!("kept {kept} already on disk (--force to overwrite)");
    }
    Ok(())
}

/// `fxr repack`'s entry point: pack the default binder and report it as JSON.
///
/// The browser bridge parses this off stdout, so the shape is a contract — see
/// `plugins/engine.ts`, `/api/fxr/repack`.
pub fn repack_default() -> Result<String, String> {
    let dir = locate_sfx_dirs()
        .first()
        .and_then(|effect| effect.parent().map(Path::to_path_buf))
        .ok_or("no unpacked SFX directory found — run `xtask sfx unpack` first")?;
    let binder = binder_for(&dir);
    let before = std::fs::metadata(&binder).ok().map(|m| m.len());

    pack_into(&dir, &binder, DEFAULT_LEVEL)?;

    let after = std::fs::metadata(&binder)
        .map(|m| m.len())
        .map_err(|e| format!("packing produced no {}: {e}", binder.display()))?;
    let pristine = binder.with_extension("dcx.orig");
    Ok(serde_json::json!({
        "binder": binder.display().to_string(),
        "bytes": after,
        "previousBytes": before,
        "changed": before != Some(after),
        "snapshot": pristine.exists().then(|| pristine.display().to_string()),
    })
    .to_string())
}

fn pack(args: &[String]) -> Result<(), String> {
    let mut dir: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut level = DEFAULT_LEVEL;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" => {
                i += 1;
                dir = Some(PathBuf::from(args.get(i).ok_or("--dir needs a directory")?));
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a path")?));
            }
            "--level" => {
                i += 1;
                level = args
                    .get(i)
                    .ok_or("--level needs a number")?
                    .parse()
                    .map_err(|_| "--level takes an Oodle compression level (0-9)")?;
            }
            other => {
                usage();
                return Err(format!("unexpected argument: {other}"));
            }
        }
        i += 1;
    }

    // The base binder only: new effects are written there, never to the DLC sibling, because
    // `SfxDirectory` resolves base-first and a DLC-side id would be shadowed.
    let dir = match dir {
        Some(d) => d,
        None => locate_sfx_dirs()
            .first()
            .and_then(|effect| effect.parent().map(Path::to_path_buf))
            .ok_or("no unpacked SFX directory found — run `xtask sfx unpack` first")?,
    };
    let out = out.unwrap_or_else(|| binder_for(&dir));
    pack_into(&dir, &out, level)
}

/// Rebuilds `out` from the files in `dir`, using `dir`'s own binder as the header source.
fn pack_into(dir: &Path, out: &Path, level: i32) -> Result<(), String> {
    let source = binder_for(dir);
    if !source.is_file() {
        return Err(format!("no binder at {} to rebuild from", source.display()));
    }

    eprintln!("source:     {}", source.display());
    eprintln!("dir:        {}", dir.display());
    eprintln!("out:        {}", out.display());

    let oodle = crate::msg::load_oodle(true)?;
    let raw = std::fs::read(&source).map_err(|e| format!("reading {}: {e}", source.display()))?;
    let payload = souls_format::dcx::unwrap_krak(&raw, &oodle)
        .map_err(|e| format!("unwrapping {}: {e}", source.display()))?;
    let (header, entries) = souls_format::regulation::parse_bnd4_full(&payload)
        .map_err(|e| format!("parsing {}: {e}", source.display()))?;

    // Rebuilt from the directory, not overlaid onto the source's entry list: a new effect
    // written by the editor has no entry to overlay onto, and dropping it silently is exactly
    // the bug that would make a crafted spell invisible in game.
    //
    // WitchyBND regenerates the same way — its manifest records only the header and these
    // directory names, no per-file ids — and the id scheme is recoverable from any binder:
    // entries are grouped in TYPES order, and id is the group's base plus its index.
    let flags_for = |kind: &str| -> u8 {
        entries
            .iter()
            .find(|e| relative_path(e).starts_with(kind))
            .map(|e| e.flags)
            .unwrap_or(0x02)
    };

    let mut rebuilt_entries: Vec<BinderEntry> = Vec::with_capacity(entries.len());
    let mut added = 0usize;
    let known: std::collections::HashSet<PathBuf> =
        entries.iter().map(relative_path).collect();

    for (kind, base) in TYPES {
        let sub = dir.join(kind);
        let Ok(read) = std::fs::read_dir(&sub) else {
            continue;
        };
        let mut names: Vec<String> = read
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();

        let flags = flags_for(kind);
        for (i, name) in names.iter().enumerate() {
            let rel = PathBuf::from(kind).join(name);
            if !known.contains(&rel) {
                added += 1;
            }
            let bytes = std::fs::read(sub.join(name))
                .map_err(|e| format!("reading {}/{name}: {e}", sub.display()))?;
            rebuilt_entries.push(BinderEntry {
                id: base + i as i32,
                name: format!("{ROOT}{kind}\\{name}"),
                bytes,
                data_offset: 0, // recomputed by the writer
                flags,
            });
        }
    }

    if rebuilt_entries.is_empty() {
        return Err(format!("{} holds no packable files", dir.display()));
    }
    let entries = rebuilt_entries;

    let rebuilt = souls_format::bnd4::write_bnd4(&header, &entries)
        .map_err(|e| format!("rebuilding the binder: {e}"))?;
    let wrapped = souls_format::dcx::wrap_krak(&rebuilt, &oodle, level)
        .map_err(|e| format!("compressing the binder: {e}"))?;

    // Keep one pristine copy the first time this binder is rebuilt, so there is always a way
    // back without re-running `xtask init`.
    let pristine = out.with_extension("dcx.orig");
    if out.is_file() && !pristine.exists() {
        std::fs::copy(&out, &pristine)
            .map_err(|e| format!("backing up {} -> {}: {e}", out.display(), pristine.display()))?;
        eprintln!("backup:     {}", pristine.display());
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(&out, &wrapped).map_err(|e| format!("writing {}: {e}", out.display()))?;

    eprintln!(
        "packed {} entries ({added} new), {:.1} MB",
        entries.len(),
        wrapped.len() as f64 / (1u64 << 20) as f64
    );
    Ok(())
}
