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

/// Installation en root sans `--user` : le compte dédié et les emplacements du système.
/// L'exécutable, les paquets et la configuration sont à root ; le compte ne possède que son
/// dossier d'état et de travail.
pub const ACCOUNT: &str = "talixio-agent";
pub const SYSTEM_DIR: &str = "/opt/talixio";
pub const SYSTEM_EXE: &str = "/opt/talixio/talixio-agent";
pub const SYSTEM_PACKS: &str = "/opt/talixio/packs";
pub const SYSTEM_CONFIG_DIR: &str = "/etc/talixio";
pub const SYSTEM_CONFIG: &str = "/etc/talixio/talixio.toml";
pub const SYSTEM_STATE: &str = "/var/lib/talixio";

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
    /// Unité du système sous le compte dédié : les emplacements du système, à root, en lecture seule.
    pub dedicated: bool,
}

/// Ce que l'installation fait : un fichier à écrire (s'il y en a un), des commandes à lancer.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub file: Option<(PathBuf, String)>,
    pub commands: Vec<Vec<String>>,
    /// Commandes dont l'échec n'empêche pas le service de tourner.
    pub optional: Vec<Vec<String>>,
    pub notes: Vec<String>,
    /// Compte dédié : ce qui est mis en place avant d'écrire le service.
    pub setup: Vec<String>,
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
                if spec.dedicated {
                    // ce qui est à root reste hors d'écriture même si un droit de fichier venait à changer
                    unit.push_str(&format!("ProtectSystem=full\nReadOnlyPaths={SYSTEM_DIR} {SYSTEM_CONFIG_DIR}\n"));
                    plan.setup = vec![
                        format!("compte « {ACCOUNT} » créé s'il manque : sans mot de passe, sans shell, sans droit d'administration"),
                        format!("{SYSTEM_EXE} : copie de cet exécutable (à root)"),
                        format!("{SYSTEM_PACKS} : dossier des paquets (à root) ; `sudo talixio-agent pack install <nom>` y installe"),
                        format!("{SYSTEM_CONFIG} : la configuration (à root, lisible par le compte), avec packs_dir = \"{SYSTEM_PACKS}\" ; l'ancienne est gardée en talixio.toml.ancien"),
                        format!("{SYSTEM_STATE} : dossier d'état et de travail, le seul qui appartienne au compte"),
                    ];
                    plan.notes.push(format!("Modifier la configuration : sudo, puis `sudo systemctl restart {NAME}`. Les chemins relatifs de la configuration partent de {SYSTEM_STATE}."));
                    plan.notes.push(format!("N'ajoutez pas le compte {ACCOUNT} à un groupe (docker, sudo…) : le groupe docker vaut root, et le maître pourrait alors tout modifier. Les cibles `docker` ne sont donc pas pour ce mode."));
                    plan.notes.push(format!("Le maître ne peut modifier ni son exécutable, ni sa configuration, ni ses paquets. Les clés saisies dans l'interface restent dans son dossier d'état ({SYSTEM_STATE})."));
                }
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
    for step in &plan.setup {
        out.push_str(&format!("Mise en place : {step}\n"));
    }
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

/// Les numéros (compte, groupe) d'un compte de la machine.
#[cfg(unix)]
pub fn account_ids(name: &str) -> Option<(u32, u32)> {
    let name = std::ffi::CString::new(name).ok()?;
    // SAFETY: chaîne terminée par un zéro ; le résultat, lu aussitôt, n'est pas gardé.
    unsafe {
        let entry = libc::getpwnam(name.as_ptr());
        if entry.is_null() { None } else { Some(((*entry).pw_uid, (*entry).pw_gid)) }
    }
}

/// La configuration telle qu'elle sera posée : `packs_dir` y désigne le dossier des paquets du
/// système. Ajouté en tête s'il manque ; refusé s'il désigne un autre dossier.
pub fn with_system_packs(text: &str) -> Result<String, String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| format!("configuration invalide : {e}"))?;
    match table.get("packs_dir") {
        None => Ok(format!("# Ajouté par `talixio-agent service install` : les paquets installés par root.\npacks_dir = \"{SYSTEM_PACKS}\"\n\n{text}")),
        Some(toml::Value::String(dir)) if dir == SYSTEM_PACKS => Ok(text.to_string()),
        Some(_) => Err(format!("la configuration désigne un autre dossier de paquets : retirez `packs_dir`, ou écrivez packs_dir = \"{SYSTEM_PACKS}\"")),
    }
}

/// Un dossier du système : créé s'il manque, à root, que root seul peut modifier. Un lien, un
/// dossier qui appartient à un autre compte ou que d'autres peuvent écrire, est refusé.
#[cfg(unix)]
fn root_dir(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let at = |e: std::io::Error| format!("{} : {e}", path.display());
    if std::fs::symlink_metadata(path).is_err() {
        std::fs::create_dir(path).map_err(at)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(at)?;
    }
    let meta = std::fs::symlink_metadata(path).map_err(at)?;
    if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
        return Err(format!("{} doit être un vrai dossier, à root, que root seul peut modifier (chown root, chmod 755)", path.display()));
    }
    Ok(())
}

/// Tout ce qu'un dossier du système contient est à root, sans lien, et fermé en écriture aux
/// autres : rien de ce que le maître lira n'a pu être posé ni ne peut être changé par un tiers.
#[cfg(unix)]
fn root_only_tree(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{} : {e}", dir.display()))? {
        let path = entry.map_err(|e| format!("{} : {e}", dir.display()))?.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("{} : {e}", path.display()))?;
        if meta.file_type().is_symlink() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err(format!("{} : un lien, ou un fichier qui n'est pas à root seul. Rien de tel dans les dossiers du maître : retirez-le, ou rendez-le à root (chown root, chmod go-w)", path.display()));
        }
        if meta.is_dir() {
            root_only_tree(&path)?;
        }
    }
    Ok(())
}

/// Les dossiers au-dessus d'un emplacement du système sont à root, que root seul peut modifier.
#[cfg(unix)]
fn root_above(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    for part in path.ancestors().skip(1) {
        let meta = std::fs::metadata(part).map_err(|e| format!("{} : {e}", part.display()))?;
        if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err(format!("{} peut être modifié par un autre compte que root : pas d'installation du système en dessous", part.display()));
        }
    }
    Ok(())
}

/// Écrit un fichier à root dans un dossier à root : à côté d'abord, puis par renommage.
#[cfg(unix)]
fn root_file(path: &Path, bytes: &[u8], mode: u32, gid: u32) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let at = |e: std::io::Error| format!("{} : {e}", path.display());
    let beside = path.with_extension("nouveau");
    let _ = std::fs::remove_file(&beside);
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&beside).map_err(at)?;
    file.write_all(bytes).map_err(at)?;
    std::os::unix::fs::fchown(&file, Some(0), Some(gid)).map_err(at)?;
    file.set_permissions(std::fs::Permissions::from_mode(mode)).map_err(at)?;
    file.sync_all().map_err(at)?;
    std::fs::rename(&beside, path).map_err(at)
}

/// Un compte `talixio-agent` qui existait déjà n'est accepté que s'il ressemble à celui que
/// l'installation aurait créé : son propre groupe, aucun autre, pas de shell.
#[cfg(unix)]
fn account_is_plain(uid: u32, gid: u32) -> Result<(), String> {
    let refuse = |why: &str| Err(format!("le compte « {ACCOUNT} » existe déjà et {why} : le maître aurait plus de droits que prévu. Corrigez-le, ou retirez-le (userdel {ACCOUNT}) et relancez"));
    if uid == 0 || gid == 0 {
        return refuse("a les droits de root");
    }
    let name = std::ffi::CString::new(ACCOUNT).map_err(|e| e.to_string())?;
    // SAFETY: chaînes terminées par un zéro ; les résultats, lus aussitôt, ne sont pas gardés ;
    // le tableau des groupes a la taille annoncée.
    unsafe {
        let entry = libc::getpwnam(name.as_ptr());
        if entry.is_null() {
            return refuse("n'est plus lisible");
        }
        let shell = std::ffi::CStr::from_ptr((*entry).pw_shell).to_string_lossy().into_owned();
        if !(shell.ends_with("/nologin") || shell.ends_with("/false")) {
            return refuse("peut ouvrir une session (shell)");
        }
        let mut groups = [0 as libc::gid_t; 64];
        let mut count = groups.len() as libc::c_int;
        let found = libc::getgrouplist(name.as_ptr(), gid as _, groups.as_mut_ptr().cast(), &mut count);
        if found < 0 || groups[..count.max(0) as usize].iter().any(|g| *g != gid) {
            return refuse("appartient à d'autres groupes que le sien (docker, sudo… valent root)");
        }
        let group = libc::getgrgid(gid);
        if group.is_null() || std::ffi::CStr::from_ptr((*group).gr_name).to_bytes() != ACCOUNT.as_bytes() || !(*(*group).gr_mem).is_null() {
            return refuse("partage son groupe avec d'autres comptes");
        }
    }
    Ok(())
}

/// Essaie une configuration sous le compte, comme le maître la chargera. Le process lancé
/// appartient au compte, donc au maître s'il est compromis : il ne reçoit de root ni
/// environnement, ni terminal, ni fichier ouvert, et root ne l'attend pas indéfiniment.
#[cfg(unix)]
fn check_as_account(trial: &Path, uid: u32, gid: u32) -> Result<(), String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let journal = trial.with_extension("journal");
    let _ = std::fs::remove_file(&journal);
    let out = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&journal).map_err(|e| format!("{} : {e}", journal.display()))?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(SYSTEM_EXE);
    command
        .args(["check", "--config"])
        .arg(trial)
        .current_dir(SYSTEM_STATE)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", SYSTEM_STATE)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .uid(uid)
        .gid(gid);
    // SAFETY: entre fork et exec, seulement des appels système sans allocation.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            for fd in 3..1024 {
                libc::close(fd);
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|e| format!("{SYSTEM_EXE} : {e}"))?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break Some(status),
            None if started.elapsed() > std::time::Duration::from_secs(30) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    };
    let mut said = Vec::new();
    if let Ok(file) = std::fs::File::open(&journal) {
        let _ = file.take(4000).read_to_end(&mut said);
    }
    let _ = std::fs::remove_file(&journal);
    // ce texte vient d'un process du compte : nettoyé avant d'atteindre le terminal de root
    let said = crate::clean(&String::from_utf8_lossy(&said), 4000, true);
    match status {
        Some(status) if status.success() => Ok(()),
        Some(_) => Err(format!("la configuration n'est pas acceptée sous le compte {ACCOUNT} ; rien n'a été changé.\n{said}")),
        None => Err(format!("la vérification de la configuration sous le compte {ACCOUNT} n'a pas répondu en 30 secondes ; rien n'a été changé")),
    }
}

/// Compte dédié : crée le compte, pose l'exécutable, la configuration et le dossier des paquets
/// à root, et le dossier d'état au compte. À lancer en root. `config` : la configuration à poser ;
/// sans elle, celle qui est déjà en place est gardée. Tous les contrôles passent avant la
/// première écriture qui compte. Rend le numéro du compte.
#[cfg(unix)]
pub fn setup_dedicated(exe: &Path, config: Option<&Path>) -> Result<u32, String> {
    use std::os::unix::fs::MetadataExt;
    for place in [SYSTEM_DIR, SYSTEM_CONFIG_DIR, SYSTEM_STATE] {
        root_above(Path::new(place))?;
    }
    let (uid, gid) = match account_ids(ACCOUNT) {
        Some(ids) => ids,
        None => {
            // sous sudo, le PATH n'a pas toujours les dossiers de l'administration
            let first = |names: &[&str], or: &str| names.iter().find(|p| Path::new(p).exists()).unwrap_or(&or).to_string();
            let useradd = first(&["/usr/sbin/useradd", "/sbin/useradd", "/usr/bin/useradd"], "useradd");
            let nologin = first(&["/usr/sbin/nologin", "/sbin/nologin"], "/bin/false");
            // pas de dossier personnel : rien de ce que le maître écrit n'est lu comme réglage du compte
            run(&words(&[&useradd, "--system", "--user-group", "--home-dir", "/nonexistent", "--no-create-home", "--shell", &nologin, ACCOUNT]))?;
            account_ids(ACCOUNT).ok_or(format!("le compte « {ACCOUNT} » n'a pas pu être créé"))?
        }
    };
    account_is_plain(uid, gid)?;
    for dir in [SYSTEM_DIR, SYSTEM_PACKS, SYSTEM_CONFIG_DIR] {
        let dir = Path::new(dir);
        root_dir(dir)?;
        root_only_tree(dir)?;
    }
    // le dossier du compte : root le crée et le lui donne, puis n'y écrit plus jamais
    let state = Path::new(SYSTEM_STATE);
    let at = |e: std::io::Error| format!("{SYSTEM_STATE} : {e}");
    if std::fs::symlink_metadata(state).is_err() {
        std::fs::create_dir(state).map_err(at)?;
        std::os::unix::fs::lchown(state, Some(uid), Some(gid)).map_err(at)?;
    }
    let meta = std::fs::symlink_metadata(state).map_err(at)?;
    if !meta.is_dir() || meta.uid() != uid {
        return Err(format!("{SYSTEM_STATE} doit être un vrai dossier qui appartient à {ACCOUNT}"));
    }
    // fermé aux autres, quoi que le compte en ait fait depuis
    crate::os::set_mode(state, 0o700).map_err(|e| e.to_string())?;
    // la configuration, lue avant toute écriture
    let dest = Path::new(SYSTEM_CONFIG);
    let source = match config {
        Some(path) => path,
        None if dest.is_file() => dest,
        None => return Err(format!("aucune configuration dans {SYSTEM_CONFIG} : donnez la vôtre avec `--config <fichier>`")),
    };
    let text = std::fs::read_to_string(source).map_err(|e| format!("{} : {e}", source.display()))?;
    let text = with_system_packs(text.trim_start_matches('\u{feff}'))?;
    // l'exécutable : une copie à root, que le compte ne peut pas remplacer
    let target = Path::new(SYSTEM_EXE);
    if exe.canonicalize().ok() != target.canonicalize().ok() {
        let bytes = std::fs::read(exe).map_err(|e| format!("{} : {e}", exe.display()))?;
        root_file(target, &bytes, 0o755, 0)?;
    }
    // la configuration : posée à côté, essayée sous le compte, puis mise en place
    let trial = dest.with_extension("essai");
    root_file(&trial, text.as_bytes(), 0o640, gid)?;
    let checked = check_as_account(&trial, uid, gid);
    let _ = std::fs::remove_file(&trial);
    checked?;
    if dest.is_file() && std::fs::read_to_string(dest).ok().as_deref() != Some(text.as_str()) {
        std::fs::rename(dest, dest.with_extension("toml.ancien")).map_err(|e| format!("{SYSTEM_CONFIG} : {e}"))?;
    }
    // même contenu : les droits sont tout de même remis d'aplomb
    root_file(dest, text.as_bytes(), 0o640, gid)?;
    Ok(uid)
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
            dedicated: false,
        }
    }

    // chemins d'Unix : sous Windows, ils ne sont pas absolus
    #[cfg(unix)]
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

    // chemins d'Unix : sous Windows, ils ne sont pas absolus
    #[cfg(unix)]
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

    // chemins d'Unix : sous Windows, ils ne sont pas absolus
    #[cfg(unix)]
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

    // chemins d'Unix : sous Windows, ils ne sont pas absolus
    #[cfg(unix)]
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

    #[cfg(windows)]
    #[test]
    fn tache_planifiee_sous_windows() {
        let ici = Spec {
            exe: r"C:\Users\ada\AppData\Local\Talixio\talixio-agent.exe".into(),
            config: r"C:\Talixio\talixio.toml".into(),
            workdir: r"C:\Talixio".into(),
            home: r"C:\Users\ada".into(),
            ..spec(Kind::WindowsTask)
        };
        let plan = install_plan(&ici).unwrap();
        assert!(plan.file.is_none());
        assert_eq!(
            plan.commands[0].last().unwrap(),
            r#""C:\Users\ada\AppData\Local\Talixio\talixio-agent.exe" root --config "C:\Talixio\talixio.toml" --workdir "C:\Talixio""#
        );
        assert!(plan.commands[0].contains(&"LIMITED".to_string()));
        for bad in [r"relatif\talixio.toml", r#"C:\a" --autre ""#, r"C:\a&calc", r"C:\%TEMP%\t.toml", r"C:\a|b"] {
            assert!(install_plan(&Spec { config: bad.into(), ..ici.clone() }).is_err(), "{bad}");
        }
    }

    // chemins d'Unix : sous Windows, ils ne sont pas absolus
    #[cfg(unix)]
    #[test]
    fn compte_dedie() {
        let asked = Spec {
            user: Some(ACCOUNT.into()),
            dedicated: true,
            exe: SYSTEM_EXE.into(),
            config: SYSTEM_CONFIG.into(),
            workdir: SYSTEM_STATE.into(),
            ..spec(Kind::SystemdSystem)
        };
        let plan = install_plan(&asked).unwrap();
        let (_, unit) = plan.file.clone().unwrap();
        assert!(unit.contains("\nUser=talixio-agent\n") && unit.contains("\nReadOnlyPaths=/opt/talixio /etc/talixio\n") && unit.contains("\nProtectSystem=full\n"));
        assert!(unit.contains("ExecStart=\"/opt/talixio/talixio-agent\" root --config \"/etc/talixio/talixio.toml\"\n") && unit.contains("WorkingDirectory=/var/lib/talixio\n"));
        assert_eq!(plan.setup.len(), 5);
        assert!(describe(&plan, false).starts_with("Mise en place : compte « talixio-agent »"));
        // sans compte dédié, rien de tout cela
        let plain = install_plan(&Spec { user: Some("ada".into()), ..spec(Kind::SystemdSystem) }).unwrap();
        assert!(plain.setup.is_empty() && !plain.file.unwrap().1.contains("ReadOnlyPaths"));
    }

    #[test]
    fn configuration_du_systeme_et_paquets() {
        let posed = with_system_packs("listen = \"127.0.0.1:7700\"\n\n[models.a]\nprovider = \"x\"\n").unwrap();
        assert!(posed.contains("packs_dir = \"/opt/talixio/packs\"\n"));
        let table: toml::Table = toml::from_str(&posed).unwrap();
        assert_eq!(table["packs_dir"].as_str(), Some(SYSTEM_PACKS));
        assert!(table.contains_key("models"), "le reste est intact");
        // déjà le bon dossier : rien ne change ; un autre : refus
        let same = format!("packs_dir = \"{SYSTEM_PACKS}\"\n");
        assert_eq!(with_system_packs(&same).unwrap(), same);
        assert!(with_system_packs("packs_dir = \"/home/ada/packs\"\n").unwrap_err().contains("autre dossier"));
        assert!(with_system_packs("packs_dir = 3\n").is_err() && with_system_packs("[[[").is_err());
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
