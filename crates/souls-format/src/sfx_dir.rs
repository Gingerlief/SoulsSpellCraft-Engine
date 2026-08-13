// Docs: docs/souls-format/sfx_dir.md

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::fxr::Fxr;

#[derive(Debug, Clone)]
pub enum FxrResult {
    Found(Arc<Fxr>),
    Missing,
    Unparseable {
        error: String,
    },
}

impl FxrResult {
    pub fn found(&self) -> Option<&Arc<Fxr>> {
        match self {
            FxrResult::Found(f) => Some(f),
            _ => None,
        }
    }
}

pub fn fxr_file_name(sfx_id: i32) -> String {
    format!("f{sfx_id:09}.fxr")
}

pub struct SfxDirectory {
    dirs: Vec<PathBuf>,
    cache: Mutex<HashMap<i32, FxrResult>>,
}

impl SfxDirectory {
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        SfxDirectory {
            dirs,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn open_located() -> Option<Self> {
        let dirs = crate::locate::locate_sfx_dirs();
        (!dirs.is_empty()).then(|| Self::new(dirs))
    }

    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    pub fn get(&self, sfx_id: i32) -> FxrResult {
        if let Some(hit) = self
            .cache
            .lock()
            .expect("sfx cache mutex poisoned")
            .get(&sfx_id)
        {
            return hit.clone();
        }

        let result = self.load_uncached(sfx_id);
        self.cache
            .lock()
            .expect("sfx cache mutex poisoned")
            .insert(sfx_id, result.clone());
        result
    }

    fn load_uncached(&self, sfx_id: i32) -> FxrResult {
        let file_name = fxr_file_name(sfx_id);
        for dir in &self.dirs {
            let path = dir.join(&file_name);
            if !path.is_file() {
                continue;
            }
            return match std::fs::read(&path) {
                Ok(bytes) => match Fxr::parse(&bytes) {
                    Ok(fxr) => FxrResult::Found(Arc::new(fxr)),
                    Err(e) => FxrResult::Unparseable {
                        error: format!("{}: {e}", path.display()),
                    },
                },
                Err(e) => FxrResult::Unparseable {
                    error: format!("{}: {e}", path.display()),
                },
            };
        }
        FxrResult::Missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/reference/fxr-samples")
    }

    #[test]
    fn file_name_is_nine_zero_padded_digits() {
        assert_eq!(fxr_file_name(529982), "f000529982.fxr");
        assert_eq!(fxr_file_name(1), "f000000001.fxr");
        assert_eq!(fxr_file_name(523002), "f000523002.fxr");
    }

    #[test]
    fn resolves_vendored_samples_and_reports_misses() {
        let sfx = SfxDirectory::new(vec![samples_dir()]);

        let found = sfx.get(529982);
        let fxr = found.found().expect("vendored sample should resolve");
        assert_eq!(fxr.id, 529982);
        assert_eq!(fxr.proxy_targets(), vec![529972]);

        // An id with no vendored file is Missing, not an error.
        assert!(matches!(sfx.get(999_999_999), FxrResult::Missing));
    }

    #[test]
    fn lookups_are_memoized() {
        let sfx = SfxDirectory::new(vec![samples_dir()]);
        let a = sfx.get(529972);
        let b = sfx.get(529972);
        match (a, b) {
            (FxrResult::Found(x), FxrResult::Found(y)) => {
                assert!(
                    Arc::ptr_eq(&x, &y),
                    "second lookup should return the cached Arc"
                )
            }
            other => panic!("expected both lookups to resolve, got {other:?}"),
        }
    }
}
