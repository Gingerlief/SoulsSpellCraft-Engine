// Docs: docs/xtask/graph_cache.md

use std::path::PathBuf;
use std::process::ExitCode;

use spellcraft_engine::cache::{CacheManifest, DiskCache, GraphCache};

use crate::env::{generated_dir, ids_from_arg, walk_options, Env};

fn cache_root() -> PathBuf {
    generated_dir().join("graph-cache")
}

fn manifest_for(env: &Env) -> CacheManifest {
    CacheManifest::new_for(
        env.regulation_sha256.clone(),
        env.paramdex_fingerprint.clone(),
        &walk_options(),
    )
}

fn open_cache(env: &Env) -> Result<GraphCache<'_>, String> {
    let disk = DiskCache::open(&cache_root(), manifest_for(env))
        .map_err(|e| format!("opening cache dir: {e}"))?;
    eprintln!("cache dir:  {}", disk.dir().display());

    let mut cache = GraphCache::in_memory(&env.source, walk_options()).with_disk(disk);
    if let Some(sfx) = &env.sfx {
        cache = cache.with_sfx(sfx);
    }
    if let Some(names) = &env.names {
        cache = cache.with_names(names);
    }
    Ok(cache)
}

pub fn run(mut args: impl Iterator<Item = String>) -> ExitCode {
    match args.next().as_deref() {
        Some("build") => dispatch(build(args.next().as_deref())),
        Some("verify") => dispatch(verify(args.next().as_deref())),
        Some("prune") => dispatch(prune()),
        Some(other) => {
            eprintln!("unknown graph-cache subcommand: {other}");
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
    eprintln!("usage: xtask graph-cache <build|verify|prune> [args]");
    eprintln!();
    eprintln!("  build [<magic_id>|--all]   walk and cache one spell, or every Magic row");
    eprintln!("  verify [<magic_id>]        re-walk and diff against the cache");
    eprintln!("  prune                      delete cache directories for other regulations");
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

fn build(arg: Option<&str>) -> Result<(), String> {
    let env = Env::prepare()?;
    let cache = open_cache(&env)?;
    let ids = ids_from_arg(&env, arg)?;

    eprintln!("building {} graph(s)...", ids.len());
    let (mut ok, mut failed) = (0usize, 0usize);
    let mut worst: Vec<(i64, usize)> = Vec::new();

    for id in &ids {
        match cache.get(*id) {
            Ok(graph) => {
                ok += 1;
                worst.push((*id, graph.nodes.len()));
            }
            Err(e) => {
                failed += 1;
                // A spell whose own Magic row won't decode is worth reporting, not fatal.
                eprintln!("  {id}: {e}");
            }
        }
    }

    worst.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    eprintln!("built {ok}, failed {failed}");
    if let Some((id, n)) = worst.first() {
        eprintln!("largest graph: spell {id} with {n} nodes");
    }
    Ok(())
}

fn verify(arg: Option<&str>) -> Result<(), String> {
    let env = Env::prepare()?;
    let cache = open_cache(&env)?;

    let ids: Vec<i64> = match arg {
        Some(id) => vec![id.parse().map_err(|_| format!("not a magic id: {id}"))?],
        None => cache.disk().map(|d| d.cached_ids()).unwrap_or_default(),
    };

    if ids.is_empty() {
        eprintln!("nothing cached to verify — run `xtask graph-cache build --all` first");
        return Ok(());
    }

    eprintln!("verifying {} cached graph(s)...", ids.len());
    let mut mismatched = Vec::new();
    for id in &ids {
        match cache.verify(*id) {
            Ok(true) => {}
            Ok(false) => mismatched.push(*id),
            Err(e) => eprintln!("  {id}: {e}"),
        }
    }

    if mismatched.is_empty() {
        eprintln!("all {} cached graphs match a fresh walk", ids.len());
        Ok(())
    } else {
        Err(format!(
            "{} cached graph(s) diverged from a fresh walk: {mismatched:?}",
            mismatched.len()
        ))
    }
}

fn prune() -> Result<(), String> {
    let env = Env::prepare()?;
    let root = cache_root();
    let keep = &env.regulation_sha256;

    let Ok(entries) = std::fs::read_dir(&root) else {
        eprintln!("no cache directory at {}", root.display());
        return Ok(());
    };

    let mut removed = 0usize;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name();
        if name.to_str() == Some(keep.as_str()) {
            continue;
        }
        eprintln!("removing {}", entry.path().display());
        std::fs::remove_dir_all(entry.path()).map_err(|e| format!("removing cache dir: {e}"))?;
        removed += 1;
    }
    eprintln!(
        "pruned {removed} stale cache director{}",
        if removed == 1 { "y" } else { "ies" }
    );
    Ok(())
}
