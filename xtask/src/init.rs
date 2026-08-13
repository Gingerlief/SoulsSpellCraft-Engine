// Docs: docs/xtask/init.md

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use souls_format::locate::{game_dir, patch_dir};

pub fn run(args: impl Iterator<Item = String>) -> ExitCode {
    match init(&args.collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!("usage: xtask init [--game <dir>] [--out <dir>] [--force]");
    eprintln!("        seed the working copy from a game install: regulation.bin, the item");
    eprintln!("        text binder, and the common SFX binders");
    eprintln!("        (default game: SSC_GAME_DIR, else a standard Steam install)");
    eprintln!("        (default out:  SSC_PATCH_DIR, else launch/patched/)");
    eprintln!("        existing files are kept unless --force is given");
}

/// What `init` seeds, as (path relative to the game install, whether it is required).
///
/// Only `regulation.bin` sits loose in an untouched install. `msg/` and `sfx/` live inside
/// `Data0-3.bdt` until UXM unpacks them, so their absence is a normal state to explain
/// rather than an error to fail on.
const SEED: &[(&str, bool)] = &[
    ("regulation.bin", true),
    ("msg/engus/item_dlc02.msgbnd.dcx", false),
    ("sfx/sfxbnd_commoneffects.ffxbnd.dcx", false),
    ("sfx/sfxbnd_commoneffects_dlc02.ffxbnd.dcx", false),
];

fn init(args: &[String]) -> Result<(), String> {
    let mut game: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut force = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--game" => {
                i += 1;
                game = Some(PathBuf::from(args.get(i).ok_or("--game needs a directory")?));
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a directory")?));
            }
            "--force" => force = true,
            "--help" | "-h" => {
                usage();
                return Ok(());
            }
            other => {
                usage();
                return Err(format!("unexpected argument: {other}"));
            }
        }
        i += 1;
    }

    let game = match game {
        Some(g) => g,
        None => game_dir().ok_or(
            "no Elden Ring install found. Set SSC_GAME_DIR (or gameDir in config.json) to the \
             folder holding regulation.bin, or pass --game.",
        )?,
    };
    if !game.join("regulation.bin").is_file() {
        return Err(format!(
            "{} has no regulation.bin — that is not an Elden Ring 'Game' folder",
            game.display()
        ));
    }

    let out = out.unwrap_or_else(patch_dir);
    eprintln!("game:       {}", game.display());
    eprintln!("out:        {}", out.display());

    let mut copied = 0;
    let mut missing = Vec::new();
    for (rel, required) in SEED {
        let src = game.join(rel);
        if !src.is_file() {
            if *required {
                return Err(format!("{} is missing from the install", src.display()));
            }
            missing.push(*rel);
            continue;
        }

        let dst = out.join(rel);
        if dst.is_file() && !force {
            eprintln!("  kept      {rel} (already present; --force to replace)");
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::copy(&src, &dst)
            .map_err(|e| format!("copying {} -> {}: {e}", src.display(), dst.display()))?;
        eprintln!("  copied    {rel} ({})", human(&dst));
        copied += 1;
    }

    if !missing.is_empty() {
        eprintln!();
        eprintln!("not in the install yet: {}", missing.join(", "));
        eprintln!(
            "  These live inside Data0-3.bdt until UXM Selective Unpack extracts them. Without\n  \
             msg/, spells fall back to community names; without sfx/, the effect panel cannot\n  \
             resolve files. Both are optional — the editor runs on regulation.bin alone."
        );
    }

    eprintln!();
    eprintln!("seeded {copied} file(s) into {}", out.display());
    if missing.len() < SEED.len() - 1 {
        eprintln!("the SFX binders still need unpacking with WitchyBND before the effect panel works");
    }
    Ok(())
}

fn human(p: &Path) -> String {
    match std::fs::metadata(p).map(|m| m.len()) {
        Ok(n) if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        Ok(n) if n >= 1 << 10 => format!("{:.1} KB", n as f64 / (1u64 << 10) as f64),
        Ok(n) => format!("{n} B"),
        Err(_) => "?".into(),
    }
}
