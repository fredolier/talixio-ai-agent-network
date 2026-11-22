//! Lancement d'un agent enfant. Pour l'instant : un process sur la même machine.

use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

use crate::config::Bootstrap;
use crate::node::Node;

/// Variables d'environnement transmises à un process lancé par un agent. Rien d'autre ne
/// passe : en particulier, aucune clé d'API du maître.
pub const SAFE_ENV: [&str; 13] = [
    "PATH", "HOME", "USER", "LANG", "LC_ALL", "TZ", "TMPDIR", "TEMP", "TMP", "SystemRoot", "USERPROFILE", "PATHEXT",
    "ComSpec",
];

pub fn apply_safe_env(command: &mut Command) {
    command.env_clear();
    for name in SAFE_ENV {
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
    // git exécute ce que sa configuration lui dit : celle de la machine et du compte est ignorée
    command.env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" });
}

/// Lance le même exécutable en mode enfant. Les consignes, jeton compris, passent par
/// l'entrée standard : elles n'apparaissent ni dans la ligne de commande ni dans l'environnement.
pub async fn spawn_local(node: &Node, boot: &Bootstrap, envelope: &[u8]) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("exécutable introuvable : {e}"))?;
    let logs = node.state_dir.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("{} : {e}", logs.display()))?;
    let log_path = logs.join(format!("{}-{}.log", boot.name, boot.agent_id));
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| format!("{} : {e}", log_path.display()))?;

    let mut command = Command::new(exe);
    command
        .arg("child")
        .arg("--state-dir")
        .arg(&node.state_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(log)
        .kill_on_drop(true);
    apply_safe_env(&mut command);

    let mut child = command.spawn().map_err(|e| format!("lancement de l'agent : {e}"))?;
    let mut stdin = child.stdin.take().ok_or("entrée standard de l'enfant indisponible")?;
    stdin.write_all(envelope).await.map_err(|e| format!("transmission des consignes : {e}"))?;
    drop(stdin);
    Ok(child)
}

// ---------------------------------------------------------------------------------------
// Déploiement hors de la machine du parent
// ---------------------------------------------------------------------------------------

use crate::config::Target;

/// Lance un enfant : sur cette machine, ou sur la cible donnée.
pub async fn launch(node: &Node, boot: &Bootstrap, envelope: &[u8], target: Option<&Target>) -> Result<Child, String> {
    match target {
        None => spawn_local(node, boot, envelope).await,
        Some(target) if target.kind == "ssh" => spawn_ssh(node, boot, envelope, target).await,
        Some(target) => spawn_docker(node, boot, envelope, target).await,
    }
}

fn log_file(node: &Node, boot: &Bootstrap) -> Result<std::fs::File, String> {
    let logs = node.state_dir.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("{} : {e}", logs.display()))?;
    let path = logs.join(format!("{}-{}.log", boot.name, boot.agent_id));
    std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("{} : {e}", path.display()))
}

async fn feed(child: &mut Child, payload: &[u8]) -> Result<(), String> {
    let mut stdin = child.stdin.take().ok_or("entrée standard indisponible")?;
    stdin.write_all(payload).await.map_err(|e| format!("transmission : {e}"))?;
    Ok(())
}

/// `ssh` vers la cible, sans rien d'interactif : la machine doit déjà être connue
/// (`known_hosts`), sinon la connexion est refusée — jamais de confiance à la première vue.
fn ssh(target: &Target, remote_command: &str) -> Command {
    let mut command = Command::new(target.program.clone().unwrap_or_else(|| if cfg!(windows) { r"C:\Windows\System32\OpenSSH\ssh.exe" } else { "/usr/bin/ssh" }.into()));
    // aucun fichier de réglages : ni ceux du compte ni ceux de la machine ne changent ce qui est lancé
    command.args(["-F", if cfg!(windows) { "NUL" } else { "/dev/null" }, "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=10"]);
    command.args(["-o", "ServerAliveInterval=15", "-o", "ForwardAgent=no", "-o", "ClearAllForwardings=yes", "-T"]);
    if let Some(port) = target.port {
        command.arg("-p").arg(port.to_string());
    }
    if let Some(identity) = &target.identity_file {
        command.arg("-i").arg(identity).args(["-o", "IdentitiesOnly=yes"]);
    }
    command.arg(format!("{}@{}", target.user.as_deref().unwrap_or(""), target.host.as_deref().unwrap_or("")));
    command.arg(remote_command).kill_on_drop(true);
    apply_safe_env(&mut command);
    command
}

/// Exécute une commande de préparation sur la cible et renvoie sa sortie. `input` : ce qu'elle lit.
async fn ssh_run(target: &Target, remote_command: &str, input: Option<Vec<u8>>) -> Result<String, String> {
    let mut command = ssh(target, remote_command);
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("ssh : {e}"))?;
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    // une machine distante peut ne rien lire, ou répondre sans fin : l'envoi et les lectures
    // avancent ensemble, sorties bornées, sous un seul délai
    let exchange = async {
        let send = async {
            if let (Some(mut stdin), Some(input)) = (stdin, input) {
                let _ = stdin.write_all(&input).await;
            }
        };
        let (_, (out, _), (err, _)) = tokio::join!(send, crate::tools::read_capped(stdout), crate::tools::read_capped(stderr));
        (child.wait().await, out, err)
    };
    let (status, out, err) =
        tokio::time::timeout(std::time::Duration::from_secs(120), exchange).await.map_err(|_| "ssh : délai dépassé".to_string())?;
    if !status.map_err(|e| format!("ssh : {e}"))?.success() {
        let detail = crate::clean(&String::from_utf8_lossy(&err), 300, false);
        return Err(format!("la machine distante a refusé : {detail}"));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn remote_paths(target: &Target, id: &str) -> (String, String, String) {
    let dir = target.remote_dir.as_deref().unwrap_or("").trim_end_matches('/').to_string();
    (format!("{dir}/talixio-agent-{id}"), format!("{dir}/state"), dir)
}

/// Dépose l'exécutable sur la machine distante et y lance l'agent. Tout passe par `ssh` :
/// l'exécutable et les consignes voyagent dans son canal chiffré, rien n'est écrit en clair
/// hors du dossier réservé aux agents.
async fn spawn_ssh(node: &Node, boot: &Bootstrap, envelope: &[u8], target: &Target) -> Result<Child, String> {
    let (binary, state, dir) = remote_paths(target, &boot.agent_id);
    // le dossier doit être à ce compte, et à lui seul : personne d'autre n'y prépare ni n'y remplace rien
    let prepare = format!(
        "umask 077 && mkdir -p {dir} {state} && [ -O {dir} ] && [ ! -L {dir} ] && [ -O {state} ] && [ ! -L {state} ] && chmod 700 {dir} {state} && uname -sm"
    );
    let system = ssh_run(target, &prepare, None).await?;
    let expected = format!("{} {}", if cfg!(target_os = "macos") { "Darwin" } else { "Linux" }, std::env::consts::ARCH);
    let system = crate::clean(system.trim(), 80, false).replace("arm64", "aarch64");
    if system != expected {
        return Err(format!("la machine distante est « {system} », cet exécutable est fait pour « {expected} »"));
    }
    let exe = std::env::current_exe().map_err(|e| format!("exécutable introuvable : {e}"))?;
    let bytes = tokio::fs::read(&exe).await.map_err(|e| format!("lecture de l'exécutable : {e}"))?;
    ssh_run(target, &format!("umask 077 && cat > {binary} && chmod 700 {binary}"), Some(bytes)).await?;

    // les erreurs de l'agent vont dans un journal distant : sa vie ne dépend pas de la session ssh
    // son numéro de process est noté : c'est ce qui permettra de le tuer, pas seulement de le lâcher
    let id = &boot.agent_id;
    let run = format!("umask 077 && echo $$ > {state}/{id}.pid && exec {binary} child --state-dir {state} --log-file {state}/{id}.log 2>>{state}/{id}.log");
    let mut command = ssh(target, &run);
    command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(log_file(node, boot)?);
    let mut child = command.spawn().map_err(|e| format!("ssh : {e}"))?;
    send_bootstrap(&mut child, envelope).await?;
    Ok(child)
}

async fn send_bootstrap(child: &mut Child, envelope: &[u8]) -> Result<(), String> {
    tokio::time::timeout(std::time::Duration::from_secs(30), feed(child, envelope)).await.map_err(|_| "transmission des consignes : délai dépassé".to_string())?
}

fn docker(target: &Target) -> Command {
    let mut command = Command::new(target.program.clone().unwrap_or_else(|| if cfg!(windows) { r"C:\Program Files\Docker\Docker\resources\bin\docker.exe" } else { "/usr/bin/docker" }.into()));
    command.kill_on_drop(true);
    apply_safe_env(&mut command);
    command
}

/// Lance l'agent dans un conteneur jetable et fermé : aucun privilège, système en lecture
/// seule, ressources bornées, et seuls les dossiers montrés par la cible.
pub fn docker_args(id: &str, target: &Target, exe: &std::path::Path) -> Vec<String> {
    let mut args: Vec<String> = ["run", "--interactive", "--rm", "--init", "--pull=never", "--name"].map(String::from).into();
    args.push(format!("talixio-{id}"));
    args.extend(["--cap-drop=ALL", "--security-opt=no-new-privileges", "--read-only"].map(String::from));
    args.extend(["--pids-limit=512".to_string(), format!("--memory={}", target.memory.as_deref().unwrap_or("1g")), "--tmpfs=/tmp".to_string()]);
    if let Some(seccomp) = &target.seccomp {
        args.push(format!("--security-opt=seccomp={}", seccomp.display()));
    }
    match &target.network {
        // le maître est dans ce réseau : l'agent le joint par son adresse là-bas
        Some(network) => args.push(format!("--network={network}")),
        None => args.push("--add-host=host.docker.internal:host-gateway".into()),
    }
    // le dossier d'état vit en mémoire et disparaît avec le conteneur
    let user = target.user.as_deref().unwrap_or("65534:65534");
    args.push(format!("--user={user}"));
    args.push(format!("--tmpfs=/state:mode=700,uid={}", user.split(':').next().unwrap_or("65534")));
    if !target.agent_in_image {
        args.extend(["--volume".into(), format!("{}:/talixio-agent:ro", exe.display())]);
    }
    for mount in &target.mounts {
        match (&mount.host, &mount.volume) {
            (Some(host), _) => {
                let mode = if mount.write { "rw" } else { "ro" };
                args.extend(["--volume".into(), format!("{}:{}:{mode}", host.display(), mount.container.display())]);
            }
            (None, Some(volume)) => {
                // jamais créé au passage (« volume-nocopy », et un volume absent fait échouer le sous-dossier)
                let mut spec = format!("type=volume,src={volume},dst={},volume-nocopy", mount.container.display());
                if let Some(subpath) = &mount.subpath {
                    spec.push_str(&format!(",volume-subpath={subpath}"));
                }
                if !mount.write {
                    spec.push_str(",readonly");
                }
                args.extend(["--mount".into(), spec]);
            }
            (None, None) => {}
        }
    }
    args.push("--entrypoint=/talixio-agent".into());
    args.push(target.image.clone().unwrap_or_default());
    args.extend(["child", "--state-dir", "/state"].map(String::from));
    args
}

async fn spawn_docker(node: &Node, boot: &Bootstrap, envelope: &[u8], target: &Target) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("exécutable introuvable : {e}"))?;
    let mut command = docker(target);
    command.args(docker_args(&boot.agent_id, target, &exe));
    command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(log_file(node, boot)?);
    let mut child = command.spawn().map_err(|e| format!("docker : {e}"))?;
    send_bootstrap(&mut child, envelope).await?;
    Ok(child)
}

/// Tue l'agent s'il tourne encore, puis retire de la cible tout ce que l'agent y a laissé.
pub async fn uninstall(target: &Target, id: &str) -> Result<(), String> {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("identifiant invalide".into());
    }
    if target.kind == "ssh" {
        let (binary, state, _) = remote_paths(target, id);
        // un agent qui n'obéit plus ne survit pas à sa désinstallation : lui et son groupe sont tués
        let remove = format!(
            "p=$(cat {state}/{id}.pid 2>/dev/null); case \"$p\" in ''|*[!0-9]*) ;; *) kill -9 -- \"-$p\" \"$p\" 2>/dev/null;; esac; \
             rm -f {binary} {state}/{id}.pid {state}/{id}.log {state}/agents/{id}.db {state}/agents/{id}.db-wal {state}/agents/{id}.db-shm"
        );
        ssh_run(target, &remove, None).await.map(|_| ())
    } else {
        let mut command = docker(target);
        command.args(["rm", "--force"]).arg(format!("talixio-{id}")).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        match tokio::time::timeout(std::time::Duration::from_secs(30), command.status()).await {
            Ok(result) => result.map(|_| ()).map_err(|e| format!("docker : {e}")),
            Err(_) => Err("docker : délai dépassé".into()),
        }
    }
}

/// Fait tourner un journal trop gros : son contenu passe dans `<journal>.1` (l'ancien `.1` est
/// perdu), puis il est vidé. Celui qui écrit dedans n'a rien à savoir : il écrit en ajout.
pub fn rotate_log(path: &std::path::Path, max_bytes: u64) {
    let too_big = std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() > max_bytes);
    if !too_big {
        return;
    }
    let mut old = path.as_os_str().to_owned();
    old.push(".1");
    let _ = std::fs::copy(path, &old);
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_len(0);
    }
}

/// Tient les journaux des enfants dans la limite fixée : rotation, et oubli des journaux
/// auxquels plus personne n'écrit depuis une semaine.
pub async fn keep_logs_small(dir: std::path::PathBuf, max_bytes: u64) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(20));
    loop {
        tick.tick().await;
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let stale = entry.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok());
            if stale.is_some_and(|age| age > std::time::Duration::from_secs(7 * 24 * 3600)) {
                let _ = std::fs::remove_file(&path);
            } else if path.extension().is_some_and(|e| e == "log") {
                rotate_log(&path, max_bytes);
            }
        }
    }
}

/// Le journal de l'agent lui-même, quand c'est lui qui le tient (déployé par SSH).
pub async fn keep_own_log_small(path: std::path::PathBuf, max_bytes: u64) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(20));
    loop {
        tick.tick().await;
        rotate_log(&path, max_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(toml: &str) -> Result<Target, String> {
        let target: Target = toml::from_str(toml).map_err(|e| e.to_string())?;
        target.validate().map_err(|e| e.to_string())?;
        Ok(target)
    }

    #[test]
    fn un_journal_trop_gros_tourne() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("a.log");
        std::fs::write(&log, vec![b'x'; 2000]).unwrap();
        rotate_log(&log, 5000);
        assert_eq!(std::fs::metadata(&log).unwrap().len(), 2000);
        rotate_log(&log, 1000);
        assert_eq!(std::fs::metadata(&log).unwrap().len(), 0);
        assert_eq!(std::fs::metadata(dir.path().join("a.log.1")).unwrap().len(), 2000);
    }

    #[test]
    fn un_conteneur_est_ferme_et_ne_montre_que_ses_montages() {
        // Côté machine, les chemins suivent le système ; côté conteneur, toujours Linux.
        let (data, exe) = if cfg!(windows) { ("C:/srv/data", "C:/opt/talixio-agent") } else { ("/srv/data", "/opt/talixio-agent") };
        let t = target(&format!("kind = 'docker'\nimage = 'debian:12-slim'\nuser = '1000:1000'\nmounts = [{{ host = '{data}', container = '/data' }}]")).unwrap();
        let args = docker_args("abc", &t, std::path::Path::new(exe));
        let (mount, binary) = (format!("{data}:/data:ro"), format!("{exe}:/talixio-agent:ro"));
        for wanted in ["--cap-drop=ALL", "--security-opt=no-new-privileges", "--read-only", "--user=1000:1000", mount.as_str(), binary.as_str(), "--pull=never"] {
            assert!(args.iter().any(|a| a == wanted), "{wanted} manque dans {args:?}");
        }
        assert!(!args.iter().any(|a| a.contains("privileged") || a.contains("docker.sock") || a.contains("seccomp")));
        assert!(args.iter().any(|a| a == "--memory=1g"));
        // mémoire et filtre d'appels système de la cible : rien d'autre ne s'ajoute
        let profile = if cfg!(windows) { "C:/opt/chromium-seccomp.json" } else { "/opt/chromium-seccomp.json" };
        let t = target(&format!("kind = 'docker'\nimage = 'talixio-navigateur'\nuser = '65532:65532'\nmemory = '2g'\nseccomp = '{profile}'")).unwrap();
        let args = docker_args("abc", &t, std::path::Path::new(exe));
        let filter = format!("--security-opt=seccomp={profile}");
        for wanted in ["--memory=2g", filter.as_str(), "--cap-drop=ALL", "--security-opt=no-new-privileges", "--read-only"] {
            assert!(args.iter().any(|a| a == wanted), "{wanted} manque dans {args:?}");
        }
    }

    #[test]
    fn un_maitre_en_conteneur_lance_un_conteneur_voisin() {
        // ni chemin de la machine, ni passage par elle : réseau du maître, agent dans l'image, dossier pris dans le volume
        let t = target("kind = 'docker'\nimage = 'talixio-navigateur'\nuser = '65532:65532'\nnetwork = 'talixio-net'\nagent_in_image = true\nmounts = [{ volume = 'talixio-data', subpath = 'work/releves', container = '/releves', write = true }, { volume = 'autre', container = '/lu' }]").unwrap();
        let args = docker_args("abc", &t, std::path::Path::new("/talixio-agent"));
        for wanted in ["--network=talixio-net", "type=volume,src=talixio-data,dst=/releves,volume-nocopy,volume-subpath=work/releves", "type=volume,src=autre,dst=/lu,volume-nocopy,readonly", "--cap-drop=ALL", "--read-only", "--entrypoint=/talixio-agent"] {
            assert!(args.iter().any(|a| a == wanted), "{wanted} manque dans {args:?}");
        }
        assert!(!args.iter().any(|a| a.contains("host-gateway") || a.contains(":/talixio-agent:ro") || a.contains("docker.sock") || a.contains("privileged")), "{args:?}");
    }

    #[test]
    fn rien_dans_une_cible_ne_passe_pour_une_option_ou_une_commande() {
        for bad in [
            "kind = 'ssh'\nhost = '-oProxyCommand=x'\nuser = 'a'\nremote_dir = '/opt/t'",
            "kind = 'ssh'\nhost = 'vm'\nuser = 'a;reboot'\nremote_dir = '/opt/t'",
            "kind = 'ssh'\nhost = 'vm'\nuser = 'a'\nremote_dir = '/opt/t; rm -rf /'",
            "kind = 'ssh'\nhost = 'vm'\nuser = 'a'\nremote_dir = '/opt/$(id)'",
            "kind = 'ssh'\nhost = 'vm'\nuser = 'a'\nremote_dir = '/opt/../etc'",
            "kind = 'docker'\nimage = '--privileged'",
            "kind = 'docker'\nimage = 'debian'\nuser = '0'",
            "kind = 'docker'\nimage = 'debian'\nuser = 'root'",
            "kind = 'docker'\nimage = 'debian'",
            "kind = 'ssh'\nhost = 'vm'\nuser = 'a'\nremote_dir = '//'",
            "kind = 'ssh'\nhost = 'vm'\nuser = 'a'\nremote_dir = '/tmp'",
            "kind = 'docker'\nimage = 'debian'\nmounts = [{ host = '/srv:/etc', container = '/data' }]",
            "kind = 'docker'\nimage = 'debian'\n[capabilities]\nmodels = ['main']",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nseccomp = 'unconfined'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nseccomp = '/opt/profil.json --privileged'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nseccomp = '/opt/../etc/profil.json'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nseccomp = '/opt/profil'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nmemory = '2g --privileged'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nmemory = '0g'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nmemory = '2'",
            "kind = 'ssh'\nhost = 'vm'\nuser = 'a'\nremote_dir = '/opt/t'\nmemory = '2g'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nnetwork = 'host'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nnetwork = 'container:talixio'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nnetwork = 'net --privileged'",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nmounts = [{ volume = '/var/run/docker.sock', container = '/s' }]",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nmounts = [{ volume = 'data', subpath = '../autre', container = '/s' }]",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nmounts = [{ volume = 'data,readonly=false', container = '/s' }]",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nmounts = [{ volume = 'data', subpath = 'a,dst=/etc', container = '/s' }]",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nmounts = [{ volume = 'data', host = '/srv', container = '/s' }]",
            "kind = 'docker'\nimage = 'debian'\nuser = '1000'\nmounts = [{ container = '/s' }]",
            "kind = 'ssh'\nhost = 'vm'\nuser = 'a'\nremote_dir = '/opt/t'\nnetwork = 'net'",
            "kind = 'telnet'",
        ] {
            assert!(target(bad).is_err(), "aurait dû être refusé : {bad}");
        }
    }
}
