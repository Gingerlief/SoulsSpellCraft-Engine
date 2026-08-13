// Docs: docs/xtask/env.md

use std::path::PathBuf;

use souls_format::locate::{locate_paramdex_defs, locate_regulation_bin, locate_sfx_dirs};
use souls_format::{NameIndex, ParamBank, ParamdefLibrary, Regulation, SfxDirectory};
use spellcraft_engine::cache::{hash_file, paramdex_fingerprint};
use spellcraft_engine::graph::WalkOptions;
use spellcraft_engine::source::{ParamTable, RegulationSource};

pub fn repo_root() -> PathBuf {
    souls_format::locate::data_root()
}

pub fn generated_dir() -> PathBuf {
    repo_root().join("data/reference/generated")
}

pub fn default_patched_regulation_path() -> PathBuf {
    repo_root().join("launch/patched/regulation.bin")
}

pub fn reference_dir() -> PathBuf {
    repo_root().join("data/reference")
}

pub fn walk_options() -> WalkOptions {
    WalkOptions::everything()
}

pub struct Env {
    pub source: RegulationSource,
    pub sfx: Option<SfxDirectory>,
    pub names: Option<NameIndex>,
    pub defs: ParamdefLibrary,
    pub regulation_sha256: String,
    pub paramdex_fingerprint: String,
}

impl Env {
    pub fn prepare() -> Result<Env, String> {
        let regulation_path = locate_regulation_bin()
            .ok_or("no regulation.bin found (set SSC_REGULATION_BIN_PATH)")?;
        let defs_dir = locate_paramdex_defs().ok_or("vendored paramdex defs not found")?;

        eprintln!("regulation: {}", regulation_path.display());
        let regulation_sha256 =
            hash_file(&regulation_path).map_err(|e| format!("hashing regulation.bin: {e}"))?;
        eprintln!("sha256:     {regulation_sha256}");

        let paramdex_fingerprint =
            paramdex_fingerprint(&defs_dir).map_err(|e| format!("fingerprinting paramdex: {e}"))?;

        let regulation = Regulation::open(&regulation_path)
            .map_err(|e| format!("opening regulation.bin: {e}"))?;
        let library =
            ParamdefLibrary::open(&defs_dir).map_err(|e| format!("opening paramdex: {e}"))?;
        let source = RegulationSource::new(ParamBank::new(regulation, library));
        let defs =
            ParamdefLibrary::open(&defs_dir).map_err(|e| format!("opening paramdex: {e}"))?;

        let sfx_dirs = locate_sfx_dirs();
        if sfx_dirs.is_empty() {
            eprintln!("sfx dirs:   none found — FXR nodes stay unresolved references");
        } else {
            eprintln!("sfx dirs:   {} configured", sfx_dirs.len());
        }
        let sfx = (!sfx_dirs.is_empty()).then(|| SfxDirectory::new(sfx_dirs));

        let names = NameIndex::open_vendored();
        if names.is_none() {
            eprintln!("names:      none found — nodes will be labelled by id only");
        }

        Ok(Env {
            source,
            sfx,
            names,
            defs,
            regulation_sha256,
            paramdex_fingerprint,
        })
    }

    pub fn all_magic_ids(&self) -> Result<Vec<i64>, String> {
        let mut ids = self
            .source
            .bank()
            .row_ids(ParamTable::Magic.entry_suffix())
            .map_err(|e| format!("listing Magic rows: {e}"))?;
        ids.sort_unstable();
        Ok(ids)
    }
}

pub fn ids_from_arg(env: &Env, arg: Option<&str>) -> Result<Vec<i64>, String> {
    match arg {
        Some("--all") | None => env.all_magic_ids(),
        Some(id) => Ok(vec![id
            .parse()
            .map_err(|_| format!("not a magic id: {id}"))?]),
    }
}
