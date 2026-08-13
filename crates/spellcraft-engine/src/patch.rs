// Docs: docs/spellcraft-engine/patch.md

use serde::{Deserialize, Serialize};

use crate::export::{DocValue, DocumentSource};

#[cfg(feature = "ts")]
use ts_rs::TS;

pub const PATCH_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct SpellPatch {
    pub schema_version: u32,
    pub base: DocumentSource,
    pub magic_id: i64,
    pub note: Option<String>,
    pub edits: Vec<FieldEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct FieldEdit {
    pub node: String,
    pub field: String,
    pub value: DocValue,
    pub expected: Option<DocValue>,
}

impl SpellPatch {
    pub fn new(base: DocumentSource, magic_id: i64, edits: Vec<FieldEdit>) -> Self {
        SpellPatch {
            schema_version: PATCH_SCHEMA_VERSION,
            base,
            magic_id,
            note: None,
            edits,
        }
    }

    pub fn check_applicable(&self, regulation_sha256: &str) -> Result<(), PatchError> {
        if self.schema_version != PATCH_SCHEMA_VERSION {
            return Err(PatchError::SchemaMismatch {
                found: self.schema_version,
                expected: PATCH_SCHEMA_VERSION,
            });
        }
        match self.base.regulation_sha256.as_deref() {
            None => Err(PatchError::NoBaseRecorded),
            Some(base) if base != regulation_sha256 => Err(PatchError::BaseMismatch {
                patch: base.to_string(),
                regulation: regulation_sha256.to_string(),
            }),
            Some(_) => Ok(()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("patch schemaVersion {found} != {expected} expected by this build")]
    SchemaMismatch { found: u32, expected: u32 },
    #[error(
        "patch records no base regulation hash, so there is no way to tell whether it was \
         written against this regulation — refusing"
    )]
    NoBaseRecorded,
    #[error(
        "patch was authored against regulation {patch}, but this one is {regulation} — \
         refusing. Re-export the spell and redo the edit against the current regulation."
    )]
    BaseMismatch { patch: String, regulation: String },
    #[error("edit {index}: {detail}")]
    Edit { index: usize, detail: String },
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

    fn patch(hash: &str) -> SpellPatch {
        SpellPatch::new(source(hash), 4000, vec![])
    }

    #[test]
    fn a_patch_for_this_regulation_applies() {
        assert!(patch("abc123").check_applicable("abc123").is_ok());
    }

    #[test]
    fn a_patch_for_another_regulation_is_refused() {
        let err = patch("abc123").check_applicable("def456").unwrap_err();
        assert!(matches!(err, PatchError::BaseMismatch { .. }));
        // The message must name both, or the user cannot tell which install is which.
        let text = err.to_string();
        assert!(text.contains("abc123") && text.contains("def456"), "{text}");
    }

    #[test]
    fn a_patch_with_no_base_is_refused() {
        let mut p = patch("abc123");
        p.base.regulation_sha256 = None;
        assert!(matches!(
            p.check_applicable("abc123").unwrap_err(),
            PatchError::NoBaseRecorded
        ));
    }

    #[test]
    fn a_patch_from_a_future_schema_is_refused() {
        let mut p = patch("abc123");
        p.schema_version = PATCH_SCHEMA_VERSION + 1;
        assert!(matches!(
            p.check_applicable("abc123").unwrap_err(),
            PatchError::SchemaMismatch { .. }
        ));
    }

    #[test]
    fn patches_round_trip_through_json() {
        let p = SpellPatch::new(
            source("abc123"),
            4000,
            vec![FieldEdit {
                node: "Bullet:10400000".into(),
                field: "life".into(),
                value: DocValue::Float(3.5),
                expected: Some(DocValue::Float(2.0)),
            }],
        );
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"schemaVersion\""), "{json}");
        let back: SpellPatch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.edits.len(), 1);
        assert_eq!(back.edits[0].node, "Bullet:10400000");
    }
}
