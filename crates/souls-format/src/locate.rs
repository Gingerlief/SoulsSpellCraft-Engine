// Docs: docs/souls-format/locate.md

use std::path::PathBuf;

// --- The three roots everything else is derived from. ---
//
// `config.json` in the consuming UI sets exactly these; every other path below falls out of
// them. Keeping the derivations here rather than in the caller means one definition of where
// a patched msgbnd lives, not one per repo.

/// The Elden Ring install (the `Game` folder holding `regulation.bin` and `oo2core_*.dll`).
/// Only ever read: `xtask init` copies out of it, and nothing writes back.
pub const GAME_DIR_ENV: &str = "SSC_GAME_DIR";
/// The working copy ME3 overlays. Everything the editor reads and writes lives here.
pub const PATCH_DIR_ENV: &str = "SSC_PATCH_DIR";
/// Where `xtask spell export` writes and the browser bridge reads.
pub const EXPORT_DIR_ENV: &str = "SSC_EXPORT_DIR";

// --- Escape hatches. Not set by config.json; used by tests and one-off invocations. ---
pub const REGULATION_BIN_ENV: &str = "SSC_REGULATION_BIN_PATH";
pub const PARAMDEX_DIR_ENV: &str = "SSC_PARAMDEX_DIR";
pub const SFX_DIRS_ENV: &str = "SSC_SFX_DIRS";
pub const ITEM_MSGBND_ENV: &str = "SSC_ITEM_MSGBND_PATH";
pub const DATA_DIR_ENV: &str = "SSC_DATA_DIR";
pub const OODLE_DLL_ENV: &str = "SSC_OODLE_DLL_PATH";

const DATA_ROOT_MARKER: &str = "data/paramdex";

/// Standard Steam install locations, tried when [`GAME_DIR_ENV`] is unset.
///
/// Only the two default library paths — a machine-specific location is what the env var is
/// for. A hardcoded personal path here is how a private path ends up in a public repo.
const GAME_DIR_FALLBACKS: &[&str] = &[
    r"C:\Games\Steam\steamapps\common\ELDEN RING\Game",
    r"C:\Program Files (x86)\Steam\steamapps\common\ELDEN RING\Game",
];

/// The unpacked SFX binder directories WitchyBND produces, by its naming convention.
const SFX_BINDERS: &[&str] = &[
    "sfxbnd_commoneffects-ffxbnd-dcx-wffxbnd",
    "sfxbnd_commoneffects_dlc02-ffxbnd-dcx-wffxbnd",
];

/// This repo's own root, found by its vendored paramdex rather than a compiled-in path, so a
/// binary copied elsewhere still finds the data shipped beside it.
pub fn data_root() -> PathBuf {
    if let Some(p) = from_env(DATA_DIR_ENV) {
        return p;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [dir, dir.parent().unwrap_or(dir)] {
                if candidate.join(DATA_ROOT_MARKER).is_dir() {
                    return candidate.to_path_buf();
                }
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn game_dir() -> Option<PathBuf> {
    if let Some(p) = from_env(GAME_DIR_ENV) {
        return Some(p);
    }
    GAME_DIR_FALLBACKS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("regulation.bin").is_file())
}

pub fn patch_dir() -> PathBuf {
    from_env(PATCH_DIR_ENV).unwrap_or_else(|| data_root().join("launch/patched"))
}

pub fn export_dir() -> PathBuf {
    from_env(EXPORT_DIR_ENV)
        .unwrap_or_else(|| data_root().join("data/reference/generated/spell-export"))
}

// --- Derived paths. ---

/// The regulation the editor reads *and* writes — the working copy, never the game's own.
pub fn locate_regulation_bin() -> Option<PathBuf> {
    if let Some(p) = from_env(REGULATION_BIN_ENV) {
        return Some(p);
    }
    let patched = patch_dir().join("regulation.bin");
    patched.is_file().then_some(patched)
}

/// The item text binder inside the working copy.
///
/// Targets `item_dlc02.msgbnd.dcx`: it is a strict superset (78 entries = base 26 +
/// `*_dlc01` 26 + `*_dlc02` 26) and holds all item goods text, so it is the only binder this
/// project reads or writes.
pub fn locate_item_msgbnd() -> Option<PathBuf> {
    if let Some(p) = from_env(ITEM_MSGBND_ENV) {
        return Some(p);
    }
    let dir = patch_dir().join("msg/engus");
    ["item_dlc02.msgbnd.dcx", "item.msgbnd.dcx"]
        .iter()
        .map(|leaf| dir.join(leaf))
        .find(|p| p.is_file())
}

/// The *pristine* regulation in the game install.
///
/// Tests that assert measured values want this, not [`locate_regulation_bin`]: the working
/// copy grows a row every time a craft allocates a slot, so a vanilla row count only holds
/// against the game's own file.
pub fn game_regulation_bin() -> Option<PathBuf> {
    let reg = game_dir()?.join("regulation.bin");
    reg.is_file().then_some(reg)
}

/// The *pristine* item binder in the game install. Used by `xtask init` to seed the working
/// copy, and by tests that need untouched game data.
pub fn game_item_msgbnd() -> Option<PathBuf> {
    let dir = game_dir()?.join("msg/engus");
    ["item_dlc02.msgbnd.dcx", "item.msgbnd.dcx"]
        .iter()
        .map(|leaf| dir.join(leaf))
        .find(|p| p.is_file())
}

/// Rad Game Tools' Oodle runtime, loaded from the install at run time by [`crate::oodle`].
/// Proprietary: never vendored, never committed.
pub fn locate_oodle_dll() -> Option<PathBuf> {
    if let Some(p) = from_env(OODLE_DLL_ENV) {
        return Some(p);
    }
    let dll = game_dir()?.join("oo2core_6_win64.dll");
    dll.is_file().then_some(dll)
}

/// Unpacked effect directories inside the working copy, base first then the DLC sibling —
/// an id present in both resolves to base, matching the game's own binder precedence.
pub fn locate_sfx_dirs() -> Vec<PathBuf> {
    if let Ok(raw) = std::env::var(SFX_DIRS_ENV) {
        let dirs: Vec<PathBuf> = raw
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }
    let sfx = patch_dir().join("sfx");
    SFX_BINDERS
        .iter()
        .map(|b| sfx.join(b).join("effect"))
        .filter(|p| p.is_dir())
        .collect()
}

pub fn locate_paramdex_defs() -> Option<PathBuf> {
    if let Some(p) = from_env(PARAMDEX_DIR_ENV) {
        return Some(p);
    }
    let vendored = data_root().join("data/paramdex/ER/Defs");
    vendored.is_dir().then_some(vendored)
}

pub fn generated_item_text_path() -> PathBuf {
    data_root().join("data/reference/generated/item-text/item-text.json")
}

fn from_env(var: &str) -> Option<PathBuf> {
    let raw = std::env::var(var).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paramdex_defs_are_vendored_and_findable() {
        // This one is committed data, so it must always resolve.
        let defs = locate_paramdex_defs().expect("vendored paramdex defs should always be found");
        assert!(defs.join("MagicParam.xml").is_file());
        assert!(defs.join("SpEffectVfx.xml").is_file());
    }

    #[test]
    #[ignore = "reads an external game install — see docs/known-offsets.md"]
    fn finds_item_msgbnd_and_oodle_on_this_machine() {
        let binder = game_item_msgbnd().expect("item msgbnd should be found");
        eprintln!("binder: {}", binder.display());
        assert!(binder
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("item"));

        let oodle = locate_oodle_dll().expect("oo2core_6_win64.dll should be found");
        eprintln!("oodle:  {}", oodle.display());
        assert!(oodle.is_file());
    }
}
