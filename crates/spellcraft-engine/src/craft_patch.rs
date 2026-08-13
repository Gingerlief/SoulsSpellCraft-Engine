// Docs: docs/spellcraft-engine/craft_patch.md

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::export::{DocValue, DocumentSource};
use crate::graph::model::NodeKey;
use crate::patch::FieldEdit;

#[cfg(feature = "ts")]
use ts_rs::TS;

pub const CRAFT_PATCH_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct CraftPatch {
    pub schema_version: u32,
    pub base: DocumentSource,
    pub target_id: i64,
    pub note: Option<String>,
    pub name: Option<String>,
    pub info: Option<String>,
    pub caption: Option<String>,
    pub rows: Vec<CraftRow>,
    #[serde(default)]
    pub edits: Vec<FieldEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct CraftRow {
    pub table: String,
    pub clone_from: i64,
    pub id: Option<i64>,
    pub overrides: BTreeMap<String, DocValue>,
}

#[derive(Debug, thiserror::Error)]
pub enum CraftPatchError {
    #[error("craft patch schemaVersion {found} != {expected} expected by this build")]
    SchemaMismatch { found: u32, expected: u32 },
    #[error(
        "craft patch records no base regulation hash, so there is no way to tell whether \
         it was written against this regulation — refusing"
    )]
    NoBaseRecorded,
    #[error(
        "craft patch was authored against regulation {patch}, but this one is {regulation} \
         — refusing. Re-run the authoring command against the current regulation."
    )]
    BaseMismatch { patch: String, regulation: String },
    #[error("row for {table}: id {id} already exists — the base moved under this patch")]
    RowIdTaken { table: String, id: i64 },
    #[error("row for {table}: clone source {id} does not exist in this regulation")]
    CloneSourceNotFound { table: String, id: i64 },
    #[error("edit names '{node}', which is not a node key: {error}")]
    EditNodeUnparseable { node: String, error: String },
    #[error("edit names '{node}', which is not a param row and cannot be edited")]
    EditNodeNotAParamRow { node: String },
    #[error(
        "edit targets {node}, which does not exist in this regulation — an edit changes a row \
         that is already there; use a row entry to create one"
    )]
    EditTargetNotFound { node: String },
    #[error(
        "edit targets {node}, but this patch also inserts that row — set the value in that \
         row's `overrides` instead, so there is one place the field is decided"
    )]
    EditTargetIsInserted { node: String },
}

impl CraftPatch {
    pub fn new(base: DocumentSource, target_id: i64, rows: Vec<CraftRow>) -> Self {
        CraftPatch {
            schema_version: CRAFT_PATCH_SCHEMA_VERSION,
            base,
            target_id,
            note: None,
            name: None,
            info: None,
            caption: None,
            rows,
            edits: Vec::new(),
        }
    }

    pub fn text_edits(&self) -> Vec<(&'static str, i64, &str)> {
        [
            ("GoodsName", self.name.as_deref()),
            ("GoodsInfo", self.info.as_deref()),
            ("GoodsCaption", self.caption.as_deref()),
        ]
        .into_iter()
        .filter_map(|(pool, text)| text.map(|t| (pool, self.target_id, t)))
        .collect()
    }

    pub fn is_noop(&self) -> bool {
        self.rows.is_empty() && self.edits.is_empty() && self.text_edits().is_empty()
    }

    pub fn check_applicable(
        &self,
        regulation_sha256: &str,
        row_exists: impl Fn(&str, i64) -> bool,
    ) -> Result<(), CraftPatchError> {
        if self.schema_version != CRAFT_PATCH_SCHEMA_VERSION {
            return Err(CraftPatchError::SchemaMismatch {
                found: self.schema_version,
                expected: CRAFT_PATCH_SCHEMA_VERSION,
            });
        }
        match self.base.regulation_sha256.as_deref() {
            None => return Err(CraftPatchError::NoBaseRecorded),
            Some(base) if base != regulation_sha256 => {
                return Err(CraftPatchError::BaseMismatch {
                    patch: base.to_string(),
                    regulation: regulation_sha256.to_string(),
                })
            }
            Some(_) => {}
        }
        for row in &self.rows {
            let target = row.id.unwrap_or(self.target_id);
            if row_exists(&row.table, target) {
                return Err(CraftPatchError::RowIdTaken {
                    table: row.table.clone(),
                    id: target,
                });
            }
            if !row_exists(&row.table, row.clone_from) {
                return Err(CraftPatchError::CloneSourceNotFound {
                    table: row.table.clone(),
                    id: row.clone_from,
                });
            }
        }

        // Edits invert the rule above: a row entry's target must NOT exist, an edit's target
        // must. Checked here rather than at apply time so a patch that is half-valid never
        // gets halfway through inserting before failing.
        for edit in &self.edits {
            let key: NodeKey =
                edit.node
                    .parse()
                    .map_err(|error| CraftPatchError::EditNodeUnparseable {
                        node: edit.node.clone(),
                        error,
                    })?;
            let table = key
                .kind
                .table()
                .ok_or_else(|| CraftPatchError::EditNodeNotAParamRow {
                    node: edit.node.clone(),
                })?;
            let suffix = table.entry_suffix();

            // A field settable two ways is a field with two answers. Refuse rather than pick.
            if self
                .rows
                .iter()
                .any(|r| r.table == suffix && r.id.unwrap_or(self.target_id) == key.id)
            {
                return Err(CraftPatchError::EditTargetIsInserted {
                    node: edit.node.clone(),
                });
            }
            if !row_exists(suffix, key.id) {
                return Err(CraftPatchError::EditTargetNotFound {
                    node: edit.node.clone(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(hash: &str) -> DocumentSource {
        DocumentSource {
            regulation_sha256: Some(hash.to_string()),
            paramdex_fingerprint: None,
        }
    }

    fn row(table: &str, clone_from: i64) -> CraftRow {
        CraftRow {
            table: table.to_string(),
            clone_from,
            id: None,
            overrides: BTreeMap::new(),
        }
    }

    fn patch(hash: &str) -> CraftPatch {
        CraftPatch::new(source(hash), 4002, vec![row("Magic.param", 4000)])
    }

    fn shell_only(_table: &str, id: i64) -> bool {
        id == 4000
    }

    #[test]
    fn a_patch_does_something_if_it_has_rows_or_edits_or_text() {
        let empty = CraftPatch::new(source("abc123"), 4002, vec![]);
        assert!(empty.is_noop());

        let mut attach_only = CraftPatch::new(source("abc123"), 4002, vec![]);
        attach_only.edits = vec![edit("Magic:4002", "refId1")];
        assert!(!attach_only.is_noop(), "an attach creates no rows but is a craft");

        let mut name_only = CraftPatch::new(source("abc123"), 4002, vec![]);
        name_only.name = Some("Remade Pebble".to_string());
        assert!(!name_only.is_noop(), "naming a spell is a change");

        assert!(!patch("abc123").is_noop(), "a patch with rows is obviously not a no-op");
    }

    #[test]
    fn a_note_alone_is_still_a_noop() {
        let mut only_note = CraftPatch::new(source("abc123"), 4002, vec![]);
        only_note.note = Some("thinking out loud".to_string());
        assert!(only_note.is_noop());
    }

    fn edit(node: &str, field: &str) -> FieldEdit {
        FieldEdit {
            node: node.to_string(),
            field: field.to_string(),
            value: DocValue::Int(400200000),
            expected: None,
        }
    }

    #[test]
    fn an_edit_to_an_existing_row_is_allowed() {
        let mut p = CraftPatch::new(
            source("abc123"),
            4002,
            vec![CraftRow {
                table: "Bullet.param".to_string(),
                clone_from: 10400000,
                id: Some(400200000),
                overrides: BTreeMap::new(),
            }],
        );
        p.edits = vec![edit("Magic:4002", "refId1")];
        // 4002 and the clone source exist; the inserted bullet id does not.
        let exists = |_t: &str, id: i64| id == 4002 || id == 10400000;
        assert!(p.check_applicable("abc123", exists).is_ok());
    }

    #[test]
    fn an_edit_to_a_missing_row_is_refused() {
        let mut p = patch("abc123");
        p.edits = vec![edit("Magic:9999", "refId1")];
        assert!(matches!(
            p.check_applicable("abc123", shell_only),
            Err(CraftPatchError::EditTargetNotFound { .. })
        ));
    }

    #[test]
    fn an_edit_to_a_row_this_patch_inserts_is_refused() {
        let mut p = patch("abc123"); // inserts Magic at target_id 4002
        p.edits = vec![edit("Magic:4002", "refId1")];
        assert!(matches!(
            p.check_applicable("abc123", shell_only),
            Err(CraftPatchError::EditTargetIsInserted { .. })
        ));
    }

    #[test]
    fn an_unparseable_edit_node_is_refused() {
        let mut p = patch("abc123");
        p.edits = vec![edit("not-a-node-key", "refId1")];
        assert!(matches!(
            p.check_applicable("abc123", shell_only),
            Err(CraftPatchError::EditNodeUnparseable { .. })
        ));
    }

    #[test]
    fn an_edit_to_a_non_param_node_is_refused() {
        let mut p = patch("abc123");
        p.edits = vec![edit("Fxr:529982", "anything")];
        assert!(matches!(
            p.check_applicable("abc123", shell_only),
            Err(CraftPatchError::EditNodeNotAParamRow { .. })
        ));
    }

    #[test]
    fn json_without_edits_still_parses() {
        let json = r#"{
            "schemaVersion": 1,
            "base": { "regulationSha256": "abc123", "paramdexFingerprint": null },
            "targetId": 4002,
            "note": null,
            "rows": []
        }"#;
        let parsed: CraftPatch = serde_json::from_str(json).expect("should parse");
        assert!(
            parsed.edits.is_empty(),
            "a patch written before `edits` existed must still load"
        );
    }

    #[test]
    fn a_patch_for_this_regulation_applies() {
        assert!(patch("abc123")
            .check_applicable("abc123", shell_only)
            .is_ok());
    }

    #[test]
    fn a_patch_for_another_regulation_is_refused() {
        let err = patch("abc123")
            .check_applicable("def456", shell_only)
            .unwrap_err();
        assert!(matches!(err, CraftPatchError::BaseMismatch { .. }));
        let text = err.to_string();
        assert!(text.contains("abc123") && text.contains("def456"), "{text}");
    }

    #[test]
    fn a_patch_with_no_base_is_refused() {
        let mut p = patch("abc123");
        p.base.regulation_sha256 = None;
        assert!(matches!(
            p.check_applicable("abc123", shell_only).unwrap_err(),
            CraftPatchError::NoBaseRecorded
        ));
    }

    #[test]
    fn a_patch_from_a_future_schema_is_refused() {
        let mut p = patch("abc123");
        p.schema_version = CRAFT_PATCH_SCHEMA_VERSION + 1;
        assert!(matches!(
            p.check_applicable("abc123", shell_only).unwrap_err(),
            CraftPatchError::SchemaMismatch { .. }
        ));
    }

    #[test]
    fn a_row_whose_id_already_exists_is_refused() {
        let p = patch("abc123");
        // Everything "exists", including the target id 4002 — even though no row in the
        // patch sets `id` explicitly, it must resolve to `target_id` for this check.
        let err = p
            .check_applicable("abc123", |_table, _id| true)
            .unwrap_err();
        assert!(matches!(err, CraftPatchError::RowIdTaken { id: 4002, .. }));
    }

    #[test]
    fn a_row_whose_clone_source_is_missing_is_refused() {
        let p = patch("abc123");
        // Nothing exists — the target (4002) is free, which is good, but the clone source
        // (4000) is missing too, which must still be refused.
        let err = p
            .check_applicable("abc123", |_table, _id| false)
            .unwrap_err();
        assert!(matches!(
            err,
            CraftPatchError::CloneSourceNotFound { id: 4000, .. }
        ));
    }

    #[test]
    fn patches_round_trip_through_json() {
        let mut p = patch("abc123");
        p.rows[0]
            .overrides
            .insert("sortId".to_string(), DocValue::Int(4002));
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"schemaVersion\""), "{json}");
        assert!(json.contains("\"targetId\""), "{json}");
        let back: CraftPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.rows.len(), 1);
        assert_eq!(back.rows[0].table, "Magic.param");
    }

    #[test]
    fn json_without_text_fields_still_parses() {
        let json = r#"{
            "schemaVersion": 1,
            "base": { "regulationSha256": "abc123", "paramdexFingerprint": null },
            "targetId": 4002,
            "note": null,
            "rows": []
        }"#;
        let patch: CraftPatch = serde_json::from_str(json).unwrap();
        assert_eq!(patch.name, None);
        assert_eq!(patch.info, None);
        assert_eq!(patch.caption, None);
    }

    #[test]
    fn text_edits_lists_only_the_populated_pools() {
        let mut patch = CraftPatch::new(
            DocumentSource {
                regulation_sha256: None,
                paramdex_fingerprint: None,
            },
            4002,
            vec![],
        );
        assert!(
            patch.text_edits().is_empty(),
            "no text means no msgbnd write"
        );

        patch.name = Some("Gravity Pebble".to_string());
        patch.caption = Some("A crafted sorcery.".to_string());
        let edits = patch.text_edits();
        assert_eq!(
            edits,
            vec![
                ("GoodsName", 4002, "Gravity Pebble"),
                ("GoodsCaption", 4002, "A crafted sorcery."),
            ],
            "info was never set, so it must not appear"
        );
    }

    #[test]
    fn note_is_not_game_text() {
        let mut patch = CraftPatch::new(
            DocumentSource {
                regulation_sha256: None,
                paramdex_fingerprint: None,
            },
            4002,
            vec![],
        );
        patch.note = Some("Glintstone Pebble shell + Gravity Well OnHitMerge".to_string());
        assert!(patch.text_edits().is_empty());
    }
}
