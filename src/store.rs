//! Ce qu'un agent garde sur disque : une petite base SQLite, dans son dossier d'état.
//!
//! Chaque agent a la sienne. Elle contient sa mémoire (des notes qu'il tient lui-même) et,
//! chez le maître, le journal de la grappe. Les outils de fichiers et de commandes n'y ont
//! pas accès : le dossier d'état leur est interdit.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};

use crate::protocol::{ActivityEvent, NodeInfo};

/// Nombre de notes au plus dans la mémoire d'un agent.
pub const MAX_NOTES: usize = 200;
/// Longueur maximale d'une note, en caractères.
pub const MAX_NOTE_CHARS: usize = 20_000;
/// Événements du journal gardés sur disque.
const JOURNAL_KEPT: i64 = 200_000;
/// Agents gardés dans le répertoire des rôles, en tout.
const ROLES_KEPT: i64 = 4_000;
/// Agents gardés par branche (un enfant direct du maître et sa descendance) : une branche
/// qui déclare beaucoup d'agents n'efface que les siens.
const ROLES_KEPT_PER_BRANCH: i64 = 2_000;
/// Agents rendus par une lecture du répertoire.
const ROLES_LISTED: i64 = 500;
/// Délai minimal entre deux écritures du répertoire pour une même branche.
const ROLES_EVERY: Duration = Duration::from_secs(2);

pub struct Store {
    db: Mutex<Connection>,
    /// Par branche : dernière écriture du répertoire des rôles, et empreinte de ce qui fut écrit.
    roles_state: Mutex<HashMap<String, (Instant, u64)>>,
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
             CREATE TABLE IF NOT EXISTS vectors (
                 key TEXT NOT NULL, model TEXT NOT NULL, hash TEXT NOT NULL, vector BLOB NOT NULL,
                 PRIMARY KEY (key, model)
             );
             CREATE TABLE IF NOT EXISTS permanent (
                 agent_id TEXT PRIMARY KEY, name TEXT NOT NULL, request TEXT NOT NULL, created_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS journal (
                 id INTEGER PRIMARY KEY, agent_id TEXT NOT NULL, agent_name TEXT NOT NULL,
                 ts_ms INTEGER NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS roles (
                 agent_id TEXT PRIMARY KEY, name TEXT NOT NULL, role TEXT NOT NULL,
                 model TEXT NOT NULL DEFAULT '', parent_id TEXT, depth INTEGER NOT NULL DEFAULT 0,
                 target TEXT NOT NULL DEFAULT '', host TEXT NOT NULL DEFAULT '',
                 capabilities TEXT NOT NULL DEFAULT '{}', status TEXT NOT NULL DEFAULT '',
                 instructions TEXT, objective TEXT, criteria TEXT,
                 first_seen_ms INTEGER NOT NULL, last_seen_ms INTEGER NOT NULL
             );",
        )
        .map_err(err)?;
        // bases créées avant la colonne : l'erreur « colonne déjà là » est attendue ensuite
        let _ = db.execute("ALTER TABLE roles ADD COLUMN owner TEXT NOT NULL DEFAULT ''", []);
        let _ = db.execute("ALTER TABLE roles ADD COLUMN unverified INTEGER NOT NULL DEFAULT 0", []);
        db.execute_batch(
            "CREATE INDEX IF NOT EXISTS roles_seen ON roles (last_seen_ms);
             CREATE INDEX IF NOT EXISTS roles_owner ON roles (owner, last_seen_ms);",
        )
        .map_err(err)?;
        let _ = crate::os::make_private(path);
        Ok(Self { db: Mutex::new(db), roles_state: Mutex::new(HashMap::new()) })
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

    /// Les vecteurs gardés pour la recherche en mémoire avec ce modèle : par clé de note,
    /// l'empreinte du texte qui a été plongé, et son vecteur. Ceux des notes effacées sont retirés.
    pub fn vectors(&self, model: &str) -> Result<HashMap<String, (String, Vec<f32>)>, String> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM vectors WHERE key NOT IN (SELECT key FROM memory)", []).map_err(err)?;
        let mut query = db.prepare("SELECT key, hash, vector FROM vectors WHERE model = ?1").map_err(err)?;
        let rows = query
            .query_map([model], |r| {
                let bytes: Vec<u8> = r.get(2)?;
                let vector = bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
                Ok((r.get::<_, String>(0)?, (r.get::<_, String>(1)?, vector)))
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    pub fn keep_vector(&self, key: &str, model: &str, hash: &str, vector: &[f32]) -> Result<(), String> {
        let bytes: Vec<u8> = vector.iter().flat_map(|x| x.to_le_bytes()).collect();
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO vectors (key, model, hash, vector) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key, model) DO UPDATE SET hash = ?3, vector = ?4",
            params![key, model, hash, bytes],
        )
        .map_err(err)?;
        Ok(())
    }

    /// Est-il temps de réécrire le répertoire des rôles pour cette branche ?
    pub fn roles_due(&self, owner: &str) -> bool {
        let mut state = self.roles_state.lock().unwrap();
        if state.len() > 4096 {
            state.clear();
        }
        match state.get_mut(owner) {
            Some((last, _)) if last.elapsed() < ROLES_EVERY => false,
            Some((last, _)) => {
                *last = Instant::now();
                true
            }
            None => {
                state.insert(owner.to_string(), (Instant::now(), 0));
                true
            }
        }
    }

    /// Répertoire des rôles : note les agents d'une branche (l'enfant direct `branch` et sa
    /// descendance), ou met à jour ce qu'on sait d'eux. Ce qu'on leur a demandé (`role_brief`)
    /// n'est pas touché. Rien n'est écrit si rien n'a changé ; une branche ne réécrit jamais la
    /// fiche d'un agent d'une autre branche, et la purge ne lui prend que ses propres fiches.
    pub fn roles_seen(&self, branch: &NodeInfo) {
        let mut rows = Vec::new();
        let mut all = vec![branch];
        while let Some(info) = all.pop() {
            let status = serde_json::to_value(info.status).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let caps = serde_json::to_string(&info.capabilities).unwrap_or_else(|_| "{}".into());
            rows.push((&info.id, &info.name, &info.role, &info.model, &info.parent_id, info.depth, &info.target, &info.host, caps, status, info.unverified));
            all.extend(info.children.iter());
        }
        let print = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            rows.hash(&mut hasher);
            hasher.finish()
        };
        {
            let mut state = self.roles_state.lock().unwrap();
            let entry = state.entry(branch.id.clone()).or_insert((Instant::now(), 0));
            if entry.1 == print {
                return;
            }
            entry.1 = print;
        }
        let mut db = self.db.lock().unwrap();
        let Ok(tx) = db.transaction() else { return };
        let now = crate::now_ms() as i64;
        for (id, name, role, model, parent_id, depth, target, host, caps, status, unverified) in &rows {
            let _ = tx.execute(
                "INSERT INTO roles (agent_id, name, role, model, parent_id, depth, target, host, capabilities, status, owner, first_seen_ms, last_seen_ms, unverified)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13)
                 ON CONFLICT(agent_id) DO UPDATE SET name = ?2, role = ?3, model = ?4, parent_id = ?5, depth = ?6,
                     target = ?7, host = ?8, capabilities = ?9, status = ?10, owner = ?11, last_seen_ms = ?12, unverified = ?13
                 WHERE roles.owner = ?11 OR roles.owner = ''",
                params![id, name, role, model, parent_id, depth, target, host, caps, status, branch.id, now, unverified],
            );
        }
        // les fiches qui portent des consignes (écrites par le maître lui-même) partent en dernier
        let _ = tx.execute(
            "DELETE FROM roles WHERE owner = ?1 AND agent_id NOT IN (
                 SELECT agent_id FROM roles WHERE owner = ?1
                 ORDER BY (instructions IS NOT NULL) DESC, last_seen_ms DESC LIMIT ?2)",
            params![branch.id, ROLES_KEPT_PER_BRANCH],
        );
        let _ = tx.execute(
            "DELETE FROM roles WHERE agent_id NOT IN (
                 SELECT agent_id FROM roles ORDER BY (instructions IS NOT NULL) DESC, last_seen_ms DESC LIMIT ?1)",
            params![ROLES_KEPT],
        );
        let _ = tx.commit();
    }

    /// Un agent permanent : la demande de création, telle que le maître l'a faite, pour la
    /// refaire à son redémarrage. Elle repassera alors par les mêmes contrôles.
    pub fn permanent_save(&self, agent_id: &str, name: &str, request: &str) -> Result<(), String> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO permanent (agent_id, name, request, created_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(agent_id) DO UPDATE SET name = ?2, request = ?3",
            params![agent_id, name, request, crate::now_ms() as i64],
        )
        .map(|_| ())
        .map_err(err)
    }

    /// L'agent n'est plus permanent. Rend vrai s'il l'était.
    pub fn permanent_delete(&self, agent_id: &str) -> bool {
        self.db.lock().unwrap().execute("DELETE FROM permanent WHERE agent_id = ?1", params![agent_id]).is_ok_and(|n| n > 0)
    }

    /// Les agents permanents, du plus ancien au plus récent : (identifiant, nom, demande).
    pub fn permanent_list(&self) -> Vec<(String, String, String)> {
        let db = self.db.lock().unwrap();
        let Ok(mut query) = db.prepare("SELECT agent_id, name, request FROM permanent ORDER BY created_ms, agent_id LIMIT 64") else { return Vec::new() };
        let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)));
        rows.map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    /// Ce que le maître a demandé à l'enfant qu'il vient de créer.
    pub fn role_brief(&self, agent_id: &str, name: &str, role: &str, instructions: &str, objective: &str, criteria: &str) {
        let instructions = crate::clean(instructions, 20_000, true);
        let objective = crate::clean(objective, 8_000, true);
        let criteria = crate::clean(criteria, 4_000, true);
        let db = self.db.lock().unwrap();
        let now = crate::now_ms() as i64;
        let _ = db.execute(
            "INSERT INTO roles (agent_id, name, role, instructions, objective, criteria, owner, first_seen_ms, last_seen_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?1, ?7, ?7)
             ON CONFLICT(agent_id) DO UPDATE SET instructions = ?4, objective = ?5, criteria = ?6
             WHERE roles.owner = ?1 OR roles.owner = ''",
            params![agent_id, name, role, instructions, objective, criteria, now],
        );
    }

    /// Le répertoire des rôles, du plus récent au plus ancien : au plus `ROLES_LISTED` fiches,
    /// celles qui portent des consignes d'abord (une branche bavarde ne les chasse pas de la liste).
    pub fn roles(&self) -> Vec<serde_json::Value> {
        let db = self.db.lock().unwrap();
        let Ok(mut query) = db.prepare(
            "SELECT agent_id, name, role, model, parent_id, depth, target, host, capabilities, status,
                    instructions, objective, criteria, first_seen_ms, last_seen_ms, unverified
             FROM (SELECT * FROM roles ORDER BY (instructions IS NOT NULL) DESC, first_seen_ms DESC LIMIT ?1)
             ORDER BY first_seen_ms DESC, agent_id",
        ) else {
            return Vec::new();
        };
        let rows = query.query_map([ROLES_LISTED], |r| {
            let caps: String = r.get(8)?;
            Ok(serde_json::json!({
                "agent_id": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?, "role": r.get::<_, String>(2)?,
                "model": r.get::<_, String>(3)?, "parent_id": r.get::<_, Option<String>>(4)?, "depth": r.get::<_, i64>(5)?,
                "target": r.get::<_, String>(6)?, "host": r.get::<_, String>(7)?,
                "capabilities": serde_json::from_str::<serde_json::Value>(&caps).unwrap_or_default(),
                "status": r.get::<_, String>(9)?,
                "instructions": r.get::<_, Option<String>>(10)?, "objective": r.get::<_, Option<String>>(11)?,
                "criteria": r.get::<_, Option<String>>(12)?,
                "first_seen_ms": r.get::<_, i64>(13)?, "last_seen_ms": r.get::<_, i64>(14)?,
                "unverified": r.get::<_, bool>(15)?,
            }))
        });
        rows.map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
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
    fn a_branch_only_writes_its_own_roles() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("agent.db")).unwrap();
        let info = |id: &str, role: &str, children: Vec<serde_json::Value>| {
            serde_json::json!({
                "id": id, "name": id, "role": role, "parent_id": null, "depth": 1, "status": "idle", "model": "main",
                "target": "local", "host": "h", "os": "o", "ips": [], "pid": 1, "started_at_ms": 0, "last_seen_ms": 0,
                "tokens_used": 0, "capabilities": {}, "children": children
            })
        };
        let tree = |v: serde_json::Value| serde_json::from_value::<NodeInfo>(v).unwrap();
        let role_of = |id: &str| store.roles().into_iter().find(|r| r["agent_id"] == id).map(|r| r["role"].as_str().unwrap().to_string());

        store.role_brief("a", "a", "chef", "consignes\u{202e}", "objectif", "critère");
        store.roles_seen(&tree(info("a", "chef", vec![info("a1", "lecteur", vec![])])));
        // une autre branche déclare un agent du même identifiant : la fiche ne bouge pas
        store.roles_seen(&tree(info("b", "autre", vec![info("a1", "usurpé", vec![]), info("a", "usurpé", vec![])])));
        assert_eq!(role_of("a1").as_deref(), Some("lecteur"));
        assert_eq!(role_of("a").as_deref(), Some("chef"));
        let brief = store.roles().into_iter().find(|r| r["agent_id"] == "a").unwrap();
        assert_eq!(brief["instructions"], "consignes", "consignes non nettoyées");

        // une branche qui déclare des milliers d'agents n'efface que les siens, et la lecture est bornée
        for round in 0..3 {
            let many = (0..1500).map(|i| info(&format!("b-{round}-{i}"), "foule", vec![])).collect();
            store.roles_seen(&tree(info("b", "autre", many)));
        }
        assert_eq!(role_of("a").as_deref(), Some("chef"), "la fiche du maître a été purgée");
        let db = store.db.lock().unwrap();
        let count = |sql: &str| db.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(count("SELECT count(*) FROM roles WHERE owner = 'a'"), 2, "la branche voisine a été purgée");
        assert!(count("SELECT count(*) FROM roles WHERE owner = 'b'") <= ROLES_KEPT_PER_BRANCH);
        drop(db);
        assert!(store.roles().len() as i64 <= ROLES_LISTED);

        // rien n'est réécrit si rien n'a changé, et pas deux fois de suite pour une même branche
        assert!(store.roles_due("c"));
        assert!(!store.roles_due("c"));
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
