//! SQLite-Index (abgeleitet, löschbar): Schema, Öffnen, Sperrverhalten.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, params};

use crate::error::{Error, Result};
use crate::fsutil::{compare_key, is_within};
use crate::vault::{OpenOptions, Vault};

/// Bei Schemaänderung erhöhen: der Index wird dann automatisch neu aufgebaut.
pub(crate) const SCHEMA_VERSION: i64 = 2;
pub(crate) const DB_FILE: &str = "index.db";

pub(crate) struct Db {
    pub conn: Connection,
    #[allow(dead_code)]
    pub path: PathBuf,
    /// Schema war neu oder wurde verworfen → nächster Sync ist ein Vollaufbau.
    pub fresh: std::cell::Cell<bool>,
    /// Befunde aller Regeln ohne Filter, gültig für den Indexstand im Schlüssel
    /// (`PRAGMA data_version` für fremde Schreiber, `total_changes` für eigene).
    pub findings: std::cell::RefCell<Option<FindingsCache>>,
}

/// Schlüssel (data_version, total_changes) und die Befunde zu diesem Stand.
pub(crate) type FindingsCache = ((i64, u64), std::rc::Rc<Vec<crate::model::Finding>>);

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS notes(
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  path_key TEXT NOT NULL,
  stem_key TEXT NOT NULL,
  fs_path TEXT NOT NULL,
  size INTEGER NOT NULL,
  mtime_ns INTEGER NOT NULL,
  hash BLOB NOT NULL,
  title TEXT NOT NULL,
  ntype TEXT NOT NULL,
  type_raw TEXT,
  project TEXT,
  status TEXT,
  state TEXT,
  checked_on TEXT,
  summary TEXT,
  aliases TEXT NOT NULL,
  paths TEXT NOT NULL,
  inferred_paths TEXT NOT NULL,
  lines INTEGER NOT NULL,
  tokens INTEGER NOT NULL,
  fm_problems TEXT NOT NULL,
  flags TEXT NOT NULL,
  is_journal INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS notes_key ON notes(path_key);
CREATE INDEX IF NOT EXISTS notes_stem ON notes(stem_key);
CREATE INDEX IF NOT EXISTS notes_project ON notes(project);
CREATE TABLE IF NOT EXISTS sections(
  id INTEGER PRIMARY KEY,
  note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  idx INTEGER NOT NULL,
  level INTEGER NOT NULL,
  title TEXT NOT NULL,
  kind TEXT NOT NULL,
  line_start INTEGER NOT NULL,
  line_end INTEGER NOT NULL,
  own_end INTEGER NOT NULL,
  tokens INTEGER NOT NULL,
  own_tokens INTEGER NOT NULL,
  anchor TEXT NOT NULL,
  body TEXT NOT NULL,
  list_items INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sections_note ON sections(note_id);
CREATE TABLE IF NOT EXISTS links(
  id INTEGER PRIMARY KEY,
  src INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  line INTEGER NOT NULL,
  target TEXT NOT NULL,
  heading TEXT,
  alias TEXT,
  dst INTEGER,
  status TEXT NOT NULL,
  candidates TEXT
);
CREATE INDEX IF NOT EXISTS links_src ON links(src);
CREATE INDEX IF NOT EXISTS links_dst ON links(dst);
CREATE TABLE IF NOT EXISTS journal(
  id INTEGER PRIMARY KEY,
  note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  line INTEGER NOT NULL,
  date TEXT NOT NULL,
  time TEXT,
  project TEXT,
  target TEXT,
  target_key TEXT,
  text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS journal_date ON journal(date);
CREATE INDEX IF NOT EXISTS journal_project ON journal(project);
CREATE TABLE IF NOT EXISTS secrets(
  note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  line INTEGER NOT NULL,
  class TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS secrets_note ON secrets(note_id);
CREATE VIRTUAL TABLE IF NOT EXISTS sec_fts USING fts5(title, heading, aliases, path, body, tokenize = "unicode61 remove_diacritics 0");
CREATE VIRTUAL TABLE IF NOT EXISTS sec_tri USING fts5(text, tokenize = "trigram");
CREATE VIRTUAL TABLE IF NOT EXISTS sec_vocab USING fts5vocab(sec_fts, 'row');
"#;

const DROP: &str = r#"
DROP TABLE IF EXISTS sec_vocab; DROP TABLE IF EXISTS sec_fts; DROP TABLE IF EXISTS sec_tri;
DROP TABLE IF EXISTS secrets; DROP TABLE IF EXISTS journal; DROP TABLE IF EXISTS links; DROP TABLE IF EXISTS sections;
DROP TABLE IF EXISTS notes; DROP TABLE IF EXISTS meta;
"#;

pub(crate) fn open_vault(opts: OpenOptions) -> Result<Vault> {
    let root = std::path::absolute(&opts.root).map_err(|e| Error::io(&opts.root, e))?;
    if !root.is_dir() {
        return Err(Error::Vault {
            message: format!("Vault-Ordner nicht gefunden: {}", root.display()),
            hint: Some("--vault <Ordner>, WB_VAULT oder `vault = \"…\"` in %APPDATA%\\wb\\config.toml".into()),
        });
    }
    let explicit = opts.index_dir.is_some();
    let index_dir = match &opts.index_dir {
        Some(d) => std::path::absolute(d).map_err(|e| Error::io(d, e))?,
        None => root.join(".wb"),
    };
    if opts.read_only && is_within(&index_dir, &root) {
        return Err(Error::ReadOnly {
            action: format!("Index im Vault ({})", index_dir.display()),
            hint: Some("--index <Ordner außerhalb des Vaults> angeben".into()),
        });
    }
    if !index_dir.is_dir() {
        if opts.create_index || explicit {
            std::fs::create_dir_all(&index_dir).map_err(|e| Error::io(&index_dir, e))?;
        } else {
            return Err(Error::index(
                format!("kein Index unter {}", index_dir.display()),
                "`wb sync --init` legt <vault>/.wb an, oder --index <Ordner> angeben",
            ));
        }
    }
    let db_path = index_dir.join(DB_FILE);
    // Unterbrochenes `wb mv` abschließen oder zurücknehmen, bevor jemand den halben Stand liest.
    // Gelingt beides nicht, laufen Lesebefehle weiter und nennen es in einer `note:`-Zeile (§19.1).
    let pending_problem = if opts.read_only { None } else { crate::write::recover_at_open(&root) };
    let db = match open_db(&db_path, &root) {
        Err(Error::Sqlite(e)) if opts.discard_broken && is_broken(&e) => {
            // Beschädigte Indexdatei verwerfen (nur unsere Dateien im Indexordner) und neu anlegen.
            for suffix in ["", "-wal", "-shm"] {
                let f = index_dir.join(format!("{DB_FILE}{suffix}"));
                if f.exists() {
                    std::fs::remove_file(&f).map_err(|e| Error::io(&f, e))?;
                }
            }
            open_db(&db_path, &root)?
        }
        r => r?,
    };
    Ok(Vault { root, index_dir, read_only: opts.read_only, db, pending_problem })
}

fn is_broken(e: &rusqlite::Error) -> bool {
    matches!(e.sqlite_error_code(), Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt))
}

/// Tabellen, ohne die der Index nicht taugt; fehlt eine, wird das Schema neu angelegt.
const TABLES: [&str; 10] = ["meta", "notes", "sections", "links", "journal", "secrets", "sec_fts", "sec_tri", "sec_vocab", "sec_fts_data"];

fn open_db(path: &Path, root: &Path) -> Result<Db> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_secs(30))?;
    // WAL: Leser blockieren Schreiber nicht; Schreiber warten (busy_timeout) statt zu scheitern.
    // Zwei Prozesse, die eine frische Datei gleichzeitig auf WAL umstellen, halten beide SHARED und
    // wollen EXCLUSIVE; SQLite meldet das als BUSY sofort, ohne busy_timeout. Daher selbst wiederholen.
    let t0 = std::time::Instant::now();
    loop {
        match conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)) {
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::DatabaseBusy && t0.elapsed() < Duration::from_secs(30) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            r => {
                r?;
                break;
            }
        }
    }
    conn.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;")?;
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version == SCHEMA_VERSION {
        let n: i64 = conn.query_row(
            &format!("SELECT count(*) FROM sqlite_master WHERE name IN ({})", TABLES.map(|t| format!("'{t}'")).join(",")),
            [],
            |r| r.get(0),
        )?;
        if n != TABLES.len() as i64 {
            version = -1; // Schema unvollständig: wie eine fremde Version behandeln
        }
    }
    let root_key = compare_key(root);
    let fresh;
    if version != SCHEMA_VERSION {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let v2: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v2 != SCHEMA_VERSION || version == -1 {
            conn.execute_batch(DROP)?;
            conn.execute_batch(SCHEMA)?;
            conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES('root', ?1)", params![root_key])?;
            conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
            fresh = true;
        } else {
            fresh = false;
        }
        conn.execute_batch("COMMIT")?;
    } else {
        fresh = false;
        let stored: Option<String> =
            conn.query_row("SELECT value FROM meta WHERE key='root'", [], |r| r.get(0)).ok();
        if let Some(s) = stored
            && s != root_key {
                return Err(Error::index(
                    format!("Index {} gehört zu einem anderen Vault ({s})", path.display()),
                    "anderen --index wählen oder den Indexordner löschen",
                ));
            }
    }
    Ok(Db { conn, path: path.to_path_buf(), fresh: std::cell::Cell::new(fresh), findings: Default::default() })
}

/// Listen im Index werden mit dem Steuerzeichen U+001F verbunden.
pub(crate) const SEP: char = '\u{1f}';

pub(crate) fn join_list(v: &[String]) -> String {
    v.join(&SEP.to_string())
}

pub(crate) fn split_list(s: &str) -> Vec<String> {
    if s.is_empty() { Vec::new() } else { s.split(SEP).map(str::to_string).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_opens_with_fts5_trigram_vocab() {
        let d = tempfile::tempdir().unwrap();
        let v = open_vault(OpenOptions::new(d.path()).create_index(true)).unwrap();
        v.db.conn.execute("INSERT INTO sec_tri(rowid, text) VALUES (1, 'kartenerzeugung mit gitter')", []).unwrap();
        let n: i64 = v.db.conn.query_row("SELECT count(*) FROM sec_tri WHERE sec_tri MATCH '\"erzeug\"'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let ver: String = v.db.conn.query_row("SELECT sqlite_version()", [], |r| r.get(0)).unwrap();
        assert!(ver.starts_with('3'));
    }

    #[test]
    fn read_only_refuses_index_inside_vault() {
        let d = tempfile::tempdir().unwrap();
        let e = open_vault(OpenOptions::new(d.path()).read_only(true).create_index(true)).err().unwrap();
        assert_eq!(e.code(), "read-only");
        assert_eq!(e.exit_code(), crate::exit::READ_ONLY);
        assert!(!d.path().join(".wb").exists());
        // ohne --init wird <vault>/.wb nicht angelegt
        let e = open_vault(OpenOptions::new(d.path())).err().unwrap();
        assert_eq!(e.code(), "index");
        assert!(!d.path().join(".wb").exists());
    }
}
