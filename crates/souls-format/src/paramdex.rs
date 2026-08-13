// Docs: docs/souls-format/paramdex.md

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::paramdef::{ParamDefError, Paramdef};

#[derive(Debug, thiserror::Error)]
pub enum ParamdexError {
    #[error("failed to read paramdex directory '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("no PARAMDEF found for param type '{0}'")]
    UnknownParamType(String),
    #[error("duplicate param type '{param_type}' in '{first}' and '{second}'")]
    DuplicateParamType {
        param_type: String,
        first: String,
        second: String,
    },
    #[error(transparent)]
    ParamDef(#[from] ParamDefError),
}

pub struct ParamdefLibrary {
    by_param_type: HashMap<String, PathBuf>,
    parsed: Mutex<HashMap<String, &'static Paramdef>>,
}

fn extract_param_type(xml: &str) -> Option<&str> {
    let start = xml.find("<ParamType>")? + "<ParamType>".len();
    let rest = &xml[start..];
    let end = rest.find("</ParamType>")?;
    Some(rest[..end].trim())
}

impl ParamdefLibrary {
    pub fn open(defs_dir: &Path) -> Result<Self, ParamdexError> {
        let entries = std::fs::read_dir(defs_dir).map_err(|source| ParamdexError::Io {
            path: defs_dir.display().to_string(),
            source,
        })?;

        let mut by_param_type: HashMap<String, PathBuf> = HashMap::new();
        for entry in entries {
            let entry = entry.map_err(|source| ParamdexError::Io {
                path: defs_dir.display().to_string(),
                source,
            })?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("xml") {
                continue;
            }

            let text = std::fs::read_to_string(&path).map_err(|source| ParamdexError::Io {
                path: path.display().to_string(),
                source,
            })?;
            let Some(param_type) = extract_param_type(&text) else {
                continue; // not a PARAMDEF; skip rather than fail the whole index
            };

            if let Some(first) = by_param_type.get(param_type) {
                return Err(ParamdexError::DuplicateParamType {
                    param_type: param_type.to_string(),
                    first: first.display().to_string(),
                    second: path.display().to_string(),
                });
            }
            by_param_type.insert(param_type.to_string(), path);
        }

        Ok(ParamdefLibrary {
            by_param_type,
            parsed: Mutex::new(HashMap::new()),
        })
    }

    pub fn open_vendored() -> Result<Self, ParamdexError> {
        let dir = crate::locate::locate_paramdex_defs().ok_or_else(|| ParamdexError::Io {
            path: "<vendored data/paramdex/ER/Defs>".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "paramdex defs not found"),
        })?;
        Self::open(&dir)
    }

    pub fn param_types(&self) -> impl Iterator<Item = &str> {
        self.by_param_type.keys().map(String::as_str)
    }

    pub fn path_for(&self, param_type: &str) -> Option<&Path> {
        self.by_param_type.get(param_type).map(PathBuf::as_path)
    }

    pub fn by_param_type(&self, param_type: &str) -> Result<&'static Paramdef, ParamdexError> {
        let mut parsed = self.parsed.lock().expect("paramdef cache mutex poisoned");
        if let Some(def) = parsed.get(param_type) {
            return Ok(def);
        }

        let path = self
            .by_param_type
            .get(param_type)
            .ok_or_else(|| ParamdexError::UnknownParamType(param_type.to_string()))?;
        let def: &'static Paramdef = Box::leak(Box::new(Paramdef::load(path)?));
        parsed.insert(param_type.to_string(), def);
        Ok(def)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> ParamdefLibrary {
        ParamdefLibrary::open_vendored().expect("vendored paramdex should open")
    }

    #[test]
    fn indexes_all_vendored_defs_by_param_type() {
        let lib = library();
        // 194 vendored XMLs; every one declares a ParamType, and `open` errors on
        // duplicates, so a successful open already proves uniqueness.
        assert_eq!(lib.param_types().count(), 194);
    }

    #[test]
    fn resolves_the_irregular_filename_mappings() {
        let lib = library();
        // The whole reason this type exists: none of these could be derived from the
        // regulation.bin entry name by string munging.
        for (param_type, expected_file) in [
            ("MAGIC_PARAM_ST", "MagicParam.xml"),
            ("BULLET_PARAM_ST", "BulletParam.xml"),
            ("ATK_PARAM_ST", "AtkParam.xml"),
            ("SP_EFFECT_PARAM_ST", "SpEffect.xml"),
            ("SP_EFFECT_VFX_PARAM_ST", "SpEffectVfx.xml"),
        ] {
            let path = lib
                .path_for(param_type)
                .unwrap_or_else(|| panic!("{param_type} should be indexed"));
            assert_eq!(
                path.file_name().unwrap().to_str().unwrap(),
                expected_file,
                "{param_type} resolved to the wrong file"
            );
        }
    }

    #[test]
    fn parses_and_memoizes_on_demand() {
        let lib = library();
        let first = lib.by_param_type("MAGIC_PARAM_ST").expect("should parse");
        let second = lib
            .by_param_type("MAGIC_PARAM_ST")
            .expect("should hit cache");
        assert!(
            std::ptr::eq(first, second),
            "second call should return the memoized def"
        );
        assert_eq!(first.param_type, "MAGIC_PARAM_ST");
    }

    #[test]
    fn unknown_param_type_is_an_error() {
        let lib = library();
        assert!(matches!(
            lib.by_param_type("NOT_A_REAL_PARAM_ST"),
            Err(ParamdexError::UnknownParamType(_))
        ));
    }

    #[test]
    fn magic_param_ref_ids_are_not_contiguous() {
        let lib = library();
        let def = lib.by_param_type("MAGIC_PARAM_ST").expect("should parse");
        let index_of = |name: &str| {
            def.fields
                .iter()
                .position(|f| f.internal_name == name)
                .unwrap_or_else(|| panic!("{name} should exist in MAGIC_PARAM_ST"))
        };

        let ref3 = index_of("refId3");
        let ai = index_of("aiUseJudgeId");
        let ref4 = index_of("refId4");
        assert!(
            ref3 < ai && ai < ref4,
            "aiUseJudgeId must sit between refId3 and refId4 (got refId3={ref3}, aiUseJudgeId={ai}, refId4={ref4})"
        );
    }
}
