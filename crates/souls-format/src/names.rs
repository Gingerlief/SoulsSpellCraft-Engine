// Docs: docs/souls-format/names.md

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum NameError {
    #[error("failed to read name list '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

fn parse_list(text: &str) -> HashMap<i64, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        let Some((id_str, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(id) = id_str.trim().parse::<i64>() else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() {
            out.insert(id, name.to_string());
        }
    }
    out
}

pub struct NameIndex {
    dir: PathBuf,
    lists: Mutex<HashMap<String, HashMap<i64, String>>>,
}

impl NameIndex {
    pub fn new(dir: PathBuf) -> Self {
        NameIndex {
            dir,
            lists: Mutex::new(HashMap::new()),
        }
    }

    pub fn open_vendored() -> Option<Self> {
        let dir = crate::locate::locate_paramdex_defs()?
            .parent()?
            .join("Names");
        dir.is_dir().then(|| Self::new(dir))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn get(&self, list_stem: &str, id: i64) -> Option<String> {
        let mut lists = self.lists.lock().expect("name index mutex poisoned");
        let list = lists.entry(list_stem.to_string()).or_insert_with(|| {
            std::fs::read_to_string(self.dir.join(format!("{list_stem}.txt")))
                .map(|t| parse_list(&t))
                // A missing list memoizes as empty, so we don't retry the read per lookup.
                .unwrap_or_default()
        });
        list.get(&id).cloned()
    }

    pub fn len(&self, list_stem: &str) -> usize {
        self.get(list_stem, i64::MIN);
        self.lists
            .lock()
            .expect("name index mutex poisoned")
            .get(list_stem)
            .map(HashMap::len)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ids_names_and_category_tags() {
        let list = parse_list(
            "4000 [Sorcery] Glintstone Pebble\n\
             0 Smithscript Spear - Bullet \n\
             110 Impact\n\
             not-a-number whatever\n\
             \n",
        );
        assert_eq!(list.get(&4000).unwrap(), "[Sorcery] Glintstone Pebble");
        // Trailing whitespace is real in the vendored lists.
        assert_eq!(list.get(&0).unwrap(), "Smithscript Spear - Bullet");
        assert_eq!(list.get(&110).unwrap(), "Impact");
        // Malformed lines are skipped, not fatal.
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn resolves_real_vendored_names() {
        let names = NameIndex::open_vendored().expect("vendored names should be present");
        assert_eq!(
            names.get("Magic", 4000).as_deref(),
            Some("[Sorcery] Glintstone Pebble")
        );
        assert_eq!(
            names.get("Magic", 4431).as_deref(),
            Some("[Sorcery] Adula's Moonblade")
        );
        assert_eq!(
            names.get("EquipParamGoods", 4000).as_deref(),
            Some("[Sorcery] Glintstone Pebble")
        );
        // Magic.txt has one line per Magic row in the regulation.
        assert_eq!(names.len("Magic"), 317);
    }

    #[test]
    fn missing_list_or_id_is_none_not_an_error() {
        let names = NameIndex::open_vendored().unwrap();
        assert_eq!(names.get("NoSuchList", 1), None);
        assert_eq!(names.get("Magic", 99_999_999), None);
    }
}
