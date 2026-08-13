// Docs: docs/souls-format/tests/fxr_shape.md

use std::collections::BTreeSet;
use std::path::PathBuf;

use souls_format::fxr::{Action, Field, Property, SFX_REFERENCE_ACTION_ID};
use souls_format::Fxr;

fn reference_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data/reference")
}

fn sample(name: &str) -> Fxr {
    let path = reference_dir().join("fxr-samples").join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
    Fxr::parse(&bytes).unwrap_or_else(|e| panic!("parsing {path:?}: {e}"))
}

fn properties(action: &Action) -> impl Iterator<Item = &Property> {
    action.properties1.iter().chain(action.properties2.iter())
}

struct Shape {
    actions: usize,
    distinct_types: BTreeSet<i16>,
    properties: usize,
    proxies: Vec<i32>,
}

fn shape(fxr: &Fxr) -> Shape {
    let actions = fxr.actions();
    Shape {
        actions: actions.len(),
        distinct_types: actions.iter().map(|a| a.id).collect(),
        properties: actions.iter().map(|a| properties(a).count()).sum(),
        proxies: fxr.proxy_targets(),
    }
}

const SAMPLES: &[&str] = &[
    "f000523002.fxr",
    "f000523003.fxr",
    "f000529972.fxr",
    "f000529982.fxr",
];

#[test]
fn the_samples_are_the_shape_the_survey_describes() {
    println!(
        "\n{:<16} {:>8} {:>7} {:>11}  proxies",
        "file", "actions", "types", "properties"
    );
    for name in SAMPLES {
        let s = shape(&sample(name));
        println!(
            "{:<16} {:>8} {:>7} {:>11}  {:?}",
            name,
            s.actions,
            s.distinct_types.len(),
            s.properties,
            s.proxies
        );
    }

    // Glintstone Pebble's bullet effect. The figure the "grouping is not optional" argument
    // rests on — a tree of this many anonymous entries is not a preview.
    let pebble = shape(&sample("f000523002.fxr"));
    assert_eq!(pebble.actions, 308, "action count in f000523002");
    assert_eq!(pebble.distinct_types.len(), 34, "distinct action types");

    // The Gravity Well emitter, which exists mainly to point at the effect that draws.
    // A preview ignoring proxy edges shows an empty emitter, so this pair is load-bearing.
    let well = shape(&sample("f000529982.fxr"));
    assert_eq!(well.proxies, vec![529972], "529982 proxies to 529972");
    assert!(
        well.actions < pebble.actions / 10,
        "an emitter is a fraction of the size of what it points at"
    );
}

#[test]
fn action_zero_is_the_most_common_thing_in_a_real_effect() {
    let fxr = sample("f000523002.fxr");
    let zeros = fxr.actions().iter().filter(|a| a.id == 0).count();
    println!(
        "\naction id 0 appears {zeros} times in f000523002 of {} total",
        fxr.actions().len()
    );

    // `actions.json` does not name id 0, and it is by far the most common entry — so any
    // grouping that drops unnamed actions drops a third of the tree. Recorded as an open
    // question in docs/fxr-preview.md; pinned here so the answer is measurable.
    assert!(zeros > 100, "id 0 dominates a real effect");
}

#[test]
fn the_proxy_action_is_the_one_the_constant_names() {
    let fxr = sample("f000529982.fxr");
    let referencing: Vec<i32> = fxr
        .actions()
        .iter()
        .filter(|a| a.id == SFX_REFERENCE_ACTION_ID)
        .filter_map(|a| match a.fields1.first() {
            Some(Field::Int(v)) => Some(*v),
            _ => None,
        })
        .collect();
    assert_eq!(
        referencing,
        fxr.proxy_targets(),
        "fields1[0] of action 132 is the target"
    );
}

// --- the semantics layer ---------------------------------------------------------------

fn semantics() -> serde_json::Value {
    let path = reference_dir().join("fxr-semantics/actions.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

#[test]
fn actions_json_is_keyed_by_array_index_and_must_be_read_by_type() {
    let all = semantics();
    let entries = all.as_array().expect("actions.json is a top-level array");

    // The trap this whole test exists to make permanent. Entry 0 describes action *1*, so
    // `entries[id]` is wrong for every id — and wrong in a way that still returns a
    // plausible-looking action, which is why it went unnoticed long enough to be written
    // into a survey. Read the `type` field.
    assert_eq!(entries[0]["type"], 1, "the first entry is not action 0");
    assert_ne!(
        entries.len(),
        11_001,
        "not a dense array indexed by action id"
    );

    let by_type: BTreeSet<i64> = entries.iter().filter_map(|e| e["type"].as_i64()).collect();
    assert!(
        by_type.contains(&132),
        "SFXReference is present, keyed by type"
    );
    assert_eq!(
        entries
            .iter()
            .find(|e| e["type"] == 132)
            .and_then(|e| e["name"].as_str()),
        Some("SFXReference"),
    );

    // Coverage over the types a real spell's effects actually use, which is the number that
    // decides whether a named tree view is worth building. Indexing by position instead
    // gives a far worse figure and a badly wrong impression.
    let used: BTreeSet<i16> = SAMPLES
        .iter()
        .flat_map(|n| shape(&sample(n)).distinct_types)
        .collect();
    let named = used
        .iter()
        .filter(|id| by_type.contains(&(**id as i64)))
        .count();
    println!(
        "\n{named} of {} action types used across the samples are named by actions.json",
        used.len()
    );
    let unnamed: Vec<i16> = used
        .iter()
        .copied()
        .filter(|id| !by_type.contains(&(*id as i64)))
        .collect();
    println!("unnamed: {unnamed:?}");

    assert!(
        named * 100 / used.len() >= 85,
        "coverage read by type should be most of them, got {named}/{}",
        used.len()
    );
}

#[test]
fn sizing_a_document_facet() {
    let used: BTreeSet<i16> = SAMPLES
        .iter()
        .flat_map(|n| shape(&sample(n)).distinct_types)
        .collect();

    // Metadata, trimmed to what a structural inspector shows: the name, the slot it fills,
    // whether it draws, and English descriptions for the action and its properties.
    let all = semantics();
    let mut meta = serde_json::Map::new();
    for entry in all.as_array().unwrap() {
        let Some(ty) = entry["type"].as_i64() else {
            continue;
        };
        if !used.contains(&(ty as i16)) {
            continue;
        }
        let mut props = serde_json::Map::new();
        if let Some(p) = entry["properties"].as_object() {
            for (name, def) in p {
                props.insert(name.clone(), def["desc"]["en-US"].clone());
            }
        }
        meta.insert(
            ty.to_string(),
            serde_json::json!({
                "name": entry["name"],
                "slot": entry["slot"],
                "meta": entry["meta"],
                "desc": entry["desc"]["en-US"],
                "properties": props,
            }),
        );
    }
    let meta_bytes = serde_json::to_string(&meta).unwrap().len();

    // Instances, in three candidate shapes. The choice between them is the whole design
    // question, because the metadata is shareable and the instances are not: they belong to
    // the node, and a spell reaches seven or so FXR nodes.
    println!("\n--- document facet sizing ---");
    println!("action types used across samples: {}", used.len());
    println!(
        "shared metadata for those types:  {meta_bytes:>8} bytes  \
         (fieldMeta, already carried per document, is ~110 KB)"
    );
    println!(
        "\n{:<16} {:>11} {:>11} {:>11} {:>11}",
        "file", "keyframes", "summarised", "named only", "tree only"
    );

    let names = FieldNames::load(&all);
    let mut totals = [0usize; 4];
    for name in SAMPLES {
        let fxr = sample(name);
        let sizes = [
            json_len(&instances(&fxr, Detail::Keyframes, &names)),
            json_len(&instances(&fxr, Detail::Summarised, &names)),
            json_len(&instances(&fxr, Detail::NamedOnly, &names)),
            json_len(&instances(&fxr, Detail::TreeOnly, &names)),
        ];
        println!(
            "{name:<16} {:>11} {:>11} {:>11} {:>11}",
            sizes[0], sizes[1], sizes[2], sizes[3]
        );
        for (t, s) in totals.iter_mut().zip(sizes) {
            *t += s;
        }
    }
    println!(
        "{:<16} {:>11} {:>11} {:>11} {:>11}   <- four nodes",
        "total", totals[0], totals[1], totals[2], totals[3]
    );

    // Three facts, pinned because each one rules out a design that looks obvious.
    //
    // Keyframes are about half the payload, so deferring curves to tier 2 helps but does not
    // decide anything.
    assert!(
        totals[1] < totals[0] * 4 / 5 && totals[1] > totals[0] / 3,
        "keyframes are roughly half the payload, got {} of {}",
        totals[1],
        totals[0]
    );

    // Dropping the field positions `actions.json` cannot name barely helps — which is the
    // finding, because it is the trim everyone reaches for first. The payload is not made of
    // unknown values; it is made of *how many* actions and properties a real effect has.
    assert!(
        totals[2] > totals[1] * 4 / 5,
        "trimming unnamed fields is not the lever: {} vs {}",
        totals[2],
        totals[1]
    );

    // The structure alone — which actions exist, in order — is a rounding error. Anything
    // that has to stay small should carry this and resolve the rest against the metadata or
    // fetch it on demand.
    assert!(
        totals[3] * 20 < totals[0],
        "the bare tree should be a small fraction of the whole, got {} of {}",
        totals[3],
        totals[0]
    );
}

enum Detail {
    Keyframes,
    Summarised,
    NamedOnly,
    TreeOnly,
}

struct FieldNames {
    by_type: std::collections::HashMap<i64, Vec<String>>,
}

impl FieldNames {
    fn load(actions: &serde_json::Value) -> Self {
        let mut by_type = std::collections::HashMap::new();
        for entry in actions.as_array().unwrap() {
            let Some(ty) = entry["type"].as_i64() else {
                continue;
            };
            let names = entry["structure"]["EldenRing"]["fields1"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|n| n.as_str().unwrap_or_default().to_string())
                        .collect()
                })
                .unwrap_or_default();
            by_type.insert(ty, names);
        }
        FieldNames { by_type }
    }

    fn is_named(&self, action_type: i16, i: usize) -> bool {
        match self.by_type.get(&(action_type as i64)) {
            None => true,
            Some(names) => names.get(i).is_none_or(|n| !n.starts_with("unk")),
        }
    }
}

fn instances(fxr: &Fxr, detail: Detail, names: &FieldNames) -> Vec<serde_json::Value> {
    let summarise = |a: &Action| {
        properties(a)
            .map(|p| {
                serde_json::json!({
                    "type": p.property_type,
                    "interpolation": p.interpolation_type,
                    "keyframes": p.fields.len(),
                })
            })
            .collect::<Vec<_>>()
    };

    fxr.actions()
        .iter()
        .map(|a| match detail {
            Detail::TreeOnly => serde_json::json!({ "type": a.id }),
            Detail::NamedOnly => serde_json::json!({
                "type": a.id,
                "fields1": a.fields1.iter().enumerate()
                    .filter(|(i, _)| names.is_named(a.id, *i))
                    .map(|(i, f)| serde_json::json!({ "at": i, "value": field_json(f) }))
                    .collect::<Vec<_>>(),
                "properties": summarise(a),
            }),
            Detail::Summarised => serde_json::json!({
                "type": a.id,
                "fields1": a.fields1.iter().map(field_json).collect::<Vec<_>>(),
                "properties": summarise(a),
            }),
            Detail::Keyframes => serde_json::json!({
                "type": a.id,
                "fields1": a.fields1.iter().map(field_json).collect::<Vec<_>>(),
                "fields2": a.fields2.iter().map(field_json).collect::<Vec<_>>(),
                "properties": properties(a)
                    .map(|p| serde_json::json!({
                        "type": p.property_type,
                        "interpolation": p.interpolation_type,
                        "loop": p.is_loop,
                        "fields": p.fields.iter().map(field_json).collect::<Vec<_>>(),
                    }))
                    .collect::<Vec<_>>(),
            }),
        })
        .collect()
}

fn json_len(v: &[serde_json::Value]) -> usize {
    serde_json::to_string(v).unwrap().len()
}

// --- what is actually editable ----------------------------------------------------------

fn interpolation_name(t: u8) -> &'static str {
    match t {
        0 => "Zero",
        1 => "One",
        2 => "Constant",
        3 => "Stepped",
        4 => "Linear",
        5 => "Curve1",
        6 => "Curve2",
        7 => "UnkAc6",
        _ => "?",
    }
}

#[test]
fn how_many_colour_and_size_properties_are_a_single_value() {
    let all = semantics();
    let names = PropertyNames::load(&all);

    let mut by_interpolation: std::collections::BTreeMap<&str, usize> = Default::default();
    let mut examples: Vec<String> = Vec::new();

    for file in SAMPLES {
        let fxr = sample(file);
        for action in fxr.actions() {
            for (bank, props) in [
                ("properties1", &action.properties1),
                ("properties2", &action.properties2),
            ] {
                for (i, p) in props.iter().enumerate() {
                    let Some(name) = names.get(action.id, bank, i) else {
                        continue;
                    };
                    if !is_colour_or_size(name) {
                        continue;
                    }
                    let kind = interpolation_name(p.interpolation_type);
                    *by_interpolation.entry(kind).or_default() += 1;
                    if examples.len() < 10 {
                        examples.push(format!(
                            "{file} action {} {name}: {kind}, {} keyframe field(s)",
                            action.id,
                            p.fields.len()
                        ));
                    }
                }
            }
        }
    }

    println!("\n--- colour/size properties by interpolation ---");
    let total: usize = by_interpolation.values().sum();
    for (kind, n) in &by_interpolation {
        println!("{kind:<10} {n:>5}  ({}%)", n * 100 / total.max(1));
    }
    println!("total {total}");
    for e in &examples {
        println!("  {e}");
    }

    assert!(
        total > 0,
        "the samples should contain colour and size properties"
    );
}

fn is_colour_or_size(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("color") || n.starts_with("colour") || n.starts_with("size")
}

struct PropertyNames {
    by_type: std::collections::HashMap<i64, serde_json::Value>,
}

impl PropertyNames {
    fn load(actions: &serde_json::Value) -> Self {
        let mut by_type = std::collections::HashMap::new();
        for entry in actions.as_array().unwrap() {
            if let Some(ty) = entry["type"].as_i64() {
                by_type.insert(ty, entry["structure"]["EldenRing"].clone());
            }
        }
        PropertyNames { by_type }
    }

    fn get(&self, action_type: i16, bank: &str, i: usize) -> Option<&str> {
        self.by_type
            .get(&(action_type as i64))?
            .get(bank)?
            .get(i)?
            .as_str()
    }
}

#[test]
fn re_encoding_is_broken_for_all_but_the_smallest_sample() {
    println!("\n--- re-encoding an untouched file ---");
    let mut broken: Vec<&str> = Vec::new();
    for name in SAMPLES {
        let path = reference_dir().join("fxr-samples").join(name);
        let original = std::fs::read(&path).unwrap();
        let parsed = Fxr::parse(&original).unwrap();
        let reencoded = parsed.encode();

        if reencoded.len() != original.len() {
            println!(
                "{name:<16} {} -> {} bytes ({:+})",
                original.len(),
                reencoded.len(),
                reencoded.len() as i64 - original.len() as i64
            );
        } else {
            let differing = (0..original.len())
                .filter(|&i| original[i] != reencoded[i])
                .collect::<Vec<_>>();
            match differing.first() {
                None => println!("{name:<16} identical ({} bytes)", original.len()),
                Some(&first) => println!(
                    "{name:<16} same length, {} of {} bytes differ, first at 0x{first:x}",
                    differing.len(),
                    original.len()
                ),
            }
        }

        // Whether the output can even be read back. `catch_unwind` because a bad offset
        // reaches `cursor.rs` as a slice-range panic rather than an `FxrError` — which is
        // itself worth knowing: `parse` is not total on hostile input.
        let reparsed = std::panic::catch_unwind(|| Fxr::parse(&reencoded));
        match reparsed {
            Err(_) => {
                println!("    -> re-parsing PANICKED");
                broken.push(*name);
            }
            Ok(Err(e)) => {
                println!("    -> re-parsing failed: {e}");
                broken.push(*name);
            }
            Ok(Ok(reparsed)) => {
                let (before, after) = (shape(&parsed), shape(&reparsed));
                if after.actions != before.actions || after.properties != before.properties {
                    println!(
                        "    -> re-parsed to a different tree: {} actions/{} properties \
                         became {}/{}",
                        before.actions, before.properties, after.actions, after.properties
                    );
                    broken.push(*name);
                } else {
                    println!("    -> re-parses to the same tree");
                }
            }
        }
    }

    // Pinned as it is, not as it should be. Three of four is the measured state; asserting
    // zero would leave a red test that says nothing new every run, and asserting nothing
    // would let the encoder quietly get worse. **If this fails because `broken` shrank, the
    // encoder was fixed — update the list and the doc comment above, and go and look at
    // whether the write path can now be trusted.**
    //
    // It will also fail if a fifth file is added to `SAMPLES`, since a new entry lands in
    // this vector. That is a fixture change, not an encoder change — add the file to the
    // list if it is broken too, and take the win if it is not.
    assert_eq!(
        broken,
        vec!["f000523002.fxr", "f000523003.fxr", "f000529972.fxr"],
        "which samples survive a re-encode has changed"
    );
}

#[test]
#[ignore = "reads the unpacked sfx directories — set SSC_SFX_DIRS, see locate.rs"]
fn every_fxr_in_the_editable_corpus_round_trips() {
    let dirs = souls_format::locate::locate_sfx_dirs();
    if dirs.is_empty() {
        eprintln!("skipping: no sfx directories configured");
        return;
    }
    println!("\nchecking {} directories:", dirs.len());
    for d in &dirs {
        println!("  {}", d.display());
    }

    let (mut checked, mut unparseable, mut mismatched) = (0usize, Vec::new(), Vec::new());
    for dir in &dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("fxr") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            match Fxr::parse(&bytes) {
                Err(e) => unparseable.push(format!("{}: {e}", path.display())),
                Ok(fxr) => {
                    checked += 1;
                    if fxr.encode() != bytes {
                        mismatched.push(path.display().to_string());
                    }
                }
            }
        }
    }

    println!(
        "{checked} parsed, {} unparseable, {} mismatched",
        unparseable.len(),
        mismatched.len()
    );
    for m in mismatched.iter().take(10) {
        println!("  mismatch: {m}");
    }
    for u in unparseable.iter().take(10) {
        println!("  unparseable: {u}");
    }
    assert!(
        mismatched.is_empty(),
        "{} files did not round-trip -- a write path would corrupt them",
        mismatched.len()
    );
}

fn field_json(f: &Field) -> serde_json::Value {
    match f {
        Field::Int(v) => serde_json::json!({ "type": "int", "value": v }),
        Field::Float(v) => serde_json::json!({ "type": "float", "value": v }),
    }
}
