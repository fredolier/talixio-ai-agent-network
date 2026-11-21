//! Les droits d'un agent. Tout ce qui n'est pas accordé est refusé.
//!
//! Ce contrôle est fait par le process lui-même. Tant qu'un cloisonnement par le système
//! d'exploitation ne le double pas, une commande autorisée s'exécute avec tous les droits
//! de l'utilisateur du process : une règle de commande se rédige donc au plus serré.

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
    /// Chemin absolu du programme.
    pub program: PathBuf,
    /// Arguments autorisés, position par position :
    /// - un texte : cet argument exact ;
    /// - `*` : un argument libre, qui ne commence pas par `-` (jamais une option) ;
    /// - `{read}` / `{write}` : un chemin que l'agent a le droit de lire / d'écrire ;
    /// - `**` en dernière position : une suite d'arguments libres, aucun ne commençant par `-`.
    ///
    /// Absent : arguments entièrement libres, ce qui revient à donner tous les droits du programme.
    #[serde(default)]
    pub args: Option<Vec<String>>,
    /// Dossier d'exécution. Absent : le dossier temporaire du système.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnPolicy {
    /// Nombre maximum d'enfants directs en vie.
    pub max_children: u32,
    /// Nombre de niveaux autorisés sous cet agent (1 : des enfants, mais pas de petits-enfants).
    pub max_depth: u32,
    /// Où l'agent peut créer ses enfants : `local` (sa propre machine), ou le nom d'une cible
    /// de déploiement définie dans la configuration du maître (`[targets.<nom>]`).
    #[serde(default = "local_only")]
    pub targets: Vec<String>,
}

fn local_only() -> Vec<String> {
    vec!["local".to_string()]
}

/// Un nom de cible : minuscules, chiffres, tirets.
pub fn valid_target_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && !name.starts_with('-')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn check_plain_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("chemin non absolu : {}", path.display()));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("chemin avec « .. » : {}", path.display()));
    }
    Ok(())
}

impl Capabilities {
    /// Vérifie que les droits sont bien formés.
    pub fn validate(&self) -> Result<(), String> {
        for path in self.fs_read.iter().chain(&self.fs_write) {
            check_plain_path(path)?;
        }
        for rule in &self.exec {
            check_plain_path(&rule.program).map_err(|e| format!("programme d'une règle de commande : {e}"))?;
            if let Some(cwd) = &rule.cwd {
                check_plain_path(cwd)?;
            }
            // un programme que l'agent peut réécrire n'est plus une liste fermée
            if self.fs_write.iter().any(|root| rule.program.starts_with(root)) {
                return Err(format!("{} est dans un dossier inscriptible par l'agent", rule.program.display()));
            }
            if let Some(args) = &rule.args {
                if args.iter().rev().skip(1).any(|a| a == "**") {
                    return Err(format!("« ** » n'est permis qu'en dernier argument ({})", rule.program.display()));
                }
            }
        }
        if let Some(spawn) = &self.spawn {
            if let Some(target) = spawn.targets.iter().find(|t| !valid_target_name(t)) {
                return Err(format!("nom de cible invalide : {target}"));
            }
        }
        Ok(())
    }

    /// Vérifié par l'enfant, sur la machine où il arrive : ses dossiers, une fois résolus sur
    /// cette machine, tiennent-ils dans le plafond de la cible ? Le parent n'a pu comparer que
    /// des noms ; un lien symbolique de la machine d'arrivée ne doit pas élargir le droit.
    pub fn check_ceiling(&self, ceiling: &Capabilities) -> Result<(), String> {
        let real = |list: &[PathBuf]| list.iter().filter_map(|p| p.canonicalize().ok()).collect::<Vec<_>>();
        for (wanted, held) in [(&self.fs_read, real(&ceiling.fs_read)), (&self.fs_write, real(&ceiling.fs_write))] {
            if let Some(path) = wanted.iter().find(|path| !held.iter().any(|root| path.starts_with(root))) {
                return Err(format!("{} sort du plafond de droits de cette machine", path.display()));
            }
        }
        Ok(())
    }

    /// Remplace chaque dossier par son chemin réel (liens symboliques résolus). Les dossiers
    /// doivent exister. À faire une fois, quand les droits sont accordés : par la suite, un
    /// dossier accordé qui deviendrait un lien n'ouvre plus rien.
    pub fn canonicalize(&mut self) -> Result<(), String> {
        self.validate()?;
        for list in [&mut self.fs_read, &mut self.fs_write] {
            for path in list.iter_mut() {
                let real = path.canonicalize().map_err(|e| format!("{} : {e}", path.display()))?;
                if !real.is_dir() {
                    return Err(format!("{} n'est pas un dossier", real.display()));
                }
                *path = real;
            }
            list.sort();
            list.dedup();
        }
        self.validate()
    }

    /// Un parent (`self`) peut-il accorder `child` à un enfant ? Jamais plus que ce qu'il a lui-même.
    /// Les deux jeux de droits doivent être passés par `canonicalize`.
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
            let covered = self.exec.iter().any(|own| {
                own.program == rule.program && own.cwd == rule.cwd && (own.args.is_none() || own.args == rule.args)
            });
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
    /// `denied` : dossiers interdits quoi qu'il arrive (l'état de l'agent, avec ses secrets).
    pub fn resolve_read(&self, path: &Path, denied: &[PathBuf]) -> Result<PathBuf, String> {
        let real = real_path(path)?;
        ensure_within(&real, &self.fs_read, denied, "lecture")?;
        Ok(real)
    }

    /// Chemin réel d'un fichier à écrire (existant ou non), s'il est inscriptible par l'agent.
    pub fn resolve_write(&self, path: &Path, denied: &[PathBuf]) -> Result<PathBuf, String> {
        check_plain_path(path)?;
        // `symlink_metadata` ne suit pas les liens : un lien, même cassé, est vu et refusé
        let real = match path.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!("écriture refusée : {} est un lien symbolique", path.display()));
            }
            Ok(_) => real_path(path)?,
            Err(_) => {
                let name = path.file_name().ok_or("chemin sans nom de fichier")?;
                let parent = path.parent().ok_or("chemin sans dossier parent")?;
                real_path(parent)?.join(name)
            }
        };
        ensure_within(&real, &self.fs_write, denied, "écriture")?;
        Ok(real)
    }

    /// La commande est-elle autorisée, exactement telle quelle ? Renvoie la règle qui la couvre et
    /// les arguments à passer : les chemins des emplacements `{read}` / `{write}` y sont remplacés
    /// par leur chemin réel, celui qui a été vérifié.
    pub fn check_exec(
        &self,
        program: &Path,
        args: &[String],
        denied: &[PathBuf],
    ) -> Result<(&ExecRule, Vec<String>), String> {
        self.exec
            .iter()
            .filter(|rule| rule.program == program)
            .find_map(|rule| rule.allows(args, self, denied).map(|resolved| (rule, resolved)))
            .ok_or_else(|| format!("commande non autorisée : {} {}", program.display(), args.join(" ")))
    }
}

impl ExecRule {
    fn allows(&self, args: &[String], caps: &Capabilities, denied: &[PathBuf]) -> Option<Vec<String>> {
        let Some(pattern) = &self.args else { return Some(args.to_vec()) };
        let free = |arg: &String| !arg.starts_with('-');
        let real = |path: Result<PathBuf, String>| path.ok().and_then(|p| p.to_str().map(str::to_string));
        let mut resolved = Vec::with_capacity(args.len());
        let mut given = args.iter();
        for (i, expected) in pattern.iter().enumerate() {
            if expected == "**" && i == pattern.len() - 1 {
                let rest: Vec<&String> = given.collect();
                if !rest.iter().all(|arg| free(arg)) {
                    return None;
                }
                resolved.extend(rest.into_iter().cloned());
                return Some(resolved);
            }
            let arg = given.next()?;
            resolved.push(match expected.as_str() {
                "*" if free(arg) => arg.clone(),
                "*" => return None,
                "{read}" => real(caps.resolve_read(Path::new(arg), denied))?,
                "{write}" => real(caps.resolve_write(Path::new(arg), denied))?,
                literal if literal == arg => arg.clone(),
                _ => return None,
            });
        }
        given.next().is_none().then_some(resolved)
    }

    pub fn describe(&self) -> String {
        match &self.args {
            Some(args) => format!("{} {}", self.program.display(), args.join(" ")).trim().to_string(),
            None => format!("{} (arguments libres)", self.program.display()),
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

fn ensure_within(real: &Path, roots: &[PathBuf], denied: &[PathBuf], action: &str) -> Result<(), String> {
    if denied.iter().any(|d| real.starts_with(d)) {
        return Err(format!("{action} refusée : {} est réservé à l'agent lui-même", real.display()));
    }
    // un dossier accordé doit être resté ce qu'il était : s'il est devenu un lien, il n'ouvre plus rien
    let allowed = roots
        .iter()
        .filter(|root| root.canonicalize().is_ok_and(|now| now == **root))
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
        ExecRule { program: abs(program), args: args.map(|a| a.iter().map(|s| s.to_string()).collect()), cwd: None }
    }

    /// Les chemins des tests sont écrits à la façon POSIX ; sous Windows, un chemin n'est absolu
    /// qu'avec sa lettre de lecteur.
    fn abs(path: &str) -> PathBuf {
        if cfg!(windows) && path.starts_with('/') { format!("C:{path}").into() } else { path.into() }
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn parent() -> Capabilities {
        Capabilities {
            fs_read: vec![abs("/srv/projet")],
            fs_write: vec![abs("/srv/projet/out")],
            exec: vec![rule("/usr/bin/git", Some(&["status"])), rule("/bin/ls", None)],
            spawn: Some(SpawnPolicy { max_children: 3, max_depth: 2, targets: local_only() }),
            models: vec!["main".into()],
            max_tokens: Some(1000),
        }
    }

    #[test]
    fn child_within_parent_rights_is_granted() {
        let child = Capabilities {
            fs_read: vec![abs("/srv/projet/docs")],
            exec: vec![rule("/usr/bin/git", Some(&["status"])), rule("/bin/ls", Some(&["-l"]))],
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
            Capabilities { fs_read: vec![abs("/srv")], ..base.clone() },
            Capabilities { fs_read: vec![abs("/srv/projet/../secret")], ..base.clone() },
            Capabilities { fs_write: vec![abs("/srv/projet")], ..base.clone() },
            Capabilities { exec: vec![rule("/usr/bin/git", Some(&["push"]))], ..base.clone() },
            Capabilities { exec: vec![rule("/usr/bin/git", None)], ..base.clone() },
            Capabilities { exec: vec![rule("/bin/rm", None)], ..base.clone() },
            Capabilities { exec: vec![ExecRule { cwd: Some(abs("/")), ..rule("/usr/bin/git", Some(&["status"])) }], ..base.clone() },
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
    fn malformed_rules_are_rejected() {
        let relative = Capabilities { exec: vec![rule("git", None)], ..Default::default() };
        assert!(relative.validate().is_err());
        let writable = Capabilities {
            fs_write: vec![abs("/opt/outils")],
            exec: vec![rule("/opt/outils/script", None)],
            ..Default::default()
        };
        assert!(writable.validate().is_err());
    }

    #[test]
    fn exec_patterns_never_let_a_wildcard_be_an_option() {
        let caps = Capabilities {
            exec: vec![
                rule("/usr/bin/git", Some(&["log", "*"])),
                rule("/bin/grep", Some(&["-r", "**"])),
                rule("/usr/bin/uptime", Some(&[])),
            ],
            ..Default::default()
        };
        let ok = |p: &str, a: &[&str]| caps.check_exec(&abs(p), &strings(a), &[]).is_ok();
        assert!(ok("/usr/bin/git", &["log", "main"]));
        assert!(!ok("/usr/bin/git", &["log"]));
        assert!(!ok("/usr/bin/git", &["log", "main", "--all"]));
        assert!(!ok("/usr/bin/git", &["push", "main"]));
        // l'audit : `git log --output=…` écrivait n'importe où
        assert!(!ok("/usr/bin/git", &["log", "--output=/etc/passwd"]));
        assert!(ok("/bin/grep", &["-r", "x", "/tmp"]));
        assert!(ok("/bin/grep", &["-r"]));
        assert!(!ok("/bin/grep", &["-r", "x", "--include=*"]));
        assert!(ok("/usr/bin/uptime", &[]));
        assert!(!ok("/usr/bin/uptime", &["-p"]));
        assert!(!ok("/bin/sh", &["-c", "uptime"]));
        assert!(!ok("git", &["log", "main"]));
    }

    #[test]
    fn path_placeholders_follow_the_file_rights() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let (allowed, secret) = (base.join("allowed"), base.join("secret"));
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(allowed.join("a.txt"), "a").unwrap();
        std::fs::write(secret.join("s.txt"), "s").unwrap();
        let caps = Capabilities {
            fs_read: vec![allowed.clone()],
            exec: vec![rule("/bin/cat", Some(&["{read}"]))],
            ..Default::default()
        };
        let ok = |arg: &Path| caps.check_exec(&abs("/bin/cat"), &[arg.display().to_string()], &[]).is_ok();
        assert!(ok(&allowed.join("a.txt")));
        assert!(!ok(&secret.join("s.txt")));
        assert!(!ok(&allowed.join("../secret/s.txt")));
    }

    #[test]
    fn file_access_stays_inside_allowed_folders() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let (allowed, secret, state) = (base.join("allowed"), base.join("secret"), base.join("allowed/.talixio"));
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(allowed.join("a.txt"), "a").unwrap();
        std::fs::write(secret.join("s.txt"), "s").unwrap();
        std::fs::write(state.join("api-token"), "t").unwrap();
        let mut caps = Capabilities { fs_read: vec![allowed.clone()], fs_write: vec![allowed.clone()], ..Default::default() };
        caps.canonicalize().unwrap();
        let denied = [state.clone()];

        assert!(caps.resolve_read(&allowed.join("a.txt"), &denied).is_ok());
        assert!(caps.resolve_read(&secret.join("s.txt"), &denied).is_err());
        assert!(caps.resolve_read(&allowed.join("../secret/s.txt"), &denied).is_err());
        assert!(caps.resolve_write(&allowed.join("new.txt"), &denied).is_ok());
        assert!(caps.resolve_write(&secret.join("new.txt"), &denied).is_err());
        // l'état de l'agent (jeton de l'API) reste hors d'atteinte, même dans un dossier accordé
        assert!(caps.resolve_read(&state.join("api-token"), &denied).is_err());
        assert!(caps.resolve_write(&state.join("api-token"), &denied).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&secret, allowed.join("link")).unwrap();
            assert!(caps.resolve_read(&allowed.join("link/s.txt"), &denied).is_err());
            assert!(caps.resolve_write(&allowed.join("link/new.txt"), &denied).is_err());
            // un lien cassé qui pointe dehors : écrire dedans créerait le fichier dehors
            symlink(secret.join("absent.txt"), allowed.join("dangling")).unwrap();
            assert!(caps.resolve_write(&allowed.join("dangling"), &denied).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_cannot_widen_a_grant() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let (work, secret) = (base.join("work"), base.join("secret"));
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&secret).unwrap();
        std::fs::write(secret.join("s.txt"), "s").unwrap();
        symlink(&secret, work.join("link")).unwrap();
        let mut parent = Capabilities {
            fs_read: vec![work.clone()],
            spawn: Some(SpawnPolicy { max_children: 1, max_depth: 1, targets: local_only() }),
            ..Default::default()
        };
        parent.canonicalize().unwrap();

        // accordé par son chemin réel, le lien se révèle hors des droits du parent
        let mut child = Capabilities { fs_read: vec![work.join("link")], ..Default::default() };
        child.canonicalize().unwrap();
        assert!(parent.check_grant(&child).is_err());

        // un dossier accordé puis remplacé par un lien n'ouvre plus rien
        let sub = work.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let mut child = Capabilities { fs_read: vec![sub.clone()], ..Default::default() };
        child.canonicalize().unwrap();
        assert_eq!(parent.check_grant(&child), Ok(()));
        std::fs::remove_dir(&sub).unwrap();
        symlink(&secret, &sub).unwrap();
        assert!(child.resolve_read(&sub.join("s.txt"), &[]).is_err());
    }
}
