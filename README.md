# SoulsSpellCraft Engine

The Rust side of [SoulsSpellCraft](https://github.com/Gingerlief/SoulsSpellCraft) — it's what
actually opens `regulation.bin`, the item text, and the SFX binders. It all ships as one
binary, `xtask`, which the editor runs for you.

## What you need

Rust (stable) and a copy of Elden Ring. No game data lives in this repo.

Oodle files are read using the `oo2core_6_win64.dll` already sitting in your install, loaded at
run time — nothing proprietary gets copied or shipped, and `cargo build` works fine on a
machine with no game on it.

## Build

```bash
cargo build --release -p xtask
```

Lands at `target/release/xtask.exe`. That's the binary the editor uses.

## Where it looks for things

Three environment variables, all optional — a normal Steam install and this repo's own
`launch/patched/` get found on their own.

| | |
|---|---|
| `SSC_GAME_DIR` | your Elden Ring `Game` folder — only ever read |
| `SSC_PATCH_DIR` | the working copy; everything is read from and written to here |
| `SSC_EXPORT_DIR` | where spell documents land |

Everything else — the regulation, the item binder, the Oodle DLL, the effect folders — falls
out of those.

## xtask

Run it bare to see the full list. The two you want first:

```bash
xtask init                 # copy regulation.bin, msg/ and sfx/ out of your install
xtask spell export --all   # a JSON document per spell, which is what the editor reads
```

`init` leaves alone anything already there unless you give it `--force`. `msg/` and `sfx/` only
exist if you've run UXM on your install — it'll say so if they're missing, and the editor still
works without them.

The rest, roughly in order of how often you'll touch them:

```bash
xtask craft new-dummy       # grab an empty spell slot
xtask craft apply <patch>   # insert the rows, write the regulation and its item text
xtask sfx unpack / pack     # explode and rebuild the SFX binders
xtask msg dump / write      # item names and descriptions
xtask fxr where <id>        # which file backs an sfx id
```

Add `--help` to most of them for their options.

`sfx pack` defaults to Oodle level 4 — about 6 seconds for the big common-effects binder.
`--level 6` is what the game itself ships at, and takes roughly two minutes for about 3% less
size, so it's rarely worth it.

## Layout

```
crates/souls-format       regulation.bin, PARAMDEF, FXR, and the Oodle/DCX/BND4/FMG stack
crates/spellcraft-engine  graph walking, craft patches, the export document
xtask                     the CLI
```

## Thanks

- [SoulsFormatsNEXT](https://github.com/soulsmods/SoulsFormatsNEXT) the reference every
  format reader and writer here was written against. If something in this repo is right, it's
  because that existed to check against.
- [fromsoftware-rs](https://github.com/vswarte/fromsoftware-rs) for proving out what a Rust-side
  FromSoft toolchain looks like.
