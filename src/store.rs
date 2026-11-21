//! Ce qu'un agent garde sur disque : une petite base SQLite, dans son dossier d'état.
//!
//! Chaque agent a la sienne. Elle contient sa mémoire (des notes qu'il tient lui-même) et,
//! chez le maître, le journal de la grappe. Les outils de fichiers et de commandes n'y ont
//! pas accès : le dossier d'état leur est interdit.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::protocol::ActivityEvent;

/// Nombre de notes au plus dans la mémoire d'un agent.
pub const MAX_NOTES: usize = 200;
/// Longueur maximale d'une note, en caractères.
pub const MAX_NOTE_CHARS: usize = 20_000;
/// Événements du journal gardés sur disque.
const JOURNAL_KEPT: i64 = 200_000;

pub struct Store {
    db: Mutex<Connection>,
}

fn err(e: rusqlite::Error) -> String {
    format!("base locale : {e}")
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let db = Connection::open(path).map_err(|e| format!("{} : {e}", path.display()))?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS memory (
                 key TEXT PRIMARY KEY, content TEXT NOT NULL, updated_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS journal (
                 id INTEGER PRIMARY KEY, agent_id TEXT NOT NULL, agent_name TEXT NOT NULL,
                 ts_ms INTEGER NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL
             );",
        )
        .map_err(err)?;
        let _ = crate::os::make_private(path);
        Ok(Self { db: Mutex::new(db) })
    }

    /// Écrit une note, ou l'efface si le contenu est vide.
    pub fn remember(&self, key: &str, content: &str) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() || key.chars().count() > 100 {
            return Err("clé de note invalide : 1 à 100 caractères".into());
        }
        if content.chars().count() > MAX_NOTE_CHARS {
            return Err(format!("note trop longue ({MAX_NOTE_CHARS} caractères au plus)"));
        }
        let db = self.db.lock().unwrap();
        if content.trim().is_empty() {
            db.execute("DELETE FROM memory WHERE key = ?1", params![key]).map_err(err)?;
            return Ok(());
        }
        let known: i64 = db.query_row("SELECT COUNT(*) FROM memory WHERE key = ?1", params![key], |r| r.get(0)).map_err(err)?;
        let total: i64 = db.query_row("SELECT COUNT(*) FROM memory", [], |r| r.get(0)).map_err(err)?;
        if known == 0 && total as usize >= MAX_NOTES {
            return Err(format!("mémoire pleine ({MAX_NOTES} notes) : efface une note d'abord"));
        }
        db.execute(
            "INSERT INTO memory (key, content, updated_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET content = ?2, updated_ms = ?3",
            params![key, content, crate::now_ms() as i64],
        )
        .map_err(err)?;
        Ok(())
    }

    /// Toutes les notes, par clé.
    pub fn notes(&self) -> Result<Vec<(String, String)>, String> {
        let db = self.db.lock().unwrap();
        let mut query = db.prepare("SELECT key, content FROM memory ORDER BY key").map_err(err)?;
        let rows = query.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    /// Inscrit un événement du journal. Une erreur d'écriture ne doit pas arrêter la grappe.
    pub fn log(&self, event: &ActivityEvent) {
        let db = self.db.lock().unwrap();
        let written = db.execute(
            "INSERT OR REPLACE INTO journal (id, agent_id, agent_name, ts_ms, kind, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![event.id as i64, event.agent_id, event.agent_name, event.ts_ms as i64, event.kind, event.text],
        );
        if let Err(e) = written {
            eprintln!("journal non écrit sur disque : {e}");
        }
    }

    /// Les derniers événements du journal, du plus ancien au plus récent. Le surplus ancien est effacé.
    pub fn recent(&self, limit: usize) -> Vec<ActivityEvent> {
        let db = self.db.lock().unwrap();
        let _ = db.execute("DELETE FROM journal WHERE id <= (SELECT MAX(id) FROM journal) - ?1", params![JOURNAL_KEPT]);
        let Ok(mut query) =
            db.prepare("SELECT id, agent_id, agent_name, ts_ms, kind, text FROM journal ORDER BY id DESC LIMIT ?1")
        else {
            return Vec::new();
        };
        let rows = query.query_map(params![limit as i64], |r| {
            Ok(ActivityEvent {
                id: r.get::<_, i64>(0)? as u64,
                agent_id: r.get(1)?,
                agent_name: r.get(2)?,
                ts_ms: r.get::<_, i64>(3)? as u64,
                kind: r.get(4)?,
                text: r.get(5)?,
            })
        });
        let mut events: Vec<ActivityEvent> = rows.map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default();
        events.reverse();
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_and_journal_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.db");
        {
            let store = Store::open(&path).unwrap();
            store.remember("serveur", "nginx sur le port 8080").unwrap();
            store.remember("à effacer", "x").unwrap();
            store.remember("à effacer", "").unwrap();
            assert!(store.remember("", "x").is_err());
            assert!(store.remember("k", &"x".repeat(MAX_NOTE_CHARS + 1)).is_err());
            for id in 1..=3 {
                store.log(&ActivityEvent { id, agent_id: "a".into(), agent_name: "n".into(), ts_ms: 0, kind: "message".into(), text: format!("e{id}") });
            }
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.notes().unwrap(), vec![("serveur".to_string(), "nginx sur le port 8080".to_string())]);
        let events = store.recent(2);
        assert_eq!(events.iter().map(|e| e.id).collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn memory_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("agent.db")).unwrap();
        for i in 0..MAX_NOTES {
            store.remember(&format!("k{i}"), "v").unwrap();
        }
        assert!(store.remember("de trop", "v").is_err());
        store.remember("k0", "remplacée").unwrap();
    }
}
