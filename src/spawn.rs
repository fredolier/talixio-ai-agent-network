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
