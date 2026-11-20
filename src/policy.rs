//! Les droits d'un agent. Tout ce qui n'est pas accordé est refusé.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Capabilities {
    /// Dossiers (chemins absolus) que l'agent peut lire, sous-dossiers compris.
    pub fs_read: Vec<PathBuf>,
    /// Dossiers (chemins absolus) où l'agent peut écrire.
    pub fs_write: Vec<PathBuf>,
    /// Commandes que l'agent peut lancer. Jamais de shell : un programme et ses arguments.
    pub exec: Vec<ExecRule>,
    /// Droit de créer des agents enfants. Absent : aucun enfant.
    pub spawn: Option<SpawnPolicy>,
    /// Modèles utilisables, par leur alias dans la configuration du maître.
    pub models: Vec<String>,
    /// Budget de tokens de l'agent et de toute sa descendance. Absent : pas de plafond.
    pub max_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRule {
    /// Nom ou chemin du programme, tel qu'il sera lancé. Préférez un chemin absolu.
    pub program: String,
    /// Arguments autorisés, position par position. `*` vaut un argument quelconque,
    /// `**` en dernière position vaut tout le reste. Absent : arguments libres.
    #[serde(default)]
    pub args: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnPolicy {
    /// Nombre maximum d'enfants directs en vie.
    pub max_children: u32,
    /// Nombre de niveaux autorisés sous cet agent (1 : des enfants, mais pas de petits-enfants).
    pub max_depth: u32,
    /// Modes de déploiement autorisés. Seul `local` existe pour l'instant.
    #[serde(default = "local_only")]
    pub targets: Vec<String>,
}

fn local_only() -> Vec<String> {
    vec!["local".to_string()]
}

impl Capabilities {
    /// Vérifie que les droits sont bien formés.
    pub fn validate(&self) -> Result<(), String> {
        for path in self.fs_read.iter().chain(&self.fs_write) {
            if !path.is_absolute() {
                return Err(format!("chemin non absolu : {}", path.display()));
            }
            if path.components().any(|c| matches!(c, Component::ParentDir)) {
                return Err(format!("chemin avec « .. » : {}", path.display()));
            }
        }
        for rule in &self.exec {
            if rule.program.trim().is_empty() {
                return Err("règle de commande sans programme".into());
            }
            if let Some(args) = &rule.args {
                if args.iter().rev().skip(1).any(|a| a == "**") {
                    return Err(format!("« ** » n'est permis qu'en dernier argument ({})", rule.program));
                }
            }
        }
        if let Some(spawn) = &self.spawn {
            if let Some(target) = spawn.targets.iter().find(|t| t.as_str() != "local") {
                return Err(format!("mode de déploiement inconnu : {target}"));
            }
        }
        Ok(())
    }

    /// Un parent (`self`) peut-il accorder `child` à un enfant ? Jamais plus que ce qu'il a lui-même.
    pub fn check_grant(&self, child: &Capabilities) -> Result<(), String> {
        child.validate()?;
        let Some(own) = &self.spawn else {
            return Err("cet agent n'a pas le droit de créer des enfants".into());
        };
        if own.max_depth == 0 {
            return Err("profondeur maximale atteinte : plus d'enfants à ce niveau".into());
        }
        for (label, wanted, held) in [
            ("lecture", &child.fs_read, &self.fs_read),
            ("écriture", &child.fs_write, &self.fs_write),
        ] {
            for path in wanted {
                if !held.iter().any(|root| path.starts_with(root)) {
                    return Err(format!("{label} de {} : hors de vos propres droits", path.display()));
                }
            }
        }
        for rule in &child.exec {
            let covered = self
                .exec
                .iter()
                .any(|own| own.program == rule.program && (own.args.is_none() || own.args == rule.args));
            if !covered {
                return Err(format!("commande « {} » : hors de vos propres droits", rule.describe()));
            }
        }
        for model in &child.models {
            if !self.models.contains(model) {
                return Err(format!("modèle « {model} » : hors de vos propres droits"));
            }
        }
        if let Some(limit) = self.max_tokens {
            match child.max_tokens {
                Some(wanted) if wanted <= limit => {}
                _ => return Err(format!("budget : l'enfant doit avoir un max_tokens d'au plus {limit}")),
            }
        }
        if let Some(wanted) = &child.spawn {
            if wanted.max_depth >= own.max_depth {
                return Err(format!("spawn.max_depth de l'enfant : au plus {}", own.max_depth - 1));
            }
            if wanted.max_children > own.max_children {
                return Err(format!("spawn.max_children de l'enfant : au plus {}", own.max_children));
            }
            if let Some(target) = wanted.targets.iter().find(|t| !own.targets.contains(t)) {
                return Err(format!("mode de déploiement « {target} » : hors de vos propres droits"));
            }
        }
        Ok(())
    }

    /// Chemin réel d'un fichier ou dossier existant, s'il est lisible par l'agent.
    pub fn resolve_read(&self, path: &Path) -> Result<PathBuf, String> {
        let real = real_path(path)?;
        ensure_within(&real, &self.fs_read, "lecture")?;
        Ok(real)
    }

    /// Chemin réel d'un fichier à écrire (existant ou non), s'il est inscriptible par l'agent.
    pub fn resolve_write(&self, path: &Path) -> Result<PathBuf, String> {
        let real = if path.exists() {
            real_path(path)?
        } else {
            let name = path.file_name().ok_or("chemin sans nom de fichier")?;
            let parent = path.parent().ok_or("chemin sans dossier parent")?;
            real_path(parent)?.join(name)
        };
        ensure_within(&real, &self.fs_write, "écriture")?;
        Ok(real)
    }

    /// La commande est-elle autorisée, exactement telle quelle ?
    pub fn check_exec(&self, program: &str, args: &[String]) -> Result<(), String> {
        if self.exec.iter().any(|rule| rule.program == program && rule.allows(args)) {
            Ok(())
        } else {
            Err(format!("commande non autorisée : {program} {}", args.join(" ")))
        }
    }
}

impl ExecRule {
    fn allows(&self, args: &[String]) -> bool {
        let Some(pattern) = &self.args else { return true };
        let mut given = args.iter();
        for (i, expected) in pattern.iter().enumerate() {
            if expected == "**" && i == pattern.len() - 1 {
                return true;
            }
            match given.next() {
                Some(arg) if expected == "*" || expected == arg => {}
                _ => return false,
            }
        }
        given.next().is_none()
    }

    pub fn describe(&self) -> String {
        match &self.args {
            Some(args) => format!("{} {}", self.program, args.join(" ")).trim().to_string(),
            None => format!("{} (arguments libres)", self.program),
        }
    }
}

/// Chemin canonique : absolu, liens symboliques résolus. Le fichier doit exister.
fn real_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("chemin non absolu : {}", path.display()));
    }
    path.canonicalize().map_err(|e| format!("{} : {e}", path.display()))
}

fn ensure_within(real: &Path, roots: &[PathBuf], action: &str) -> Result<(), String> {
    let allowed = roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| real.starts_with(root));
    if allowed {
        Ok(())
    } else {
        Err(format!("{action} refusée : {} est hors des dossiers autorisés", real.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(program: &str, args: Option<&[&str]>) -> ExecRule {
        ExecRule { program: program.into(), args: args.map(|a| a.iter().map(|s| s.to_string()).collect()) }
    }

    fn parent() -> Capabilities {
        Capabilities {
            fs_read: vec!["/srv/projet".into()],
            fs_write: vec!["/srv/projet/out".into()],
            exec: vec![rule("git", Some(&["status"])), rule("ls", None)],
            spawn: Some(SpawnPolicy { max_children: 3, max_depth: 2, targets: local_only() }),
            models: vec!["main".into()],
            max_tokens: Some(1000),
        }
    }

    #[test]
    fn child_within_parent_rights_is_granted() {
        let child = Capabilities {
            fs_read: vec!["/srv/projet/docs".into()],
            exec: vec![rule("git", Some(&["status"])), rule("ls", Some(&["-l"]))],
            spawn: Some(SpawnPolicy { max_children: 1, max_depth: 1, targets: local_only() }),
            models: vec!["main".into()],
            max_tokens: Some(500),
            ..Default::default()
        };
        assert_eq!(parent().check_grant(&child), Ok(()));
    }

    #[test]
    fn child_cannot_exceed_parent() {
        let p = parent();
        let base = Capabilities { max_tokens: Some(10), ..Default::default() };
        let cases = [
            Capabilities { fs_read: vec!["/srv".into()], ..base.clone() },
            Capabilities { fs_read: vec!["/srv/projet/../secret".into()], ..base.clone() },
            Capabilities { fs_write: vec!["/srv/projet".into()], ..base.clone() },
            Capabilities { exec: vec![rule("git", Some(&["push"]))], ..base.clone() },
            Capabilities { exec: vec![rule("git", None)], ..base.clone() },
            Capabilities { exec: vec![rule("rm", None)], ..base.clone() },
            Capabilities { models: vec!["deep".into()], ..base.clone() },
            Capabilities { max_tokens: Some(1001), ..base.clone() },
            Capabilities { max_tokens: None, ..base.clone() },
            Capabilities { spawn: Some(SpawnPolicy { max_children: 1, max_depth: 2, targets: local_only() }), ..base.clone() },
            Capabilities { spawn: Some(SpawnPolicy { max_children: 4, max_depth: 1, targets: local_only() }), ..base.clone() },
        ];
        for case in cases {
            assert!(p.check_grant(&case).is_err(), "aurait dû être refusé : {case:?}");
        }
    }

    #[test]
    fn agent_without_spawn_right_grants_nothing() {
        let p = Capabilities { spawn: None, ..parent() };
        assert!(p.check_grant(&Capabilities::default()).is_err());
    }

    #[test]
    fn exec_patterns() {
        let caps = Capabilities {
            exec: vec![rule("git", Some(&["log", "*"])), rule("grep", Some(&["-r", "**"])), rule("uptime", Some(&[]))],
            ..Default::default()
        };
        let ok = |p: &str, a: &[&str]| caps.check_exec(p, &a.iter().map(|s| s.to_string()).collect::<Vec<_>>()).is_ok();
        assert!(ok("git", &["log", "main"]));
        assert!(!ok("git", &["log"]));
        assert!(!ok("git", &["log", "main", "--all"]));
        assert!(!ok("git", &["push", "main"]));
        assert!(ok("grep", &["-r", "x", "/tmp"]));
        assert!(ok("grep", &["-r"]));
        assert!(ok("uptime", &[]));
        assert!(!ok("uptime", &["-p"]));
        assert!(!ok("sh", &["-c", "uptime"]));
    }

    #[test]
    fn file_access_stays_inside_allowed_folders() {
        let dir = tempfile::tempdir().unwrap();
        let allowed = dir.path().join("allowed");
        let secret = dir.path().join("secret");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(allowed.join("a.txt"), "a").unwrap();
        std::fs::write(secret.join("s.txt"), "s").unwrap();
        let caps = Capabilities { fs_read: vec![allowed.clone()], fs_write: vec![allowed.clone()], ..Default::default() };

        assert!(caps.resolve_read(&allowed.join("a.txt")).is_ok());
        assert!(caps.resolve_read(&secret.join("s.txt")).is_err());
        assert!(caps.resolve_read(&allowed.join("../secret/s.txt")).is_err());
        assert!(caps.resolve_write(&allowed.join("new.txt")).is_ok());
        assert!(caps.resolve_write(&secret.join("new.txt")).is_err());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&secret, allowed.join("link")).unwrap();
            assert!(caps.resolve_read(&allowed.join("link/s.txt")).is_err());
            assert!(caps.resolve_write(&allowed.join("link/new.txt")).is_err());
        }
    }
}
