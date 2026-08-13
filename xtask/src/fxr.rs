// Docs: docs/xtask/fxr.md

use std::path::PathBuf;
use std::process::ExitCode;

use souls_format::locate::locate_sfx_dirs;
use souls_format::sfx_dir::fxr_file_name;

pub fn run(mut args: impl Iterator<Item = String>) -> ExitCode {
    match args.next().as_deref() {
        Some("where") => where_is(args),
        Some("repack") => repack(),
        Some(other) => {
            eprintln!("unknown fxr subcommand: {other}");
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
    eprintln!("usage: xtask fxr <subcommand>");
    eprintln!();
    eprintln!("  where <sfx-id>  print, as JSON, the file backing an id and the dirs searched");
    eprintln!("  repack          pack the effect directory back into its .ffxbnd.dcx");
}

fn repack() -> ExitCode {
    // Native since the BND4 writer landed: `sfx::pack` rebuilds the binder from the unpacked
    // directory and re-wraps it in DCX_KRAK. Verified byte-identical against the shipped
    // binder, so this is a drop-in for what WitchyBND did — minus the external tool.
    match crate::sfx::repack_default() {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn where_is(mut args: impl Iterator<Item = String>) -> ExitCode {
    let Some(raw) = args.next() else {
        usage();
        return ExitCode::FAILURE;
    };
    let Ok(id) = raw.trim().parse::<i32>() else {
        eprintln!("not an sfx id: {raw}");
        return ExitCode::FAILURE;
    };

    let dirs = locate_sfx_dirs();
    let file_name = fxr_file_name(id);
    // First hit wins, matching `SfxDirectory`'s own precedence — base binder before the DLC
    // sibling. Resolved to an absolute path because the point of this is to be pasted into a
    // file picker, and a path relative to the engine's working directory is not.
    let found: Option<PathBuf> = dirs
        .iter()
        .map(|d| d.join(&file_name))
        .find(|p| p.is_file());

    println!(
        "{}",
        serde_json::json!({
            "id": id,
            "fileName": file_name,
            "path": found.as_ref().map(|p| display_path(p)),
            "found": found.is_some(),
            "dirs": dirs.iter().map(|d| display_path(d)).collect::<Vec<_>>(),
            // Where a *new* effect belongs: the base binder, never the DLC sibling.
            // `SfxDirectory` resolves base-first, so an id written to the DLC copy would be
            // shadowed by any base file of the same id and silently never load. Policy rather
            // than a detail, which is why it is answered here instead of guessed by a caller.
            "writeDir": dirs.first().map(|d| display_path(d)),
        })
    );
    ExitCode::SUCCESS
}

fn display_path(p: &std::path::Path) -> String {
    let resolved = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = resolved.display().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}
