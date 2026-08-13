# FXR semantics — what the actions and enums *mean*

Vendored from **`@cccode/fxr` v32.1.1**, `dist/actions.json` and `dist/enums.json`, copied
verbatim.

`souls-format/src/fxr.rs` knows how to *read* an FXR: containers, effects, actions,
properties, fields. It has no idea what any of it means — action `600` is a number, and
`fields1[1] = 2` is a number at an index. These two files are the missing half:

- **`actions.json`** — 80 action types, of which 78 support Elden Ring. Each carries a
  `name` (`132 → SFXReference`, `600 → PointSprite`), the `slot` it fills
  (`Appearance`, `NodeMovement`, …), `meta.isParticle`/`meta.isAppearance` saying whether it
  draws anything, English descriptions for the action and each of its properties, and a
  `structure` giving **positional names** per game — so `fields1[1]` on a `PointSprite`
  becomes `blendMode`.
- **`enums.json`** — 17 enums (`ActionType`, `BlendMode`, `EmitterShape`, `AttachMode`,
  `DistortionMode`, `LightingMode`, …), members documented.

## Two things to know before using them

**`actions.json` is a top-level array, not a map keyed by action id.** The real id is the
`type` field *inside* each entry, and entry 0 describes action `1`. Indexing by position
returns a plausible-looking wrong answer rather than nothing, which is how it survived long
enough to be written into a survey with a badly wrong coverage figure. Read by `type`.
`crates/souls-format/tests/fxr_shape.rs` pins this.

**This is data, not code.** Using it does not mean running `@cccode/fxr`, and it does not
move FXR knowledge out of Rust — the engine reads these files and keeps being the one place
that understands the format, which is the standing decision recorded in the UI repo's
`docs/fxr-preview.md`. `@cccode/fxr` stays the oracle it already was for the parser.

## Coverage, measured

Of the 38 distinct action types used across the four effects in `../fxr-samples/`, **37 are
named**. The one that is not is id `0`, which is also the most common entry in a real
effect — 103 of `f000523002`'s 308 actions. Whether it is genuinely inert or a default slot
is still open; see `docs/fxr-preview.md`.

Re-measure with:

```bash
cargo test -p souls-format --test fxr_shape -- --nocapture
```
