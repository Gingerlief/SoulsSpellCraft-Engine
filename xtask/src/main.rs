// Docs: docs/xtask/main.md

mod craft;
mod env;
mod fxr;
mod init;
mod graph_cache;
mod msg;
mod sfx;
mod spell;
mod spike;

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("prelaunch") => prelaunch(),
        Some("sandbox-clone") => sandbox_clone(),
        Some("sandbox-deploy") => sandbox_deploy(),
        Some("spike") => spike_dispatch(args),
        Some("graph-cache") => graph_cache::run(args),
        Some("init") => init::run(args),
        Some("msg") => msg::run(args),
        Some("craft") => craft::run(args),
        Some("fxr") => fxr::run(args),
        Some("sfx") => sfx::run(args),
        Some("spell") => spell::run(args),
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_usage();
            ExitCode::FAILURE
        }
        None => {
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!("usage: xtask <command>");
    eprintln!();
    eprintln!("commands:");
    eprintln!("  prelaunch       regenerate a pristine regulation.bin from a known-good base before a play session");
    eprintln!("  sandbox-clone   clone vanilla SFX/effect data into data/reference/generated/ for experimentation");
    eprintln!("  sandbox-deploy  repack sandbox clones back into the game data (via WitchyBND) for in-game validation");
    eprintln!("  spike <name>    Phase 0 spike smoke tests (docs/planning/bootstrap-checklist.md); see 'xtask spike' with no name for the list");
    eprintln!("  graph-cache     build/verify/prune the spell-graph cache; see 'xtask graph-cache' for subcommands");
    eprintln!("  init            seed the working copy from a game install; see 'xtask init'");
    eprintln!("  msg             extract/apply real game item text (GoodsName/Info/Caption); see 'xtask msg'");
    eprintln!("  craft           author/apply a CraftPatch to insert new rows; see 'xtask craft'");
    eprintln!("  spell           export self-contained spell documents for external tooling; see 'xtask spell'");
    eprintln!("  sfx             unpack/pack the SFX binders (replaces WitchyBND); see 'xtask sfx'");
    eprintln!("  fxr             locate the file backing an sfx id; see 'xtask fxr'");
}

fn spike_dispatch(mut args: impl Iterator<Item = String>) -> ExitCode {
    match args.next().as_deref() {
        Some("gravity-pebble") => {
            if spike::gravity_pebble::run() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Some(other) => {
            eprintln!("unknown spike: {other}");
            eprintln!("available spikes: gravity-pebble");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("usage: xtask spike <name>");
            eprintln!("available spikes: gravity-pebble");
            ExitCode::FAILURE
        }
    }
}

fn prelaunch() -> ExitCode {
    eprintln!("xtask prelaunch: not implemented yet — see docs/planning/working-agreement.md ('Single source of truth for state')");
    ExitCode::FAILURE
}

fn sandbox_clone() -> ExitCode {
    eprintln!(
        "xtask sandbox-clone: not implemented yet — see docs/planning/recommendations.md Phase 5"
    );
    ExitCode::FAILURE
}

fn sandbox_deploy() -> ExitCode {
    eprintln!("xtask sandbox-deploy: not implemented yet — see docs/planning/open-blockers-and-scope-decisions.md item 6 (guardrail against full-binder repacks breaking crafted spells)");
    ExitCode::FAILURE
}
