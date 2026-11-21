//! Le maître en service du système : il repart seul au démarrage de la machine et après une panne.
//!
//! Linux : une unité systemd (du compte, ou du système avec `--user` quand on installe en root).
//! macOS : un agent launchd du compte. Windows : une tâche planifiée à l'ouverture de session.
//! Le service ne tourne jamais en root, et sa définition ne contient aucun secret : les clés
//! restent dans leurs fichiers, ou dans un fichier d'environnement fermé (Linux).

use std::path::{Path, PathBuf};

pub const NAME: &str = "talixio-agent";
const LABEL: &str = "com.talixio.agent";
const TASK: &str = "TalixioAgent";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Unité systemd du compte courant.
    SystemdUser,
    /// Unité systemd du système, qui tourne sous un compte ordinaire nommé.
    SystemdSystem,
    Launchd,
    WindowsTask,
}

/// Ce qu'il faut pour écrire la définition du service. Tous les chemins sont absolus.
#[derive(Clone, Debug)]
pub struct Spec {
    pub kind: Kind,
    pub exe: PathBuf,
    pub config: PathBuf,
    pub workdir: PathBuf,
    /// Le dossier personnel du compte (unité du compte, launchd).
    pub home: PathBuf,
    /// Linux : variables d'environnement du maître (les clés lues par `api_key_env`).
    pub env_file: Option<PathBuf>,
    /// Unité du système : le compte, jamais root, sous lequel le maître tourne.
    pub user: Option<String>,
    pub uid: u32,
}

/// Ce que l'installation fait : un fichier à écrire (s'il y en a un), des commandes à lancer.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub file: Option<(PathBuf, String)>,
    pub commands: Vec<Vec<String>>,
    /// Commandes dont l'échec n'empêche pas le service de tourner.
    pub optional: Vec<Vec<String>>,
    pub notes: Vec<String>,
}

fn words(text: &[&str]) -> Vec<String> {
    text.iter().map(|w| w.to_string()).collect()
}

/// Un chemin qui entre tel quel dans une définition de service : absolu, sans caractère qui
/// changerait le sens de la ligne (guillemet, retour, `%` et `$` de systemd, chevrons du XML).
fn plain(path: &Path, what: &str) -> Result<String, String> {
    let text = path.to_str().ok_or(format!("{what} : chemin illisible"))?;
    // hors Windows, « \ » n'a rien à faire dans un chemin : pour systemd, il joint deux lignes
    // de l'unité ou change un caractère entre guillemets
    let odd = text.chars().any(|c| c.is_control() || matches!(c, '"' | '\'' | '%' | '$' | '<' | '>' | '&' | '`' | ';' | '|') || (c == '\\' && !cfg!(windows)));
    if !path.is_absolute() || odd || text.is_empty() {
        return Err(format!("{what} : « {} » ne peut pas figurer dans un service (chemin absolu, sans guillemet ni % $ < > & ; |)", crate::clean(text, 200, false)));
    }
    Ok(text.to_string())
}

fn user_name(name: &str) -> bool {
    let first = name.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b == b'_');
    first && name.len() <= 32 && name != "root" && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn unit_path(spec: &Spec) -> PathBuf {
    match spec.kind {
        Kind::SystemdUser => spec.home.join(".config/systemd/user").join(format!("{NAME}.service")),
        Kind::SystemdSystem => PathBuf::from(format!("/etc/systemd/system/{NAME}.service")),
        Kind::Launchd => spec.home.join("Library/LaunchAgents").join(format!("{LABEL}.plist")),
        Kind::WindowsTask => PathBuf::new(),
    }
}

/// Ce que fait `service install`.
pub fn install_plan(spec: &Spec) -> Result<Plan, String> {
    let exe = plain(&spec.exe, "exécutable")?;
    let config = plain(&spec.config, "configuration")?;
    let workdir = plain(&spec.workdir, "dossier de travail")?;
    if spec.env_file.is_some() && !matches!(spec.kind, Kind::SystemdUser | Kind::SystemdSystem) {
        return Err("`--env-file` ne vaut que sous Linux : ailleurs, donnez les clés par `api_key_file` ou depuis l'interface".into());
    }
    if matches!(spec.kind, Kind::SystemdUser | Kind::Launchd) {
        plain(&spec.home, "dossier personnel (HOME)")?;
    }
    let mut plan = Plan::default();
    match spec.kind {
        Kind::SystemdUser | Kind::SystemdSystem => {
            let system = spec.kind == Kind::SystemdSystem;
            let mut unit = String::from(
                "# Écrit par `talixio-agent service install`. Aucun secret ici.\n\
                 [Unit]\nDescription=Talixio Agent Network (maître)\nAfter=network-online.target\nWants=network-online.target\n\
                 StartLimitIntervalSec=300\nStartLimitBurst=10\n\n\
                 [Service]\n",
            );
            // le compte d'abord, avant toute ligne qui porte un chemin
            if system {
                let user = spec.user.as_deref().filter(|u| user_name(u)).ok_or("installé en root, le service doit tourner sous un compte ordinaire : `--user <compte>` (jamais root)")?;
                unit.push_str(&format!(
                    "User={user}\nCapabilityBoundingSet=\nAmbientCapabilities=\nProtectKernelTunables=yes\nProtectKernelModules=yes\n\
                     ProtectKernelLogs=yes\nProtectControlGroups=yes\nProtectClock=yes\nProtectHostname=yes\nRestrictSUIDSGID=yes\n\
                     LockPersonality=yes\nRestrictRealtime=yes\n"
                ));
            }
            unit.push_str(&format!(
                "Restart=on-failure\nRestartSec=5\nNoNewPrivileges=true\nUMask=0077\nLimitCORE=0\n\
                 ExecStart=\"{exe}\" root --config \"{config}\"\nWorkingDirectory={workdir}\n"
            ));
            if let Some(env) = &spec.env_file {
                unit.push_str(&format!("EnvironmentFile={}\n", plain(env, "fichier d'environnement")?));
            }
            unit.push_str(&format!("\n[Install]\nWantedBy={}\n", if system { "multi-user.target" } else { "default.target" }));
            plan.file = Some((unit_path(spec), unit));
            let scope: &[&str] = if system { &[] } else { &["--user"] };
            let systemctl = |rest: &[&str]| words(&[&["systemctl"], scope, rest].concat());
            plan.commands = vec![systemctl(&["daemon-reload"]), systemctl(&["enable", "--now", &format!("{NAME}.service")])];
            if !system {
                // sans cela, une unité du compte ne démarre qu'à l'ouverture d'une session
                plan.optional = vec![words(&["loginctl", "enable-linger"])];
                plan.notes.push("Pour démarrer avec la machine sans session ouverte, le compte doit pouvoir « s'attarder » (`loginctl enable-linger`) : tenté ci-dessus ; en cas de refus, un administrateur le fait une fois.".into());
            }
            plan.notes.push(format!("Journal du service : journalctl {}-u {NAME}", if system { "" } else { "--user " }));
        }
        Kind::Launchd => {
            let log = plain(&spec.home.join("Library/Logs").join(format!("{NAME}.log")), "journal")?;
            let plist = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\">\n<dict>\n\
                 \t<key>Label</key><string>{LABEL}</string>\n\
                 \t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{exe}</string>\n\t\t<string>root</string>\n\t\t<string>--config</string>\n\t\t<string>{config}</string>\n\t</array>\n\
                 \t<key>WorkingDirectory</key><string>{workdir}</string>\n\
                 \t<key>RunAtLoad</key><true/>\n\
                 \t<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n\
                 \t<key>ThrottleInterval</key><integer>5</integer>\n\
                 \t<key>Umask</key><integer>63</integer>\n\
                 \t<key>StandardOutPath</key><string>{log}</string>\n\
                 \t<key>StandardErrorPath</key><string>{log}</string>\n\
                 </dict>\n</plist>\n"
            );
            let path = unit_path(spec);
            let target = format!("gui/{}", spec.uid);
            plan.optional = vec![words(&["launchctl", "bootout", &format!("{target}/{LABEL}")])];
            plan.commands = vec![words(&["launchctl", "bootstrap", &target, &plain(&path, "définition du service")?])];
            plan.file = Some((path, plist));
            plan.notes.push("Le service démarre à l'ouverture de la session de ce compte, pas avant : sur une machine sans écran, activez l'ouverture de session automatique.".into());
            plan.notes.push(format!("Journal du service : {log}"));
        }
        Kind::WindowsTask => {
            if workdir.ends_with('\\') {
                return Err("dossier de travail : lancez la commande depuis un dossier, pas depuis la racine d'un disque".into());
            }
            // une tâche planifiée démarre dans le dossier du système : le maître rejoint le sien
            let run = format!("\"{exe}\" root --config \"{config}\" --workdir \"{workdir}\"");
            plan.commands = vec![words(&["schtasks", "/Create", "/TN", TASK, "/SC", "ONLOGON", "/RL", "LIMITED", "/F", "/TR", &run])];
            plan.optional = vec![words(&["schtasks", "/Run", "/TN", TASK])];
            plan.notes.push("Le maître démarre à l'ouverture de votre session et n'est pas relancé après une panne : un vrai service Windows reste à faire.".into());
        }
    }
    Ok(plan)
}

/// Ce que fait `service uninstall`. Le dossier d'état, les clés et la configuration restent.
pub fn uninstall_plan(spec: &Spec) -> Plan {
    let mut plan = Plan::default();
    match spec.kind {
        Kind::SystemdUser | Kind::SystemdSystem => {
            let scope: &[&str] = if spec.kind == Kind::SystemdSystem { &[] } else { &["--user"] };
            let systemctl = |rest: &[&str]| words(&[&["systemctl"], scope, rest].concat());
            plan.optional = vec![systemctl(&["disable", "--now", &format!("{NAME}.service")])];
            plan.file = Some((unit_path(spec), String::new()));
            plan.commands = vec![systemctl(&["daemon-reload"])];
        }
        Kind::Launchd => {
            plan.optional = vec![words(&["launchctl", "bootout", &format!("gui/{}/{LABEL}", spec.uid)])];
            plan.file = Some((unit_path(spec), String::new()));
        }
        Kind::WindowsTask => plan.commands = vec![words(&["schtasks", "/Delete", "/TN", TASK, "/F"])],
    }
    plan
}

/// La commande qui montre l'état du service.
pub fn status_command(spec: &Spec) -> Vec<String> {
    match spec.kind {
        Kind::SystemdUser => words(&["systemctl", "--user", "status", "--no-pager", &format!("{NAME}.service")]),
        Kind::SystemdSystem => words(&["systemctl", "status", "--no-pager", &format!("{NAME}.service")]),
        Kind::Launchd => words(&["launchctl", "print", &format!("gui/{}/{LABEL}", spec.uid)]),
        Kind::WindowsTask => words(&["schtasks", "/Query", "/TN", TASK, "/V", "/FO", "LIST"]),
    }
}

/// Le service de cette machine, pour ce compte. `root` : la commande est lancée en root.
pub fn kind_here(root: bool) -> Result<Kind, String> {
    if cfg!(target_os = "linux") {
        Ok(if root { Kind::SystemdSystem } else { Kind::SystemdUser })
    } else if cfg!(target_os = "macos") {
        if root {
            return Err("lancez cette commande sous le compte qui fera tourner le maître, pas en root".into());
        }
        Ok(Kind::Launchd)
    } else if cfg!(windows) {
        Ok(Kind::WindowsTask)
    } else {
        Err("pas de service connu sur ce système".into())
    }
}

/// Montre ce qui serait fait, sans rien faire.
pub fn describe(plan: &Plan, removing: bool) -> String {
    let mut out = String::new();
    if let Some((path, content)) = &plan.file {
        if removing {
            out.push_str(&format!("Fichier retiré : {}\n", path.display()));
        } else {
            out.push_str(&format!("Fichier écrit : {}\n\n{content}\n", path.display()));
        }
    }
    for command in plan.optional.iter().chain(&plan.commands) {
        out.push_str(&format!("Commande : {}\n", command.join(" ")));
    }
    for note in &plan.notes {
        out.push_str(&format!("À savoir : {note}\n"));
    }
    out
}

fn run(command: &[String]) -> Result<(), String> {
    let status = std::process::Command::new(&command[0]).args(&command[1..]).status().map_err(|e| format!("{} : {e}", command[0]))?;
    if status.success() { Ok(()) } else { Err(format!("« {} » a échoué ({status})", command.join(" "))) }
}

/// Applique un plan : le fichier d'abord, puis les commandes. Rend ce qui n'a pas pu se faire
/// sans gravité.
pub fn apply(plan: &Plan, removing: bool) -> Result<Vec<String>, String> {
    let mut skipped = Vec::new();
    if removing {
        for command in &plan.optional {
            if let Err(e) = run(command) {
                skipped.push(e);
            }
        }
    }
    if let Some((path, content)) = &plan.file {
        if removing {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("{} : {e}", path.display())),
                _ => {}
            }
        } else {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
            }
            std::fs::write(path, content).map_err(|e| format!("{} : {e}", path.display()))?;
        }
    }
    if !removing {
        // launchd : retirer l'ancienne définition avant de charger la nouvelle
        let (before, after): (Vec<_>, Vec<_>) = plan.optional.iter().partition(|c| c.iter().any(|w| w == "bootout"));
        for command in before {
            let _ = run(command);
        }
        for command in &plan.commands {
            run(command)?;
        }
        for command in after {
            if let Err(e) = run(command) {
                skipped.push(e);
            }
        }
    } else {
        for command in &plan.commands {
            run(command)?;
        }
    }
    Ok(skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: Kind) -> Spec {
        Spec {
            kind,
            exe: "/home/ada/.local/bin/talixio-agent".into(),
            config: "/srv/talixio/talixio.toml".into(),
            workdir: "/srv/talixio".into(),
            home: "/home/ada".into(),
            env_file: None,
            user: None,
            uid: 501,
        }
    }

    #[test]
    fn systemd_du_compte() {
        let plan = install_plan(&Spec { env_file: Some("/srv/talixio/cles.env".into()), ..spec(Kind::SystemdUser) }).unwrap();
        let (path, unit) = plan.file.unwrap();
        assert_eq!(path, PathBuf::from("/home/ada/.config/systemd/user/talixio-agent.service"));
        assert!(unit.contains("ExecStart=\"/home/ada/.local/bin/talixio-agent\" root --config \"/srv/talixio/talixio.toml\"\n"));
        assert!(unit.contains("WorkingDirectory=/srv/talixio\n") && unit.contains("Restart=on-failure\n") && unit.contains("NoNewPrivileges=true\n"));
        assert!(unit.contains("EnvironmentFile=/srv/talixio/cles.env\n") && unit.contains("WantedBy=default.target\n") && !unit.contains("User="));
        assert_eq!(plan.commands[1], words(&["systemctl", "--user", "enable", "--now", "talixio-agent.service"]));
        assert_eq!(plan.optional, vec![words(&["loginctl", "enable-linger"])]);
    }

    #[test]
    fn systemd_du_systeme_jamais_en_root() {
        for user in [None, Some("root"), Some("Ada"), Some("ada\nUser=root"), Some("")] {
            let asked = Spec { user: user.map(str::to_string), ..spec(Kind::SystemdSystem) };
            assert!(install_plan(&asked).unwrap_err().contains("jamais root"), "{user:?}");
        }
        let plan = install_plan(&Spec { user: Some("talixio".into()), ..spec(Kind::SystemdSystem) }).unwrap();
        let (path, unit) = plan.file.unwrap();
        assert_eq!(path, PathBuf::from("/etc/systemd/system/talixio-agent.service"));
        assert!(unit.contains("\nUser=talixio\n") && unit.contains("WantedBy=multi-user.target\n"));
        // le compte est dit avant toute ligne portant un chemin : aucun chemin ne peut l'avaler
        assert!(unit.find("User=talixio").unwrap() < unit.find("ExecStart=").unwrap());
        let asked = Spec { user: Some("talixio".into()), env_file: Some("/srv/talixio/cles.env\\".into()), ..spec(Kind::SystemdSystem) };
        assert!(install_plan(&asked).is_err(), "une barre oblique inversée joindrait deux lignes de l'unité");
        assert!(install_plan(&Spec { workdir: "/srv/talixio\\".into(), ..spec(Kind::SystemdUser) }).is_err());
        assert!(install_plan(&Spec { home: PathBuf::new(), ..spec(Kind::SystemdUser) }).is_err(), "sans dossier personnel, pas de chemin relatif");
        assert_eq!(plan.commands[0], words(&["systemctl", "daemon-reload"]));
        assert!(plan.optional.is_empty());
    }

    #[test]
    fn un_chemin_ne_reecrit_pas_le_service() {
        for bad in ["relatif/talixio.toml", "/srv/a\nExecStartPre=/bin/sh", "/srv/a\" --autre \"", "/srv/%h/t.toml", "/srv/$HOME/t.toml", "/srv/<a>/t.toml", "/srv/a;b"] {
            for kind in [Kind::SystemdUser, Kind::Launchd, Kind::WindowsTask] {
                assert!(install_plan(&Spec { config: bad.into(), ..spec(kind) }).is_err(), "{bad}");
            }
        }
        // un espace est permis : les chemins sont entre guillemets
        let plan = install_plan(&Spec { config: "/srv/mon projet/talixio.toml".into(), ..spec(Kind::SystemdUser) }).unwrap();
        assert!(plan.file.unwrap().1.contains("--config \"/srv/mon projet/talixio.toml\""));
        assert!(install_plan(&Spec { env_file: Some("/srv/cles.env".into()), ..spec(Kind::Launchd) }).unwrap_err().contains("Linux"));
    }

    #[test]
    fn launchd_et_windows() {
        let plan = install_plan(&spec(Kind::Launchd)).unwrap();
        let (path, plist) = plan.file.unwrap();
        assert_eq!(path, PathBuf::from("/home/ada/Library/LaunchAgents/com.talixio.agent.plist"));
        assert!(plist.contains("<string>/home/ada/.local/bin/talixio-agent</string>") && plist.contains("<key>RunAtLoad</key><true/>"));
        assert_eq!(plan.commands, vec![words(&["launchctl", "bootstrap", "gui/501", "/home/ada/Library/LaunchAgents/com.talixio.agent.plist"])]);
        let plan = install_plan(&spec(Kind::WindowsTask)).unwrap();
        assert!(plan.file.is_none());
        assert_eq!(plan.commands[0].last().unwrap(), "\"/home/ada/.local/bin/talixio-agent\" root --config \"/srv/talixio/talixio.toml\" --workdir \"/srv/talixio\"");
        assert!(plan.commands[0].contains(&"LIMITED".to_string()));
    }

    #[test]
    fn desinstaller_ne_touche_que_le_service() {
        let plan = uninstall_plan(&spec(Kind::SystemdUser));
        assert_eq!(plan.file.unwrap().0, PathBuf::from("/home/ada/.config/systemd/user/talixio-agent.service"));
        assert_eq!(plan.optional, vec![words(&["systemctl", "--user", "disable", "--now", "talixio-agent.service"])]);
        assert_eq!(uninstall_plan(&spec(Kind::WindowsTask)).commands, vec![words(&["schtasks", "/Delete", "/TN", "TalixioAgent", "/F"])]);
        assert_eq!(status_command(&spec(Kind::Launchd)), words(&["launchctl", "print", "gui/501/com.talixio.agent"]));
    }
}
