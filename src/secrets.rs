//! Secrets for commands: a token or a password that a command of an installed pack needs, kept
//! by the master and handed to that command alone, for one run.
//!
//! A secret never goes to a model: not in instructions, not in a tool, not in an output. The
//! operator adds it on the master (interface or API), ties it to one command of a pack and to
//! the roles that may use it. When an agent runs that command, its process asks for the secret
//! over the link, with a throwaway key: the master checks, seals the answer for that key, and
//! the agents in between relay what they cannot read. The value lives in the environment of the
//! one command, and is removed from its output before the model reads it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305};
use ring::agreement::{agree_ephemeral, EphemeralPrivateKey, UnparsedPublicKey, X25519};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::pack::SecretCommand;

/// A value shorter than this would be found by chance in an output, and guessed.
pub const MIN_VALUE: usize = 8;
pub const MAX_VALUE: usize = 4096;
const MAX_SECRETS: usize = 100;

/// One secret, as the master keeps it: the value, and where the operator lets it go.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Secret {
    pub name: String,
    value: String,
    pub enabled: bool,
    /// The command it is for (`pack/program`) and the variable that command reads it in.
    pub command: String,
    pub variable: String,
    /// The installed roles whose agents may use it.
    #[serde(default)]
    pub roles: Vec<String>,
    /// The master itself may use it.
    #[serde(default)]
    pub master: bool,
    /// It may go to an agent on another machine (an ssh target): there it exists in the memory
    /// of an account the master does not hold. Refused unless the operator says so.
    #[serde(default)]
    pub remote: bool,
    /// Plain settings that travel with it (the address it is valid for, a user name).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// SHA-256 of the command's program when the operator last saved this secret: a replaced
    /// pack does not inherit it.
    #[serde(default)]
    pinned: String,
    pub updated_ms: u64,
    /// Last time it was handed to a command, and to which agent.
    #[serde(default)]
    pub used_ms: u64,
    #[serde(default)]
    pub used_by: String,
}

// the value never shows, not even in a debug print
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({}, {} → {})", self.name, self.command, self.variable)
    }
}

impl Secret {
    /// What the interface may know: everything but the value.
    fn view(&self) -> Value {
        json!({
            "name": self.name, "enabled": self.enabled, "command": self.command, "variable": self.variable,
            "roles": self.roles, "master": self.master, "remote": self.remote, "env": self.env,
            "updated_ms": self.updated_ms, "used_ms": self.used_ms, "used_by": self.used_by,
        })
    }
}

/// One variable handed to a command.
#[derive(Clone, Serialize, Deserialize)]
pub struct Given {
    pub name: String,
    pub value: String,
    /// A secret (removed from the output), or a plain setting that goes with one.
    pub secret: bool,
}

/// A request for the secrets of one command, as it goes up the link.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecretAsk {
    /// The agent whose command it is.
    pub origin: String,
    /// Its role, written by its parent (never by itself).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// It runs on another machine, or below an agent that does: written by the parents.
    #[serde(default)]
    pub remote: bool,
    pub program: PathBuf,
    pub variables: Vec<String>,
    /// The throwaway public key the answer is sealed for.
    pub key: String,
}

impl SecretAsk {
    /// What the sealed answer is tied to.
    pub fn binding(&self) -> String {
        format!("{}\n{}", self.origin, self.program.display())
    }
}

/// Who asks: stamped by the parents on the way up, never taken from the asking agent alone.
pub struct Asker<'a> {
    pub agent: &'a str,
    /// Its role; `None`: the master itself.
    pub role: Option<&'a str>,
    /// It runs on another machine, or below an agent that does.
    pub remote: bool,
}

/// The master's vault: one closed file per secret under `<state_dir>/secrets/`.
pub struct Vault {
    dir: PathBuf,
    secrets: Mutex<BTreeMap<String, Secret>>,
    commands: Vec<SecretCommand>,
}

fn name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 40 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !name.starts_with('-')
}

/// A plain setting: one short printable line, nothing that hides.
fn setting_ok(value: &str) -> bool {
    !value.is_empty() && value.len() <= 300 && crate::clean(value, 300, false) == value
}

impl Vault {
    /// Reads the vault. A file that cannot be read stops the master: a secret silently dropped
    /// would look like a command that stopped working.
    pub fn open(state_dir: &Path, commands: Vec<SecretCommand>) -> Result<Self, String> {
        let dir = state_dir.join("secrets");
        let mut secrets = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "json") {
                    continue;
                }
                crate::config::ensure_private(&path).map_err(|e| format!("{e:#}"))?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{} : {e}", path.display()))?;
                let secret: Secret = serde_json::from_str(&text).map_err(|_| format!("{} is not a secret of this master (remove it, or restore it)", path.display()))?;
                if !name_ok(&secret.name) || path.file_stem().is_none_or(|stem| stem != secret.name.as_str()) {
                    return Err(format!("{} : name and file do not match", path.display()));
                }
                secrets.insert(secret.name.clone(), secret);
            }
        }
        Ok(Self { dir, secrets: Mutex::new(secrets), commands })
    }

    fn save(&self, secret: &Secret) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).and_then(|_| crate::os::make_private(&self.dir).map_err(std::io::Error::other)).map_err(|e| hidden(format!("{} : {e}", self.dir.display())))?;
        let path = self.dir.join(format!("{}.json", secret.name));
        let content = serde_json::to_vec(secret).map_err(|e| e.to_string())?;
        crate::connect::write_private(&path, &content).map_err(hidden)
    }

    /// What the interface shows: the secrets without their values, and the commands that accept one.
    pub fn list(&self) -> Value {
        let secrets = self.secrets.lock().unwrap();
        json!({
            "secrets": secrets.values().map(|s| { let mut view = s.view(); view["stale"] = json!(self.stale(s)); view }).collect::<Vec<_>>(),
            "commands": self.commands.iter().map(|c| json!({ "name": c.name, "secrets": c.secrets, "env": c.env, "roles": c.roles })).collect::<Vec<_>>(),
        })
    }

    fn pin(&self, secret: &mut Secret) {
        if let Some(command) = self.commands.iter().find(|c| c.name == secret.command) {
            secret.pinned = command.sha256.clone();
        }
    }

    /// The command of this secret is no longer the program it was saved for.
    fn stale(&self, secret: &Secret) -> bool {
        self.commands.iter().find(|c| c.name == secret.command).is_none_or(|c| c.sha256 != secret.pinned)
    }

    /// Checks where a secret is allowed to go, against the installed packs.
    fn check_link(&self, secret: &Secret) -> Result<(), String> {
        let command = self.commands.iter().find(|c| c.name == secret.command).ok_or("this command is not one of an installed pack that accepts secrets")?;
        if !command.secrets.contains(&secret.variable) {
            return Err(format!("the command {} does not accept a secret in {}", command.name, crate::clean(&secret.variable, 64, false)));
        }
        if let Some(role) = secret.roles.iter().find(|role| !command.roles.contains(role)) {
            return Err(format!("the role {} does not hold the command {}", crate::clean(role, 80, false), command.name));
        }
        if secret.roles.len() > 32 || secret.roles.iter().any(|r| secret.roles.iter().filter(|o| *o == r).count() > 1) {
            return Err("roles: each one once".into());
        }
        if !secret.master && secret.roles.is_empty() {
            return Err("choose who may use it: at least one role, or the master".into());
        }
        for (name, value) in &secret.env {
            if !command.env.contains(name) {
                return Err(format!("the command {} has no setting named {}", command.name, crate::clean(name, 64, false)));
            }
            if !setting_ok(value) {
                return Err(format!("{name}: one printable line of 300 characters at most"));
            }
        }
        Ok(())
    }

    fn check_value(value: &str) -> Result<(), String> {
        if value.len() < MIN_VALUE || value.len() > MAX_VALUE {
            return Err(format!("the value must have {MIN_VALUE} to {MAX_VALUE} characters"));
        }
        if value.chars().any(char::is_control) || value.trim() != value {
            return Err("the value must be one line, without a space at either end".into());
        }
        Ok(())
    }

    /// Adds a secret. The value is written once: nothing gives it back.
    pub fn create(&self, body: &Value) -> Result<Value, String> {
        let text = |field: &str| body[field].as_str().unwrap_or("").to_string();
        let name = text("name");
        if !name_ok(&name) {
            return Err("name: 1 to 40 lowercase letters, digits or dashes".into());
        }
        let value = text("value");
        Self::check_value(&value)?;
        let mut secrets = self.secrets.lock().unwrap();
        if secrets.contains_key(&name) {
            return Err(format!("a secret is already named {name}"));
        }
        if secrets.len() >= MAX_SECRETS {
            return Err(format!("{MAX_SECRETS} secrets at most"));
        }
        let mut secret = Secret {
            name: name.clone(),
            value,
            enabled: true,
            command: text("command"),
            variable: text("variable"),
            roles: Vec::new(),
            master: false,
            remote: false,
            env: BTreeMap::new(),
            pinned: String::new(),
            updated_ms: crate::now_ms(),
            used_ms: 0,
            used_by: String::new(),
        };
        apply(&mut secret, body)?;
        self.check_link(&secret)?;
        self.pin(&mut secret);
        self.save(&secret)?;
        let view = secret.view();
        secrets.insert(name, secret);
        Ok(view)
    }

    /// Changes a secret: its value, who may use it, its settings, enabled or not. The command
    /// and the variable do not change: another use is another secret.
    pub fn update(&self, name: &str, body: &Value) -> Result<Value, String> {
        let mut secrets = self.secrets.lock().unwrap();
        let mut secret = secrets.get(name).cloned().ok_or("unknown secret")?;
        if let Some(value) = body["value"].as_str().filter(|v| !v.is_empty()) {
            Self::check_value(value)?;
            secret.value = value.to_string();
        }
        apply(&mut secret, body)?;
        // a secret whose pack is gone can still be disabled, never widened
        if secret.enabled || self.commands.iter().any(|c| c.name == secret.command) {
            self.check_link(&secret)?;
        }
        // saved by the operator: it is for the command as it is installed now
        self.pin(&mut secret);
        secret.updated_ms = crate::now_ms();
        self.save(&secret)?;
        let view = secret.view();
        secrets.insert(name.to_string(), secret);
        Ok(view)
    }

    pub fn delete(&self, name: &str) -> Result<Value, String> {
        let mut secrets = self.secrets.lock().unwrap();
        if !secrets.contains_key(name) {
            return Err("unknown secret".into());
        }
        let path = self.dir.join(format!("{name}.json"));
        std::fs::remove_file(&path).map_err(|e| hidden(format!("{} : {e}", path.display())))?;
        secrets.remove(name);
        Ok(json!({ "deleted": name }))
    }

    /// What a command receives: for each variable it asks for, the one enabled secret the
    /// operator tied to this command for this asker, and the settings that go with it.
    /// `Ok` with nothing: no secret is meant for this asker, the command runs without.
    /// Second value: what happened, for the master's journal (names only).
    pub fn resolve(&self, asker: &Asker, program: &Path, variables: &[String]) -> (Vec<Given>, Vec<String>) {
        let mut given = Vec::new();
        let mut notes = Vec::new();
        let Some(command) = self.commands.iter().find(|c| c.program == program) else {
            return (given, vec![format!("secret refused to {}: {} is not a command that accepts one", asker.agent, program.display())]);
        };
        let mut secrets = self.secrets.lock().unwrap();
        for variable in variables.iter().filter(|v| command.secrets.contains(v)) {
            let meant = |s: &&mut Secret| {
                s.command == command.name && s.variable == *variable && match asker.role {
                    None => s.master,
                    Some(role) => s.roles.iter().any(|r| r == role),
                }
            };
            let mut found: Vec<&mut Secret> = secrets.values_mut().filter(meant).collect();
            let enabled = found.iter().filter(|s| s.enabled).count();
            if enabled > 1 {
                notes.push(format!("secret refused to {}: several enabled secrets fill {} of {} for it, keep one", asker.agent, variable, command.name));
                continue;
            }
            let Some(secret) = found.iter_mut().find(|s| s.enabled) else {
                if let Some(off) = found.first() {
                    notes.push(format!("secret {} not given to {} of {}: it is disabled", off.name, command.name, asker.agent));
                }
                continue;
            };
            if secret.pinned != command.sha256 {
                notes.push(format!("secret {} not given to {} of {}: the command changed since the secret was saved (open it in the interface and save it to confirm)", secret.name, command.name, asker.agent));
                continue;
            }
            if asker.remote && !secret.remote {
                notes.push(format!("secret {} refused to {} of {}: the agent runs on another machine, and this secret is not allowed there", secret.name, command.name, asker.agent));
                continue;
            }
            secret.used_ms = crate::now_ms();
            secret.used_by = asker.agent.to_string();
            notes.push(format!("secret {} given to {} of {}{}", secret.name, command.name, asker.agent, asker.role.map(|r| format!(" ({r})")).unwrap_or_default()));
            given.push(Given { name: variable.clone(), value: secret.value.clone(), secret: true });
            given.extend(secret.env.iter().map(|(name, value)| Given { name: name.clone(), value: value.clone(), secret: false }));
        }
        (given, notes)
    }
}

/// Applies the fields of a request that say where a secret goes.
fn apply(secret: &mut Secret, body: &Value) -> Result<(), String> {
    if let Some(enabled) = body["enabled"].as_bool() {
        secret.enabled = enabled;
    }
    if let Some(master) = body["master"].as_bool() {
        secret.master = master;
    }
    if let Some(remote) = body["remote"].as_bool() {
        secret.remote = remote;
    }
    if let Some(roles) = body["roles"].as_array() {
        secret.roles = roles.iter().map(|r| r.as_str().map(str::to_string).ok_or("roles: a list of names")).collect::<Result<_, _>>()?;
    }
    if let Some(env) = body["env"].as_object() {
        secret.env = env
            .iter()
            // an empty field is a setting left out
            .filter(|(_, value)| value.as_str() != Some(""))
            .map(|(name, value)| value.as_str().map(|v| (name.clone(), v.to_string())).ok_or("env: names and texts"))
            .collect::<Result<_, _>>()?;
    }
    Ok(())
}

/// What the API says of a failure on disk: no path, no detail (they go to the master's error output).
fn hidden(detail: String) -> String {
    eprintln!("secrets: {detail}");
    "the master could not write its state directory (detail in its error output)".to_string()
}

// --- the way: sealed for the asking agent, unreadable by those who relay

/// The asking side: a throwaway key pair, made for one request.
pub struct Ask {
    private: EphemeralPrivateKey,
    pub public: String,
}

fn bound(request: &str, asker_public: &[u8], master_public: &[u8]) -> Vec<u8> {
    [b"talixio-secret-v1\n", request.as_bytes(), b"\n", asker_public, master_public].concat()
}

fn key(shared: &[u8], bound: &[u8]) -> Result<LessSafeKey, String> {
    let okm = ring::hkdf::Salt::new(ring::hkdf::HKDF_SHA256, b"talixio-secret-v1").extract(shared);
    let info = [bound];
    let okm = okm.expand(&info, &CHACHA20_POLY1305).map_err(|_| "key derivation failed")?;
    Ok(LessSafeKey::new(UnboundKey::from(okm)))
}

impl Ask {
    pub fn new() -> Result<Self, String> {
        let private = EphemeralPrivateKey::generate(&X25519, &SystemRandom::new()).map_err(|_| "no source of randomness")?;
        let public = crate::hex(private.compute_public_key().map_err(|_| "key refused")?.as_ref());
        Ok(Self { private, public })
    }

    /// Opens the master's answer. `request` ties the answer to this one request.
    pub fn open(self, request: &str, sealed: &str) -> Result<Vec<Given>, String> {
        let bytes = crate::unhex(sealed).filter(|b| b.len() > 32 + 12 + 16).ok_or("unreadable answer")?;
        let (master_public, rest) = bytes.split_at(32);
        let (nonce, data) = rest.split_at(12);
        let asker_public = crate::unhex(&self.public).ok_or("key refused")?;
        let bound = bound(request, &asker_public, master_public);
        let mut data = data.to_vec();
        let plain = agree_ephemeral(self.private, &UnparsedPublicKey::new(&X25519, master_public), |shared| {
            let key = key(shared, &bound)?;
            let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| "unreadable answer")?;
            key.open_in_place(nonce, Aad::from(&bound), &mut data).map(|plain| plain.to_vec()).map_err(|_| "the answer was not made for this request".to_string())
        })
        .map_err(|_| "key agreement failed")??;
        serde_json::from_slice(&plain).map_err(|_| "unreadable answer".into())
    }
}

/// The master's side: seals what a command receives for the key of the agent that asked.
pub fn seal(request: &str, asker_public: &str, given: &[Given]) -> Result<String, String> {
    let rng = SystemRandom::new();
    let asker_public = crate::unhex(asker_public).filter(|k| k.len() == 32).ok_or("key refused")?;
    let private = EphemeralPrivateKey::generate(&X25519, &rng).map_err(|_| "no source of randomness")?;
    let public = private.compute_public_key().map_err(|_| "key refused")?;
    let bound = bound(request, &asker_public, public.as_ref());
    let mut nonce = [0u8; 12];
    rng.fill(&mut nonce).map_err(|_| "no source of randomness")?;
    let mut data = serde_json::to_vec(given).map_err(|e| e.to_string())?;
    agree_ephemeral(private, &UnparsedPublicKey::new(&X25519, &asker_public), |shared| {
        key(shared, &bound)?.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(&bound), &mut data).map_err(|_| "sealing failed".to_string())
    })
    .map_err(|_| "key agreement failed")??;
    Ok(format!("{}{}{}", crate::hex(public.as_ref()), crate::hex(&nonce), crate::hex(&data)))
}

/// Removes every secret value from what a command wrote, before a model reads it: the value
/// itself, and the start of it that a cut output would end with.
pub fn redact(output: &mut Vec<u8>, truncated: bool, given: &[Given]) {
    for secret in given.iter().filter(|g| g.secret && g.value.len() >= MIN_VALUE) {
        let value = secret.value.as_bytes();
        let mark = format!("[secret {}]", secret.name).into_bytes();
        let mut clean = Vec::with_capacity(output.len());
        let mut at = 0;
        while at < output.len() {
            if output[at..].starts_with(value) {
                clean.extend_from_slice(&mark);
                at += value.len();
            } else {
                clean.push(output[at]);
                at += 1;
            }
        }
        // an output cut at its limit may end in the middle of the value: the end goes, whatever
        // it holds (looking for the value there would tell a reader how the value starts)
        if truncated {
            clean.truncate(clean.len().saturating_sub(value.len() - 1));
        }
        *output = clean;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(dir: &Path) -> Vault {
        let commands = vec![SecretCommand {
            name: "audit/fetch-repo".into(),
            program: "/packs/audit/fetch-repo".into(),
            secrets: vec!["AUDIT_READ_TOKEN".into()],
            env: vec!["AUDIT_READ_URL".into()],
            roles: vec!["audit/lead".into()],
            sha256: "aa".into(),
        }];
        Vault::open(dir, commands).unwrap()
    }

    fn ask(vault: &Vault, role: Option<&str>, remote: bool) -> (Vec<Given>, Vec<String>) {
        vault.resolve(&Asker { agent: "a-1", role, remote }, Path::new("/packs/audit/fetch-repo"), &["AUDIT_READ_TOKEN".to_string()])
    }

    #[test]
    fn a_secret_goes_only_where_the_operator_said() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = vault(tmp.path());
        let body = json!({ "name": "gitlab", "value": "glpat-0123456789", "command": "audit/fetch-repo", "variable": "AUDIT_READ_TOKEN",
            "roles": ["audit/lead"], "env": { "AUDIT_READ_URL": "https://gitlab.com/acme/" } });
        let view = vault.create(&body).unwrap();
        // nothing gives the value back
        assert!(!view.to_string().contains("glpat") && !vault.list().to_string().contains("glpat"));
        assert!(vault.create(&body).is_err(), "same name twice");

        let (given, notes) = ask(&vault, Some("audit/lead"), false);
        assert_eq!(given.len(), 2);
        assert!(given[0].secret && given[0].value == "glpat-0123456789" && !given[1].secret && given[1].name == "AUDIT_READ_URL");
        assert!(notes[0].contains("gitlab given") && !notes[0].contains("glpat"));
        // another role, the master, another machine, another command, another variable: nothing
        assert!(ask(&vault, Some("audit/goal"), false).0.is_empty());
        assert!(ask(&vault, None, false).0.is_empty());
        let (given, notes) = ask(&vault, Some("audit/lead"), true);
        assert!(given.is_empty() && notes[0].contains("another machine"));
        let asker = Asker { agent: "a-1", role: Some("audit/lead"), remote: false };
        assert!(vault.resolve(&asker, Path::new("/usr/bin/env"), &["AUDIT_READ_TOKEN".to_string()]).0.is_empty());
        assert!(vault.resolve(&asker, Path::new("/packs/audit/fetch-repo"), &["PATH".to_string()]).0.is_empty());

        // allowed elsewhere and to the master, then disabled: at once
        vault.update("gitlab", &json!({ "remote": true, "master": true })).unwrap();
        assert_eq!(ask(&vault, Some("audit/lead"), true).0.len(), 2);
        assert_eq!(ask(&vault, None, false).0.len(), 2);
        vault.update("gitlab", &json!({ "enabled": false })).unwrap();
        let (given, notes) = ask(&vault, Some("audit/lead"), false);
        assert!(given.is_empty() && notes[0].contains("disabled"));

        // kept on disk, closed, and read again; the value changes without being shown
        vault.update("gitlab", &json!({ "enabled": true, "value": "glpat-abcdefghij" })).unwrap();
        let again = super::tests::vault(tmp.path());
        assert_eq!(ask(&again, Some("audit/lead"), false).0[0].value, "glpat-abcdefghij");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tmp.path().join("secrets/gitlab.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
        // the pack was replaced: the same command name with another program does not inherit it
        let mut replaced = super::tests::vault(tmp.path());
        replaced.commands[0].sha256 = "bb".into();
        let (given, notes) = ask(&replaced, Some("audit/lead"), false);
        assert!(given.is_empty() && notes[0].contains("command changed") && replaced.list()["secrets"][0]["stale"] == true);
        replaced.update("gitlab", &json!({})).unwrap();
        assert_eq!(ask(&replaced, Some("audit/lead"), false).0.len(), 2);
        again.delete("gitlab").unwrap();
        assert!(ask(&again, Some("audit/lead"), false).0.is_empty() && !tmp.path().join("secrets/gitlab.json").exists());
    }

    #[test]
    fn a_secret_is_refused_when_it_is_tied_to_nothing_known() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = vault(tmp.path());
        let base = json!({ "name": "gitlab", "value": "glpat-0123456789", "command": "audit/fetch-repo", "variable": "AUDIT_READ_TOKEN", "roles": ["audit/lead"] });
        let with = |field: &str, value: Value| {
            let mut body = base.clone();
            body[field] = value;
            vault.create(&body)
        };
        assert!(with("name", json!("Git Lab")).is_err());
        assert!(with("value", json!("short")).is_err());
        assert!(with("value", json!("two\nlines-of-text")).is_err());
        assert!(with("command", json!("audit/repo-git")).is_err());
        assert!(with("variable", json!("PATH")).is_err());
        assert!(with("roles", json!(["audit/goal"])).is_err(), "a role that does not hold the command");
        assert!(with("roles", json!([])).is_err(), "nobody to use it");
        assert!(with("env", json!({ "LD_PRELOAD": "/tmp/x.so" })).is_err());
        assert!(with("env", json!({ "AUDIT_READ_URL": "two\nlines" })).is_err());
        // two enabled secrets for the same place: neither goes
        vault.create(&base).unwrap();
        with("name", json!("gitlab-2")).unwrap();
        let (given, notes) = ask(&vault, Some("audit/lead"), false);
        assert!(given.is_empty() && notes[0].contains("several"));
    }

    #[test]
    fn an_answer_opens_only_for_the_key_and_the_request_it_was_sealed_for() {
        let given = vec![Given { name: "AUDIT_READ_TOKEN".into(), value: "glpat-0123456789".into(), secret: true }];
        let ask = Ask::new().unwrap();
        let sealed = seal("r1|a-1|/packs/audit/fetch-repo", &ask.public, &given).unwrap();
        assert!(!sealed.contains(&crate::hex(b"glpat-0123456789")));
        let opened = ask.open("r1|a-1|/packs/audit/fetch-repo", &sealed).unwrap();
        assert_eq!(opened[0].value, "glpat-0123456789");
        // another key (an agent in between), another request, a changed answer
        let ask = Ask::new().unwrap();
        let other = Ask::new().unwrap();
        let sealed = seal("r1", &ask.public, &given).unwrap();
        assert!(other.open("r1", &sealed).is_err());
        let ask2 = Ask::new().unwrap();
        let sealed2 = seal("r1", &ask2.public, &given).unwrap();
        assert!(ask2.open("r2", &sealed2).is_err());
        let mut changed = sealed.clone();
        changed.replace_range(100..102, if &sealed[100..102] == "00" { "01" } else { "00" });
        assert!(ask.open("r1", &changed).is_err());
    }

    #[test]
    fn a_secret_is_removed_from_what_the_model_reads() {
        let given = vec![
            Given { name: "AUDIT_READ_TOKEN".into(), value: "glpat-0123456789".into(), secret: true },
            Given { name: "AUDIT_READ_URL".into(), value: "https://gitlab.com/".into(), secret: false },
        ];
        let full = b"fatal: https://oauth2:glpat-0123456789@gitlab.com/ twice glpat-0123456789, end: glpat-012".to_vec();
        let mut output = full.clone();
        redact(&mut output, false, &given);
        // a whole output is not searched for the start of the value: that would tell how it starts
        assert_eq!(String::from_utf8(output).unwrap(), "fatal: https://oauth2:[secret AUDIT_READ_TOKEN]@gitlab.com/ twice [secret AUDIT_READ_TOKEN], end: glpat-012");
        // a cut output loses its end, whatever it holds
        let mut output = full;
        redact(&mut output, true, &given);
        let text = String::from_utf8(output).unwrap();
        assert!(text.ends_with("twice [secret AUDIT_READ_TOKEN],") && !text.contains("glpat"), "{text}");
    }
}
