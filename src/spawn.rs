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

/// Tue un groupe de process entier : la commande et tout ce qu'elle a lancé.
pub fn kill_group(group: i32) {
    #[cfg(unix)]
    if group > 1 {
        // SAFETY: simple appel système ; un groupe disparu renvoie une erreur sans autre effet.
        unsafe {
            libc::killpg(group, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = group;
}

/// Ferme la mémoire et l'environnement du process aux autres process du même compte
/// (`/proc/<pid>/environ`, `ptrace`) : c'est là que vivent les clés chez le maître.
pub fn harden_process() {
    #[cfg(target_os = "linux")]
    // SAFETY: simple appel système, sans pointeur.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
}

/// Lance le même exécutable en mode enfant. Les consignes, jeton compris, passent par
/// l'entrée standard : elles n'apparaissent ni dans la ligne de commande ni dans l'environnement.
pub async fn spawn_local(node: &Node, boot: &Bootstrap) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("exécutable introuvable : {e}"))?;
    let logs = node.state_dir.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("{} : {e}", logs.display()))?;
    let log_path = logs.join(format!("{}-{}.log", boot.name, boot.agent_id));
    let log = std::fs::File::create(&log_path).map_err(|e| format!("{} : {e}", log_path.display()))?;

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
    let payload = serde_json::to_vec(boot).map_err(|e| e.to_string())?;
    stdin.write_all(&payload).await.map_err(|e| format!("transmission des consignes : {e}"))?;
    drop(stdin);
    Ok(child)
}

// ---------------------------------------------------------------------------------------
// Déploiement hors de la machine du parent
// ---------------------------------------------------------------------------------------

use crate::config::Target;

/// Lance un enfant : sur cette machine, ou sur la cible donnée.
pub async fn launch(node: &Node, boot: &Bootstrap, target: Option<&Target>) -> Result<Child, String> {
    match target {
        None => spawn_local(node, boot).await,
        Some(target) if target.kind == "ssh" => spawn_ssh(node, boot, target).await,
        Some(target) => spawn_docker(node, boot, target).await,
    }
}

fn log_file(node: &Node, boot: &Bootstrap) -> Result<std::fs::File, String> {
    let logs = node.state_dir.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("{} : {e}", logs.display()))?;
    let path = logs.join(format!("{}-{}.log", boot.name, boot.agent_id));
    std::fs::File::create(&path).map_err(|e| format!("{} : {e}", path.display()))
}

async fn feed(child: &mut Child, payload: &[u8]) -> Result<(), String> {
    let mut stdin = child.stdin.take().ok_or("entrée standard indisponible")?;
    stdin.write_all(payload).await.map_err(|e| format!("transmission : {e}"))?;
    Ok(())
}

/// `ssh` vers la cible, sans rien d'interactif : la machine doit déjà être connue
/// (`known_hosts`), sinon la connexion est refusée — jamais de confiance à la première vue.
fn ssh(target: &Target, remote_command: &str) -> Command {
    let mut command = Command::new(target.program.clone().unwrap_or_else(|| "/usr/bin/ssh".into()));
    command.args(["-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=10"]);
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
    if let Some(input) = input {
        feed(&mut child, &input).await?;
    }
    let output = tokio::time::timeout(std::time::Duration::from_secs(120), child.wait_with_output())
        .await
        .map_err(|_| "ssh : délai dépassé".to_string())?
        .map_err(|e| format!("ssh : {e}"))?;
    if !output.status.success() {
        let detail = crate::clean(&String::from_utf8_lossy(&output.stderr), 300, false);
        return Err(format!("la machine distante a refusé : {detail}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn remote_paths(target: &Target, id: &str) -> (String, String, String) {
    let dir = target.remote_dir.as_deref().unwrap_or("").trim_end_matches('/').to_string();
    (format!("{dir}/talixio-agent-{id}"), format!("{dir}/state"), dir)
}

/// Dépose l'exécutable sur la machine distante et y lance l'agent. Tout passe par `ssh` :
/// l'exécutable et les consignes voyagent dans son canal chiffré, rien n'est écrit en clair
/// hors du dossier réservé aux agents.
async fn spawn_ssh(node: &Node, boot: &Bootstrap, target: &Target) -> Result<Child, String> {
    let (binary, state, dir) = remote_paths(target, &boot.agent_id);
    let system = ssh_run(target, &format!("umask 077 && mkdir -p {dir} {state} && uname -sm"), None).await?;
    let expected = format!("{} {}", if cfg!(target_os = "macos") { "Darwin" } else { "Linux" }, std::env::consts::ARCH);
    let system = system.trim().replace("arm64", "aarch64");
    if system != expected {
        return Err(format!("la machine distante est « {system} », cet exécutable est fait pour « {expected} »"));
    }
    let exe = std::env::current_exe().map_err(|e| format!("exécutable introuvable : {e}"))?;
    let bytes = tokio::fs::read(&exe).await.map_err(|e| format!("lecture de l'exécutable : {e}"))?;
    ssh_run(target, &format!("umask 077 && cat > {binary} && chmod 700 {binary}"), Some(bytes)).await?;

    // les erreurs de l'agent vont dans un journal distant : sa vie ne dépend pas de la session ssh
    let run = format!("exec {binary} child --state-dir {state} 2>>{state}/{}.log", boot.agent_id);
    let mut command = ssh(target, &run);
    command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(log_file(node, boot)?);
    let mut child = command.spawn().map_err(|e| format!("ssh : {e}"))?;
    feed(&mut child, &serde_json::to_vec(boot).map_err(|e| e.to_string())?).await?;
    Ok(child)
}

fn docker(target: &Target) -> Command {
    let mut command = Command::new(target.program.clone().unwrap_or_else(|| "/usr/bin/docker".into()));
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
    args.extend(["--pids-limit=512", "--memory=1g", "--tmpfs=/tmp"].map(String::from));
    args.push("--add-host=host.docker.internal:host-gateway".into());
    // le dossier d'état vit en mémoire et disparaît avec le conteneur
    match &target.user {
        Some(user) => {
            let uid = user.split(':').next().unwrap_or("");
            args.push(format!("--user={user}"));
            args.push(format!("--tmpfs=/state:mode=700,uid={uid}"));
        }
        None => args.push("--tmpfs=/state:mode=700".into()),
    }
    args.extend(["--volume".into(), format!("{}:/talixio-agent:ro", exe.display())]);
    for mount in &target.mounts {
        let mode = if mount.write { "rw" } else { "ro" };
        args.extend(["--volume".into(), format!("{}:{}:{mode}", mount.host.display(), mount.container.display())]);
    }
    args.push("--entrypoint=/talixio-agent".into());
    args.push(target.image.clone().unwrap_or_default());
    args.extend(["child", "--state-dir", "/state"].map(String::from));
    args
}

async fn spawn_docker(node: &Node, boot: &Bootstrap, target: &Target) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("exécutable introuvable : {e}"))?;
    let mut command = docker(target);
    command.args(docker_args(&boot.agent_id, target, &exe));
    command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(log_file(node, boot)?);
    let mut child = command.spawn().map_err(|e| format!("docker : {e}"))?;
    feed(&mut child, &serde_json::to_vec(boot).map_err(|e| e.to_string())?).await?;
    Ok(child)
}

/// Retire de la cible tout ce que l'agent y a laissé.
pub async fn uninstall(target: &Target, id: &str) -> Result<(), String> {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("identifiant invalide".into());
    }
    if target.kind == "ssh" {
        let (binary, state, _) = remote_paths(target, id);
        let remove = format!("rm -f {binary} {state}/{id}.log {state}/agents/{id}.db {state}/agents/{id}.db-wal {state}/agents/{id}.db-shm");
        ssh_run(target, &remove, None).await.map(|_| ())
    } else {
        let mut command = docker(target);
        command.args(["rm", "--force"]).arg(format!("talixio-{id}")).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        command.status().await.map(|_| ()).map_err(|e| format!("docker : {e}"))
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
    fn un_conteneur_est_ferme_et_ne_montre_que_ses_montages() {
        let t = target("kind = 'docker'\nimage = 'debian:12-slim'\nuser = '1000:1000'\nmounts = [{ host = '/srv/data', container = '/data' }]").unwrap();
        let args = docker_args("abc", &t, std::path::Path::new("/opt/talixio-agent"));
        for wanted in ["--cap-drop=ALL", "--security-opt=no-new-privileges", "--read-only", "--user=1000:1000", "/srv/data:/data:ro", "/opt/talixio-agent:/talixio-agent:ro", "--pull=never"] {
            assert!(args.iter().any(|a| a == wanted), "{wanted} manque dans {args:?}");
        }
        assert!(!args.iter().any(|a| a.contains("privileged") || a.contains("docker.sock")));
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
            "kind = 'docker'\nimage = 'debian'\nmounts = [{ host = '/srv:/etc', container = '/data' }]",
            "kind = 'docker'\nimage = 'debian'\n[capabilities]\nmodels = ['main']",
            "kind = 'telnet'",
        ] {
            assert!(target(bad).is_err(), "aurait dû être refusé : {bad}");
        }
    }
}
