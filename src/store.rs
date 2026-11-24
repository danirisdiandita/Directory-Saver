use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use rusqlite::{Connection, Row, params, params_from_iter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    pub id: i64,
    pub path: PathBuf,
    pub name: String,
    pub added_at: i64,
    pub last_opened_at: Option<i64>,
    pub open_count: i64,
}

pub struct Added {
    pub dir: Directory,
    pub is_new: bool,
}

pub struct Store {
    conn: Connection,
}

const MIGRATIONS: &[&str] = &["CREATE TABLE directories (
        id INTEGER PRIMARY KEY,
        path TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        added_at INTEGER NOT NULL,
        last_opened_at INTEGER,
        open_count INTEGER NOT NULL DEFAULT 0
    );"];

const COLUMNS: &str = "id, path, name, added_at, last_opened_at, open_count";

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Store> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let tx = conn.transaction()?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        }
        tx.commit()?;
        Ok(Store { conn })
    }

    pub fn add(&self, path: &Path) -> Result<Added> {
        let path = path
            .canonicalize()
            .with_context(|| format!("{} is not accessible", path.display()))?;
        anyhow::ensure!(path.is_dir(), "{} is not a directory", path.display());
        let path_str = path
            .to_str()
            .with_context(|| format!("{} is not valid UTF-8", path.display()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_str.to_owned());
        let inserted = self.conn.execute(
            "INSERT INTO directories (path, name, added_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO NOTHING",
            params![path_str, name, now()],
        )?;
        let dir = self.conn.query_row(
            &format!("SELECT {COLUMNS} FROM directories WHERE path = ?1"),
            [path_str],
            directory_from_row,
        )?;
        Ok(Added { dir, is_new: inserted > 0 })
    }

    pub fn search(&self, query: &str) -> Result<Vec<Directory>> {
        let tokens: Vec<String> = query.split_whitespace().map(escape_like).collect();
        let filter = if tokens.is_empty() {
            "1".to_owned()
        } else {
            (1..=tokens.len())
                .map(|i| format!("(name LIKE '%' || ?{i} || '%' ESCAPE '\\' OR path LIKE '%' || ?{i} || '%' ESCAPE '\\')"))
                .collect::<Vec<_>>()
                .join(" AND ")
        };
        let prefix_rank = if tokens.is_empty() {
            ""
        } else {
            "CASE WHEN name LIKE ?1 || '%' ESCAPE '\\' THEN 0 ELSE 1 END,"
        };
        let sql = format!(
            "SELECT {COLUMNS} FROM directories WHERE {filter}
             ORDER BY {prefix_rank} last_opened_at DESC NULLS LAST, added_at DESC, id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(tokens), directory_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn mark_opened(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE directories SET last_opened_at = ?1, open_count = open_count + 1 WHERE id = ?2",
            params![now(), id],
        )?;
        Ok(())
    }

    pub fn remove(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM directories WHERE id = ?1", [id])?;
        Ok(())
    }
}

fn directory_from_row(row: &Row) -> rusqlite::Result<Directory> {
    Ok(Directory {
        id: row.get(0)?,
        path: PathBuf::from(row.get::<_, String>(1)?),
        name: row.get(2)?,
        added_at: row.get(3)?,
        last_opened_at: row.get(4)?,
        open_count: row.get(5)?,
    })
}

fn escape_like(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    for c in token.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn dirs(root: &TempDir, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|n| {
                let p = root.path().join(n);
                std::fs::create_dir_all(&p).unwrap();
                p
            })
            .collect()
    }

    fn names(results: &[Directory]) -> Vec<&str> {
        results.iter().map(|d| d.name.as_str()).collect()
    }

    #[test]
    fn re_adding_returns_the_existing_entry() {
        let store = Store::open_in_memory().unwrap();
        let root = TempDir::new().unwrap();
        let [p] = dirs(&root, &["proj"]).try_into().unwrap();

        let first = store.add(&p).unwrap();
        let again = store.add(&p.join("..").join("proj")).unwrap();

        assert!(first.is_new);
        assert!(!again.is_new);
        let (first, again) = (first.dir, again.dir);
        assert_eq!(first, again);
        assert_eq!(store.search("").unwrap().len(), 1);
        assert_eq!(first.name, "proj");
    }

    #[test]
    fn adding_a_missing_path_fails() {
        let store = Store::open_in_memory().unwrap();
        let root = TempDir::new().unwrap();
        assert!(store.add(&root.path().join("nope")).is_err());
    }

    #[test]
    fn every_token_must_match_name_or_path() {
        let store = Store::open_in_memory().unwrap();
        let root = TempDir::new().unwrap();
        for p in dirs(&root, &["work/api", "work/web", "play/api"]) {
            store.add(&p).unwrap();
        }

        assert_eq!(names(&store.search("WORK api").unwrap()), ["api"]);
        let hits = store.search("work api").unwrap();
        assert!(hits[0].path.ends_with("work/api"));
        assert_eq!(store.search("api").unwrap().len(), 2);
        assert!(store.search("play web").unwrap().is_empty());
    }

    #[test]
    fn like_wildcards_in_query_are_literal() {
        let store = Store::open_in_memory().unwrap();
        let root = TempDir::new().unwrap();
        for p in dirs(&root, &["axb", "a_b", "100%", "back\\slash"]) {
            store.add(&p).unwrap();
        }

        assert_eq!(names(&store.search("a_b").unwrap()), ["a_b"]);
        assert_eq!(names(&store.search("0%").unwrap()), ["100%"]);
        assert_eq!(names(&store.search("k\\s").unwrap()), ["back\\slash"]);
        assert!(store.search("'").unwrap().is_empty());
    }

    #[test]
    fn recently_opened_comes_first_and_name_prefix_wins() {
        let store = Store::open_in_memory().unwrap();
        let root = TempDir::new().unwrap();
        let ps = dirs(&root, &["alpha", "beta", "gamma"]);
        let added: Vec<_> = ps.iter().map(|p| store.add(p).unwrap().dir).collect();

        assert_eq!(names(&store.search("").unwrap()), ["gamma", "beta", "alpha"]);

        store.mark_opened(added[0].id).unwrap();
        let all = store.search("").unwrap();
        assert_eq!(names(&all), ["alpha", "gamma", "beta"]);
        assert_eq!(all[0].open_count, 1);
        assert!(all[0].last_opened_at.is_some());

        assert_eq!(names(&store.search("b").unwrap())[0], "beta");
    }

    #[test]
    fn remove_deletes_the_entry() {
        let store = Store::open_in_memory().unwrap();
        let root = TempDir::new().unwrap();
        let ps = dirs(&root, &["keep", "drop"]);
        let keep = store.add(&ps[0]).unwrap().dir;
        let drop = store.add(&ps[1]).unwrap().dir;

        store.remove(drop.id).unwrap();

        assert_eq!(store.search("").unwrap(), [keep]);
    }

    #[test]
    fn data_survives_reopening_the_file() {
        let root = TempDir::new().unwrap();
        let db = root.path().join("nested/directories.db");
        let [p] = dirs(&root, &["proj"]).try_into().unwrap();
        Store::open(&db).unwrap().add(&p).unwrap();

        assert_eq!(names(&Store::open(&db).unwrap().search("").unwrap()), ["proj"]);
    }
}
