// Docs: docs/spellcraft-engine/export.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use souls_format::paramdef::{DefType, ParamValue};
use souls_format::ParamdefLibrary;

use crate::graph::model::{
    CastType, Classification, EdgeResolution, NodeKind, NodeStatus, Presentation, SpellGraph,
    GRAPH_SCHEMA_VERSION, WALKER_VERSION,
};

pub const EXPORT_SCHEMA_VERSION: u32 = 4;

#[cfg(feature = "ts")]
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct SpellDocument {
    pub schema_version: u32,
    pub graph_schema_version: u32,
    pub walker_version: u32,
    pub source: DocumentSource,
    pub spell: SpellSummary,
    pub nodes: Vec<DocNode>,
    pub edges: Vec<DocEdge>,
    pub field_meta: BTreeMap<String, ParamTypeMeta>,
    pub link_fields: BTreeMap<String, crate::link_fields::LinkFields>,
    pub diagnostics: Vec<DocDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct DocumentSource {
    pub regulation_sha256: Option<String>,
    pub paramdex_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct SpellSummary {
    pub magic_id: i64,
    pub name: Option<String>,
    pub icon_id: Option<i64>,
    pub text_source: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub classifications: Vec<String>,
    pub generated_row_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct DocNode {
    pub key: String,
    pub kind: String,
    pub id: i64,
    pub name: Option<String>,
    pub status: String,
    pub status_detail: Option<String>,
    pub param_type: Option<String>,
    pub table: Option<String>,
    pub cast_types: Vec<String>,
    pub fields: Vec<DocField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct DocField {
    pub name: String,
    pub value: DocValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum DocValue {
    Int(i64),
    Float(f32),
    Double(f64),
    Str(String),
    Bytes(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct DocEdge {
    pub from: String,
    pub to: String,
    pub field: String,
    pub resolution: String,
    pub cast_type: Option<String>,
    pub ref_category: Option<i32>,
    pub consume_type: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct ParamTypeMeta {
    pub param_type: String,
    pub description: Option<String>,
    pub fields: BTreeMap<String, FieldMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct FieldMeta {
    pub display_name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct SpellIndex {
    pub schema_version: u32,
    pub graph_schema_version: u32,
    pub walker_version: u32,
    pub source: DocumentSource,
    pub spells: Vec<SpellIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct SpellIndexEntry {
    pub magic_id: i64,
    pub file: String,
    pub name: Option<String>,
    pub icon_id: Option<i64>,
    pub classifications: Vec<String>,
    pub node_count: usize,
    pub edge_count: usize,
    pub generated_row_count: usize,
    pub diagnostic_count: usize,
    pub unresolved_node_count: usize,
}

impl SpellIndexEntry {
    pub fn of(doc: &SpellDocument, file: String) -> Self {
        SpellIndexEntry {
            magic_id: doc.spell.magic_id,
            file,
            name: doc.spell.name.clone(),
            icon_id: doc.spell.icon_id,
            classifications: doc.spell.classifications.clone(),
            node_count: doc.nodes.len(),
            edge_count: doc.edges.len(),
            generated_row_count: doc.spell.generated_row_count,
            diagnostic_count: doc.diagnostics.len(),
            unresolved_node_count: doc.nodes.iter().filter(|n| n.status != "resolved").count(),
        }
    }
}

impl SpellIndex {
    pub fn new(source: DocumentSource, mut spells: Vec<SpellIndexEntry>) -> Self {
        spells.sort_by_key(|s| s.magic_id);
        SpellIndex {
            schema_version: EXPORT_SCHEMA_VERSION,
            graph_schema_version: GRAPH_SCHEMA_VERSION,
            walker_version: WALKER_VERSION,
            source,
            spells,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct DocDiagnostic {
    pub node: Option<String>,
    pub field: Option<String>,
    pub kind: String,
    pub detail: String,
}

// --- Field metadata loading -------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawParamNotes {
    #[serde(rename = "Param")]
    param: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "Fields")]
    fields: Vec<RawFieldNote>,
}

#[derive(Debug, Deserialize)]
struct RawFieldNote {
    #[serde(rename = "Field")]
    field: String,
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Description")]
    description: Option<String>,
}

pub fn load_field_meta(reference_dir: &std::path::Path) -> BTreeMap<String, ParamTypeMeta> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(reference_dir) else {
        return out;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_notes = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with("_PARAM_ST.json"));
        if !is_notes {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(notes) = serde_json::from_str::<RawParamNotes>(&text) else {
            continue;
        };

        let fields = notes
            .fields
            .into_iter()
            .map(|f| {
                (
                    f.field,
                    FieldMeta {
                        display_name: f.name,
                        description: f.description,
                    },
                )
            })
            .collect();

        out.insert(
            notes.param.clone(),
            ParamTypeMeta {
                param_type: notes.param,
                description: notes.description,
                fields,
            },
        );
    }
    out
}

// --- Building the document --------------------------------------------------------

fn kind_str(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Magic => "magic",
        NodeKind::Bullet => "bullet",
        NodeKind::Atk => "atk",
        NodeKind::SpEffect => "spEffect",
        NodeKind::SpEffectVfx => "spEffectVfx",
        NodeKind::Fxr => "fxr",
        NodeKind::Goods => "goods",
    }
}

fn status_parts(status: &NodeStatus) -> (&'static str, Option<String>) {
    match status {
        NodeStatus::Resolved => ("resolved", None),
        NodeStatus::RowMissing => ("rowMissing", None),
        NodeStatus::RowUndecodable { error } => ("rowUndecodable", Some(error.clone())),
        NodeStatus::TableUnavailable { error } => ("tableUnavailable", Some(error.clone())),
        NodeStatus::FxrFileMissing => ("fxrFileMissing", None),
        NodeStatus::FxrUnparseable { error } => ("fxrUnparseable", Some(error.clone())),
    }
}

fn resolution_str(r: EdgeResolution) -> &'static str {
    match r {
        EdgeResolution::Declared => "declared",
        EdgeResolution::ProbedBullet => "probedBullet",
        EdgeResolution::ProbedAtk => "probedAtk",
        EdgeResolution::Unresolvable => "unresolvable",
        EdgeResolution::IdConvention => "idConvention",
    }
}

fn cast_str(c: CastType) -> String {
    match c {
        CastType::Default => "default".to_string(),
        CastType::Charged => "charged".to_string(),
        CastType::None => "none".to_string(),
        CastType::Unknown(v) => format!("unknown({v})"),
    }
}

fn classification_str(c: Classification) -> &'static str {
    match c {
        Classification::Projectile => "projectile",
        Classification::DirectAttack => "directAttack",
        Classification::Charged => "charged",
        Classification::ChildBullet => "childBullet",
        Classification::Status => "status",
    }
}

fn to_doc_value(v: &ParamValue) -> DocValue {
    match v {
        ParamValue::I64(x) => DocValue::Int(*x),
        ParamValue::F32(x) => DocValue::Float(*x),
        ParamValue::F64(x) => DocValue::Double(*x),
        ParamValue::Str(s) => DocValue::Str(s.clone()),
        ParamValue::Bytes(b) => {
            DocValue::Bytes(b.iter().map(|byte| format!("{byte:02x}")).collect())
        }
    }
}

pub fn build_document(
    graph: &SpellGraph,
    defs: &ParamdefLibrary,
    field_meta: BTreeMap<String, ParamTypeMeta>,
    source: DocumentSource,
) -> SpellDocument {
    let default_presentation = Presentation::default();
    let p = graph.presentation.as_ref().unwrap_or(&default_presentation);

    let nodes: Vec<DocNode> = graph
        .nodes
        .iter()
        .map(|node| {
            let param_type = node.key.kind.table().map(|t| t.param_type().to_string());
            let table = node.key.kind.table().map(|t| t.entry_suffix().to_string());
            let (status, status_detail) = status_parts(&node.status);

            // Paramdef order, padding excluded. A node whose row didn't resolve simply
            // has no fields — its status says why.
            let fields = match (&node.row, param_type.as_deref()) {
                (Some(row), Some(pt)) => defs
                    .by_param_type(pt)
                    .map(|def| {
                        def.fields
                            .iter()
                            .filter(|f| f.display_type != DefType::Dummy8)
                            .filter_map(|f| {
                                row.fields.get(&f.internal_name).map(|v| DocField {
                                    name: f.internal_name.clone(),
                                    value: to_doc_value(v),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            };

            DocNode {
                key: node.key.to_string(),
                kind: kind_str(node.key.kind).to_string(),
                id: node.key.id,
                name: None, // filled in below
                status: status.to_string(),
                status_detail,
                param_type,
                table,
                cast_types: graph
                    .cast_reachability
                    .get(&node.key)
                    .map(|set| set.iter().map(|c| cast_str(*c)).collect())
                    .unwrap_or_default(),
                fields,
            }
        })
        .collect();

    let edges = graph
        .edges
        .iter()
        .map(|e| DocEdge {
            from: e.from.to_string(),
            to: e.to.to_string(),
            field: e.field.to_string(),
            resolution: resolution_str(e.resolution).to_string(),
            // Slot edges only. `consume_type.is_some()` is an exact discriminator for
            // them — verified across every Magic row in the regulation: 905 edges carry
            // it, none of them from anywhere but the root, and no root refId edge lacks
            // it. See `edge_cast_type_is_only_published_for_real_slot_edges`.
            cast_type: e.consume_type.map(|_| cast_str(e.cast_type)),
            ref_category: e.ref_category,
            consume_type: e.consume_type,
        })
        .collect();

    let diagnostics = graph
        .diagnostics
        .iter()
        .map(|d| DocDiagnostic {
            node: d.node.map(|k| k.to_string()),
            field: d.field.clone(),
            kind: format!("{:?}", d.kind),
            detail: d.detail.clone(),
        })
        .collect();

    SpellDocument {
        schema_version: EXPORT_SCHEMA_VERSION,
        graph_schema_version: GRAPH_SCHEMA_VERSION,
        walker_version: WALKER_VERSION,
        source,
        spell: SpellSummary {
            magic_id: graph.magic_id,
            name: p.name.clone(),
            icon_id: p.icon_id,
            text_source: format!("{:?}", p.text_source),
            summary: p.summary.clone(),
            description: p.description.clone(),
            classifications: graph
                .classifications
                .iter()
                .map(|c| classification_str(*c).to_string())
                .collect(),
            generated_row_count: graph.generated_row_count(),
        },
        // The whole catalogue, not just the types this document contains — see
        // `CATALOGUED_PARAM_TYPES`. An editor composing a graph needs to know what a node
        // *dragged into* the document can reference, and an empty slot contains nothing to
        // infer that from.
        link_fields: crate::link_fields::all_link_fields(),
        nodes,
        edges,
        field_meta,
        diagnostics,
    }
}

pub fn apply_node_names(
    doc: &mut SpellDocument,
    graph: &SpellGraph,
    names: &dyn crate::graph::NameSource,
) {
    for (doc_node, node) in doc.nodes.iter_mut().zip(graph.nodes.iter()) {
        if let Some(list) = node.key.kind.name_list() {
            doc_node.name = names.name(list, node.key.id);
        }
    }
}

// --- TypeScript declarations ------------------------------------------------------

#[cfg(feature = "ts")]
pub fn typescript_declarations() -> String {
    use std::fmt::Write as _;
    use ts_rs::{Config, TS};

    let cfg = Config::new().with_large_int("number");
    let mut out = String::new();

    out.push_str(concat!(
        "// GENERATED by `cargo run -p xtask -- spell types`. Do not edit by hand.\n",
        "//\n",
        "// Source of truth: crates/spellcraft-engine/src/export.rs in the\n",
        "// morro-ring-spells repo. Regenerate and `git diff` after any change there.\n",
        "//\n",
        "// `kind`, `status`, `resolution` and `castType` are `string`, not literal unions:\n",
        "// their values come from hand-written `match` helpers rather than serde, so a\n",
        "// generated union would assert an agreement nothing checks. Two are genuinely\n",
        "// open-ended (`castType` can be `unknown(3)`). Narrow them locally if you want\n",
        "// exhaustiveness, but keep a default branch.\n\n",
    ));

    // Order is reading order, outermost type first — these are meant to be read.
    for decl in [
        SpellDocument::decl(&cfg),
        DocumentSource::decl(&cfg),
        SpellSummary::decl(&cfg),
        DocNode::decl(&cfg),
        DocField::decl(&cfg),
        DocValue::decl(&cfg),
        DocEdge::decl(&cfg),
        ParamTypeMeta::decl(&cfg),
        FieldMeta::decl(&cfg),
        crate::link_fields::LinkFields::decl(&cfg),
        DocDiagnostic::decl(&cfg),
        SpellIndex::decl(&cfg),
        SpellIndexEntry::decl(&cfg),
        crate::patch::SpellPatch::decl(&cfg),
        crate::patch::FieldEdit::decl(&cfg),
        crate::craft_patch::CraftPatch::decl(&cfg),
        crate::craft_patch::CraftRow::decl(&cfg),
    ] {
        let _ = writeln!(out, "export {decl}\n");
    }

    // The version gate. Typed fields catch shape changes; only this catches a *semantic*
    // change to a field that kept its shape, which is the failure mode that silently
    // produces a wrong edit.
    let patch_version = crate::patch::PATCH_SCHEMA_VERSION;
    let craft_patch_version = crate::craft_patch::CRAFT_PATCH_SCHEMA_VERSION;
    let _ = write!(
        out,
        "\
export const EXPORT_SCHEMA_VERSION = {EXPORT_SCHEMA_VERSION};
export const PATCH_SCHEMA_VERSION = {patch_version};
export const CRAFT_PATCH_SCHEMA_VERSION = {craft_patch_version};
export const GRAPH_SCHEMA_VERSION = {GRAPH_SCHEMA_VERSION};
export const WALKER_VERSION = {WALKER_VERSION};

/**
 * Why three constants but only one hard check:
 *
 * `schemaVersion` describes the document's *shape*. A mismatch means fields this code
 * reads may not exist, so it throws — there is nothing safe to render.
 *
 * `graphSchemaVersion` and `walkerVersion` describe how the graph was *derived*. A bump
 * there means the same shape may now hold different edges (a new reference field, a
 * changed traversal rule). The document is still readable, but it may not agree with a
 * freshly exported one, so it warns rather than refusing — a stale document is still
 * worth looking at, as long as you know it is stale.
 */
export type Incompatibility = {{
  field: 'schemaVersion' | 'graphSchemaVersion' | 'walkerVersion';
  found: number;
  expected: number;
  fatal: boolean;
}};

/** Every version disagreement between a document and this build, worst first. */
export function checkVersions(doc: SpellDocument): Incompatibility[] {{
  const checks: Incompatibility[] = [
    {{
      field: 'schemaVersion',
      found: doc.schemaVersion,
      expected: EXPORT_SCHEMA_VERSION,
      fatal: true,
    }},
    {{
      field: 'graphSchemaVersion',
      found: doc.graphSchemaVersion,
      expected: GRAPH_SCHEMA_VERSION,
      fatal: false,
    }},
    {{
      field: 'walkerVersion',
      found: doc.walkerVersion,
      expected: WALKER_VERSION,
      fatal: false,
    }},
  ];
  return checks
    .filter((c) => c.found !== c.expected)
    .sort((a, b) => Number(b.fatal) - Number(a.fatal));
}}

/**
 * Parses an exported spell document, refusing one this build cannot read.
 *
 * Fail loudly rather than render a document from a different engine version: a shape
 * change is visible, but a field that kept its shape and changed meaning is not.
 */
export function parseSpellDocument(text: string): SpellDocument {{
  const doc = JSON.parse(text) as SpellDocument;
  for (const bad of checkVersions(doc)) {{
    const detail =
      `spell document ${{bad.field}} ${{bad.found}} != ${{bad.expected}} expected by this build`;
    if (bad.fatal) {{
      throw new Error(
        `${{detail}} — re-run \\`xtask spell export\\` against the same engine.`,
      );
    }}
    console.warn(
      `${{detail}}. The document is readable but may not match a fresh walk; ` +
        `re-export to be sure.`,
    );
  }}
  return doc;
}}

/**
 * Parses an export directory's `index.json`.
 *
 * Same gate as a document, for the same reason: an index that describes documents of a
 * shape this build cannot read is worse than no index, because every row in it looks
 * openable.
 */
export function parseSpellIndex(text: string): SpellIndex {{
  const index = JSON.parse(text) as SpellIndex;
  if (index.schemaVersion !== EXPORT_SCHEMA_VERSION) {{
    throw new Error(
      `spell index schemaVersion ${{index.schemaVersion}} != ${{EXPORT_SCHEMA_VERSION}} ` +
        `expected by this build — re-run \\`xtask spell export --all\\`.`,
    );
  }}
  return index;
}}
"
    );

    out
}

#[cfg(all(test, feature = "ts"))]
mod ts_tests {
    use super::*;

    #[test]
    fn declarations_use_number_not_bigint() {
        let ts = typescript_declarations();
        assert!(
            !ts.contains("bigint"),
            "ids arrive via JSON.parse as `number`; a `bigint` declaration would describe \
             a value the runtime never produces:\n{ts}"
        );
        assert!(ts.contains("magicId: number"), "{ts}");
    }

    #[test]
    fn declarations_cover_every_exported_type() {
        let ts = typescript_declarations();
        for name in [
            "SpellDocument",
            "DocumentSource",
            "SpellSummary",
            "DocNode",
            "DocField",
            "DocValue",
            "DocEdge",
            "ParamTypeMeta",
            "FieldMeta",
            "DocDiagnostic",
            "SpellIndex",
            "SpellIndexEntry",
            "SpellPatch",
            "FieldEdit",
        ] {
            assert!(
                ts.contains(&format!("export type {name} =")),
                "missing declaration for {name}"
            );
        }
    }

    #[test]
    fn version_gate_matches_the_rust_constants() {
        let ts = typescript_declarations();
        assert!(ts.contains(&format!(
            "export const EXPORT_SCHEMA_VERSION = {EXPORT_SCHEMA_VERSION};"
        )));
        assert!(ts.contains(&format!(
            "export const PATCH_SCHEMA_VERSION = {};",
            crate::patch::PATCH_SCHEMA_VERSION
        )));
        assert!(ts.contains(&format!(
            "export const GRAPH_SCHEMA_VERSION = {GRAPH_SCHEMA_VERSION};"
        )));
        assert!(ts.contains(&format!("export const WALKER_VERSION = {WALKER_VERSION};")));
    }

    #[test]
    fn every_exported_version_is_checked_somewhere() {
        let ts = typescript_declarations();
        for field in ["schemaVersion", "graphSchemaVersion", "walkerVersion"] {
            assert!(
                ts.contains(&format!("field: '{field}'")),
                "{field} is exported as a constant but never checked"
            );
        }
        // Shape mismatch is fatal; derivation mismatch is a warning. See the doc comment
        // on `checkVersions` in the emitted module.
        assert_eq!(ts.matches("fatal: true").count(), 1);
        assert_eq!(ts.matches("fatal: false").count(), 2);
    }

    #[test]
    fn doc_value_stays_a_discriminated_union() {
        let ts = typescript_declarations();
        assert!(ts.contains(r#"{ "type": "int", "value": number }"#), "{ts}");
        assert!(ts.contains(r#"{ "type": "str", "value": string }"#), "{ts}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{WalkOptions, Walker};
    fn build() -> (SpellDocument, ParamdefLibrary) {
        let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
        let defs = ParamdefLibrary::open_vendored().unwrap();
        let names = souls_format::NameIndex::open_vendored().unwrap();
        let graph = Walker::new(&src)
            .with_options(WalkOptions::everything())
            .with_names(&names)
            .walk(4000)
            .unwrap();

        let mut doc = build_document(
            &graph,
            &defs,
            load_field_meta(&crate::test_support::reference_dir()),
            DocumentSource {
                regulation_sha256: Some("test".into()),
                paramdex_fingerprint: None,
            },
        );
        apply_node_names(&mut doc, &graph, &names);
        (doc, defs)
    }

    #[test]
    fn document_is_self_contained_and_named() {
        let (doc, _) = build();
        assert_eq!(doc.schema_version, EXPORT_SCHEMA_VERSION);
        assert_eq!(doc.spell.magic_id, 4000);
        assert_eq!(
            doc.spell.name.as_deref(),
            Some("[Sorcery] Glintstone Pebble")
        );
        assert!(doc
            .spell
            .classifications
            .contains(&"projectile".to_string()));

        // Per-node names, not just the spell root — the comprehension win.
        let bullet = doc
            .nodes
            .iter()
            .find(|n| n.key == "Bullet:10400000")
            .unwrap();
        assert!(bullet.name.is_some(), "nodes should carry readable names");
    }

    #[test]
    fn fields_are_in_paramdef_order_with_padding_excluded() {
        let (doc, defs) = build();
        let magic = doc.nodes.iter().find(|n| n.kind == "magic").unwrap();
        let def = defs.by_param_type("MAGIC_PARAM_ST").unwrap();

        let paramdef_order: Vec<&str> = def
            .fields
            .iter()
            .filter(|f| f.display_type != DefType::Dummy8)
            .map(|f| f.internal_name.as_str())
            .collect();
        let actual: Vec<&str> = magic.fields.iter().map(|f| f.name.as_str()).collect();

        // A *subsequence* check rather than equality, because a row can legitimately lack
        // a paramdef field. The vendored CSV fixtures are the live example: they were
        // exported against an older Paramdex that called two fields
        // `enable_friendly`/`enable_hostile`, which the current defs name
        // `enable_white`/`enable_black`. Skipping absent fields (rather than erroring, or
        // emitting a placeholder) is what lets a slightly-stale source still export.
        let mut expected = paramdef_order.iter().copied().peekable();
        for name in &actual {
            loop {
                let next = expected
                    .next()
                    .unwrap_or_else(|| panic!("{name} is not in paramdef order, or appears twice"));
                if next == *name {
                    break;
                }
            }
        }

        assert!(magic.fields.len() > 100, "expected ~108 real Magic fields");
        assert!(
            !actual
                .iter()
                .any(|n| n.starts_with("pad") || *n == "disableParamReserve1"),
            "padding should be excluded"
        );

        // Values that "look empty" are still present — filtering them is the C# mistake
        // that made FollowType == 1 invisible.
        assert!(magic
            .fields
            .iter()
            .any(|f| matches!(f.value, DocValue::Int(-1))));
        assert!(magic
            .fields
            .iter()
            .any(|f| matches!(f.value, DocValue::Int(0))));
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn cast_reachability_survives_dropping_per_edge_cast_type() {
        use crate::graph::model::{CastType, NodeKey};
        use std::collections::{BTreeMap, BTreeSet, VecDeque};

        let Some(path) = souls_format::locate::locate_regulation_bin() else {
            eprintln!("skipping: no regulation.bin found");
            return;
        };
        let bank = souls_format::ParamBank::new(
            souls_format::Regulation::open(&path).unwrap(),
            ParamdefLibrary::open_vendored().unwrap(),
        );
        let src = crate::source::RegulationSource::new(bank);
        let ids = src
            .bank()
            .row_ids(crate::source::ParamTable::Magic.entry_suffix())
            .unwrap();

        let (mut slot_edges, mut checked) = (0usize, 0usize);
        for id in ids {
            let Ok(g) = Walker::new(&src)
                .with_options(WalkOptions::params_only())
                .walk(id)
            else {
                continue;
            };
            for e in &g.edges {
                if e.consume_type.is_some() {
                    slot_edges += 1;
                    assert_eq!(e.from, g.root(), "spell {id}: consumeType off the root");
                } else {
                    assert!(
                        !(e.from == g.root() && e.ref_category.is_some()),
                        "spell {id}: root refId edge without consumeType"
                    );
                }
            }

            // Reconstruct reachability the way a consumer must: seed from the slot edges,
            // then propagate along structure alone.
            let mut recon: BTreeMap<NodeKey, BTreeSet<CastType>> = BTreeMap::new();
            let mut queue: VecDeque<(NodeKey, CastType)> = VecDeque::new();
            let mut seen: BTreeSet<(NodeKey, CastType)> = BTreeSet::new();
            for e in g.edges.iter().filter(|e| e.from == g.root()) {
                if seen.insert((e.to, e.cast_type)) {
                    queue.push_back((e.to, e.cast_type));
                }
            }
            while let Some((node, cast)) = queue.pop_front() {
                recon.entry(node).or_default().insert(cast);
                for e in g.edges.iter().filter(|e| e.from == node) {
                    if seen.insert((e.to, cast)) {
                        queue.push_back((e.to, cast));
                    }
                }
            }

            for (key, truth) in &g.cast_reachability {
                if *key == g.root() {
                    continue;
                }
                assert_eq!(
                    recon.get(key).cloned().unwrap_or_default(),
                    *truth,
                    "spell {id}: {key} reachability is not recoverable from edges"
                );
                checked += 1;
            }
        }
        assert!(
            slot_edges > 500,
            "expected hundreds of slot edges, got {slot_edges}"
        );
        assert!(
            checked > 1000,
            "expected to check thousands of nodes, got {checked}"
        );
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn real_rows_emit_every_non_padding_field_in_order() {
        let Some(path) = souls_format::locate::locate_regulation_bin() else {
            eprintln!("skipping: no regulation.bin found");
            return;
        };
        let defs = ParamdefLibrary::open_vendored().unwrap();
        let bank = souls_format::ParamBank::new(
            souls_format::Regulation::open(&path).unwrap(),
            ParamdefLibrary::open_vendored().unwrap(),
        );
        let src = crate::source::RegulationSource::new(bank);
        let graph = Walker::new(&src)
            .with_options(WalkOptions::everything())
            .walk(4000)
            .unwrap();
        let doc = build_document(
            &graph,
            &defs,
            BTreeMap::new(),
            DocumentSource {
                regulation_sha256: None,
                paramdex_fingerprint: None,
            },
        );

        let magic = doc.nodes.iter().find(|n| n.kind == "magic").unwrap();
        let expected: Vec<&str> = defs
            .by_param_type("MAGIC_PARAM_ST")
            .unwrap()
            .fields
            .iter()
            .filter(|f| f.display_type != DefType::Dummy8)
            .map(|f| f.internal_name.as_str())
            .collect();
        let actual: Vec<&str> = magic.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn field_meta_is_normalized_not_repeated() {
        let (doc, _) = build();
        // Four tables have vendored notes; Goods and SpEffectVfx have none.
        assert!(doc.field_meta.contains_key("MAGIC_PARAM_ST"));
        assert!(doc.field_meta.contains_key("ATK_PARAM_ST"));

        let magic_meta = &doc.field_meta["MAGIC_PARAM_ST"];
        assert_eq!(
            magic_meta.fields["yesNoDialogMessageId"]
                .display_name
                .as_deref(),
            Some("Dialog Message ID")
        );

        // Nodes reference metadata by param type rather than carrying a copy.
        let magic = doc.nodes.iter().find(|n| n.kind == "magic").unwrap();
        assert_eq!(magic.param_type.as_deref(), Some("MAGIC_PARAM_ST"));
    }

    #[test]
    fn edge_cast_type_is_only_published_for_real_slot_edges() {
        let (doc, _) = build();
        for e in &doc.edges {
            assert_eq!(
                e.cast_type.is_some(),
                e.consume_type.is_some(),
                "castType must track consumeType exactly, but {} -> {} ({}) has \
                 castType={:?} consumeType={:?}",
                e.from,
                e.to,
                e.field,
                e.cast_type,
                e.consume_type
            );
        }
        assert!(
            doc.edges.iter().any(|e| e.cast_type.is_some()),
            "fixture should contain at least one real slot edge"
        );
        // The Goods link is an id correspondence, not a cast — it must never claim one.
        let goods = doc
            .edges
            .iter()
            .find(|e| e.to.starts_with("Goods:"))
            .unwrap();
        assert_eq!(goods.cast_type, None);
    }

    #[test]
    fn nodes_carry_the_casts_that_actually_reach_them() {
        let (doc, _) = build();
        let magic = doc.nodes.iter().find(|n| n.kind == "magic").unwrap();
        assert!(
            !magic.cast_types.is_empty(),
            "the root is reachable under whatever casts its slots declare"
        );
        // Everything the root's slots lead to inherits a cast; nothing is left blank
        // merely because it sits deep in the tree.
        let reached: std::collections::BTreeSet<&str> = doc
            .edges
            .iter()
            .filter(|e| e.cast_type.is_some())
            .map(|e| e.to.as_str())
            .collect();
        for key in reached {
            let node = doc.nodes.iter().find(|n| n.key == key).unwrap();
            assert!(
                !node.cast_types.is_empty(),
                "{key} is reached from a cast slot but carries no castTypes"
            );
        }
    }

    #[test]
    fn goods_edge_is_marked_as_an_id_correspondence() {
        let (doc, _) = build();
        let goods_edge = doc
            .edges
            .iter()
            .find(|e| e.to.starts_with("Goods:"))
            .unwrap();
        assert_eq!(goods_edge.resolution, "idConvention");
        assert_eq!(goods_edge.ref_category, None);
    }

    #[test]
    fn unresolved_nodes_carry_status_but_no_fields() {
        let (doc, _) = build();
        // Bullet 10400099 isn't in the vendored CSV.
        let missing = doc
            .nodes
            .iter()
            .find(|n| n.key == "Bullet:10400099")
            .unwrap();
        assert_eq!(missing.status, "rowMissing");
        assert!(missing.fields.is_empty());
    }

    #[test]
    fn export_is_deterministic() {
        let (a, _) = build();
        let (b, _) = build();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap(),
            "no clock in the document, so identical inputs must produce identical bytes"
        );
    }

    #[test]
    fn document_round_trips_through_json() {
        let (doc, _) = build();
        let json = serde_json::to_string(&doc).unwrap();
        let back: SpellDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back.spell.magic_id, doc.spell.magic_id);
        assert_eq!(back.nodes.len(), doc.nodes.len());
        assert_eq!(back.edges.len(), doc.edges.len());
    }
}
