//! Se connecter à un fournisseur et activer un modèle depuis l'interface, pendant que le maître tourne.
//!
//! Tout se passe chez le maître : la clé arrive par l'API (HTTPS, jeton), est essayée chez le
//! fournisseur, puis rangée dans un fichier fermé du dossier d'état (`keys/<fournisseur>.key`)
//! que rien ne rend jamais : ni l'API, ni un outil d'agent (le dossier d'état leur est interdit).
//! Seules les adresses du catalogue sont appelées : la page ne choisit jamais où va une clé.
//! Ce qui est connecté et activé est noté dans `connections.json`, repris au démarrage.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::catalog::{self, Provider};
use crate::config::{ModelConfig, RootConfig};
use crate::node::Node;

/// Appels accordés au plus, par nature, depuis l'interface.
const MAX_CALLS: u64 = 1_000_000;
/// Une autorisation OAuth commencée doit finir dans ce délai.
const OAUTH_WINDOW: Duration = Duration::from_secs(600);

/// Ce qui a été connecté et activé depuis l'interface. Aucun secret ici.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub providers: BTreeSet<String>,
    /// Par alias.
    pub models: BTreeMap<String, SavedModel>,
    pub max_calls: BTreeMap<String, u64>,
    /// L'alias du modèle donné au maître depuis l'interface, quand sa configuration n'en fixe pas.
    pub master: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedModel {
    pub provider: String,
    pub model: String,
}

/// Un changement à la fois.
static CHANGES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// L'autorisation OAuth en cours : fournisseur, secret de vérification, heure de départ.
/// Les alias réellement activés d'ici dans ce maître : eux seuls se retirent d'ici.
static ACTIVE: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
static PENDING: Mutex<Option<(String, String, Instant)>> = Mutex::new(None);

fn file(state_dir: &Path) -> PathBuf {
    state_dir.join("connections.json")
}

fn key_file(state_dir: &Path, provider: &str) -> PathBuf {
    state_dir.join("keys").join(format!("{provider}.key"))
}

pub fn load(state_dir: &Path) -> Saved {
    std::fs::read_to_string(file(state_dir)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// Les fournisseurs connectés depuis l'interface.
pub fn linked(state_dir: &Path) -> Vec<String> {
    load(state_dir).providers.into_iter().collect()
}

/// Écrit un fichier fermé aux autres comptes : créé vide et fermé, puis seulement rempli, puis mis en place.
fn write_private(path: &Path, content: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e| format!("{} : {e}", tmp.display()))?;
    crate::os::make_private(&tmp)?;
    file.write_all(content).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|e| format!("{} : {e}", path.display()))
}

/// Ce que l'API dit d'un échec sur le disque : ni chemin, ni détail (ils vont à la sortie d'erreur du maître).
fn hidden(detail: String) -> String {
    eprintln!("connexion depuis l'interface : {detail}");
    "le maître n'a pas pu écrire ou relire son dossier d'état (détail dans sa sortie d'erreur)".to_string()
}

fn save(state_dir: &Path, saved: &Saved) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(saved).map_err(|e| e.to_string())?;
    write_private(&file(state_dir), &text).map_err(hidden)
}

/// Les alias activés depuis l'interface, tels que ce maître les tient.
pub fn activated() -> Vec<String> {
    ACTIVE.lock().unwrap().iter().cloned().collect()
}

fn id_name(text: &str) -> bool {
    alias_name(text)
}

/// Les natures de service encore servies par un modèle activé d'ici gardent leurs appels ; les autres les perdent.
fn prune_calls(node: &Node, saved: &mut Saved) {
    let natures = node.natures();
    let used: BTreeSet<&String> = saved.models.keys().filter_map(|alias| natures.get(alias)).collect();
    let gone: Vec<String> = saved.max_calls.keys().filter(|nature| !used.contains(nature)).cloned().collect();
    for nature in gone {
        saved.max_calls.remove(&nature);
        node.clear_calls(&nature);
    }
}

fn known(id: &str) -> Result<&'static Provider, String> {
    let provider = catalog::provider(id).ok_or("fournisseur inconnu du catalogue")?;
    if provider.endpoint.is_empty() || provider.apis.is_empty() {
        return Err(format!("{} se déclare dans la configuration du maître : son adresse dépend du compte", provider.name));
    }
    Ok(provider)
}

fn graphic(text: &str, max: usize) -> bool {
    !text.is_empty() && text.len() <= max && text.bytes().all(|b| b.is_ascii_graphic())
}

/// Le nom exact d'un modèle chez son fournisseur : rien qui puisse sortir d'une adresse ou d'un en-tête.
fn model_name(text: &str) -> bool {
    text.len() <= 200
        && text.starts_with(|c: char| c.is_ascii_alphanumeric())
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'/' | b'@' | b'-'))
        && !text.contains("..")
}

fn alias_name(text: &str) -> bool {
    text.len() <= 40
        && text.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Un alias tiré du nom du modèle : minuscules, chiffres, tirets.
fn slug(model: &str) -> String {
    let mut out = String::new();
    for c in model.chars() {
        match c.to_ascii_lowercase() {
            c if c.is_ascii_lowercase() || c.is_ascii_digit() => out.push(c),
            _ if !out.ends_with('-') && !out.is_empty() => out.push('-'),
            _ => {}
        }
    }
    let out: String = out.trim_matches('-').chars().take(36).collect();
    if out.is_empty() { "modele".to_string() } else { out.trim_matches('-').to_string() }
}

/// La déclaration d'un modèle d'un fournisseur connecté, comme on l'écrirait dans la configuration.
/// Rend aussi sa nature telle qu'on la dit aux agents.
fn declare(state_dir: &Path, provider: &Provider, model: &str) -> Result<(ModelConfig, &'static str), String> {
    let (api, nature, vision) = catalog::declaration(provider, model)
        .ok_or_else(|| format!("le maître n'a pas de connecteur pour ce modèle chez {}", provider.name))?;
    let key = key_file(state_dir, &provider.id);
    let cfg = ModelConfig {
        provider: api.to_string(),
        model: model.to_string(),
        base_url: Some(provider.endpoint.clone()),
        api_key_file: key.is_file().then_some(key),
        nature: (nature != "llm").then(|| nature.to_string()),
        vision,
        ..Default::default()
    };
    Ok((cfg, if vision { "vision" } else { nature }))
}

/// Au démarrage, avant de créer le maître : les modèles activés depuis l'interface rejoignent
/// ceux de la configuration. Rend ceux qui ont été repris (alias, nature), à accorder au maître
/// par [`restore`], et ce qu'il faut signaler. La configuration gagne : un alias qu'elle prend
/// est rayé de ce que l'interface a activé. Un modèle dont le fournisseur a quitté le catalogue,
/// ou dont la clé n'est plus lisible, est laissé de côté sans bloquer le démarrage.
pub fn merge(cfg: &mut RootConfig) -> (Vec<(String, &'static str)>, Vec<String>) {
    let mut saved = load(&cfg.state_dir);
    let (mut restored, mut notes) = (Vec::new(), Vec::new());
    let before = saved.models.len();
    let mut struck = Vec::new();
    saved.models.retain(|alias, _| {
        let free = alias_name(alias) && !cfg.models.contains_key(alias);
        if !free {
            struck.push(alias.clone());
            notes.push(format!("modèle « {} » : alias pris par la configuration ou illisible, activation oubliée", crate::clean(alias, 40, false)));
        }
        free
    });
    // un alias repris par la configuration n'est plus celui qu'on avait donné au maître
    let stale = saved.master.as_ref().is_some_and(|alias| struck.contains(alias));
    if stale {
        saved.master = None;
    }
    if saved.models.len() != before || stale {
        let _ = save(&cfg.state_dir, &saved);
    }
    for (alias, model) in &saved.models {
        let declared = known(&model.provider)
            .and_then(|provider| if saved.providers.contains(&model.provider) { declare(&cfg.state_dir, provider, &model.model) } else { Err("fournisseur déconnecté".into()) })
            .and_then(|(declared, nature)| declared.api_key().map(|_| (declared, nature)).map_err(|e| format!("clé illisible ({e:#})")));
        match declared {
            Ok((declared, nature)) => {
                cfg.models.insert(alias.clone(), declared);
                restored.push((alias.clone(), nature));
            }
            Err(e) => notes.push(format!("modèle « {alias} » laissé de côté : {e}")),
        }
    }
    (restored, notes)
}

/// Le maître créé : il retrouve les droits donnés depuis l'interface, que l'interface peut reprendre.
/// Les appels ne reviennent que pour les natures d'un modèle repris ; la configuration, si elle fixe
/// une nature, garde le dernier mot.
pub fn restore(node: &Node, restored: &[(String, &'static str)]) {
    let mut active = ACTIVE.lock().unwrap();
    for (alias, nature) in restored {
        node.grant_model(alias, nature);
        active.insert(alias.clone());
    }
    for (nature, calls) in &load(&node.state_dir).max_calls {
        if crate::policy::counted_nature(nature) && restored.iter().any(|(_, n)| n == nature) {
            node.set_calls(nature, (*calls).min(MAX_CALLS));
        }
    }
    // le modèle donné au maître depuis l'interface, s'il est toujours dans ses droits
    if let Some(alias) = load(&node.state_dir).master.filter(|alias| thinks(node, alias)) {
        seat_master(node, &alias);
    }
}

/// Le maître a eu un modèle depuis son démarrage : plus aucun ne lui est donné d'office.
static SEATED: AtomicBool = AtomicBool::new(false);

fn seat_master(node: &Node, alias: &str) {
    SEATED.store(true, Ordering::Relaxed);
    node.set_model(alias);
}

/// Ce modèle peut-il être celui du maître ? Il est dans ses droits et tient une conversation.
fn thinks(node: &Node, alias: &str) -> bool {
    node.rights().models.iter().any(|m| m == alias) && node.natures().get(alias).is_some_and(|nature| nature == "llm" || nature == "vision")
}

/// Donne au maître le modèle avec lequel il réfléchit, parmi ceux qu'il peut déjà appeler.
/// Refusé si sa configuration en fixe un : elle garde le dernier mot.
pub async fn choose(node: &Node, alias: &str) -> Result<Value, String> {
    let _guard = CHANGES.lock().await;
    if node.model_fixed {
        return Err("le modèle du maître est fixé par sa configuration (`model`) : c'est là qu'il se change".into());
    }
    if !thinks(node, alias) {
        return Err("le maître ne peut prendre qu'un modèle activé qui tient une conversation".into());
    }
    let mut saved = load(&node.state_dir);
    saved.master = Some(alias.to_string());
    save(&node.state_dir, &saved)?;
    seat_master(node, alias);
    node.emit("status", format!("modèle du maître choisi depuis l'interface : « {} » ; la conversation en cours du maître ira désormais à ce modèle", crate::clean(alias, 40, false)));
    Ok(json!({ "default": alias }))
}

/// Le maître perd son modèle si c'est celui-ci qui s'en va.
fn unseat(node: &Node, saved: &mut Saved, alias: &str) {
    if saved.master.as_deref() == Some(alias) {
        saved.master = None;
    }
    if !node.model_fixed && node.model() == alias {
        node.set_model("");
        node.emit("status", "le maître n'a plus de modèle : donnez-lui en un autre dans « Modèles »".to_string());
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        // jamais d'autre adresse que celle du catalogue : la clé ne suit aucune redirection
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}

/// Essaie la clé chez le fournisseur, en listant ses modèles. `Ok(true)` : acceptée ;
/// `Ok(false)` : le fournisseur ne dit ni oui ni non (pas de liste à cette adresse).
async fn verify(provider: &Provider, key: &str) -> Result<bool, String> {
    let base = provider.endpoint.trim_end_matches('/');
    let api = provider.apis.iter().map(String::as_str).find(|a| matches!(*a, "anthropic" | "openai" | "elevenlabs")).unwrap_or("openai");
    let request = match api {
        "anthropic" => client()?.get(format!("{base}/v1/models")).header("x-api-key", key).header("anthropic-version", "2023-06-01"),
        "elevenlabs" => client()?.get(format!("{base}/v1/models")).header("xi-api-key", key),
        _ if key.is_empty() => client()?.get(format!("{base}/models")),
        _ => client()?.get(format!("{base}/models")).bearer_auth(key),
    };
    let response = request.send().await.map_err(|e| format!("{} est injoignable : {}", provider.name, e.without_url()))?;
    match response.status().as_u16() {
        200..=299 => Ok(true),
        401 | 403 => Err(format!("{} refuse cette clé", provider.name)),
        _ => Ok(false),
    }
}

fn store_key(node: &Node, provider: &Provider, key: &str) -> Result<(), String> {
    let dir = node.state_dir.join("keys");
    std::fs::create_dir_all(&dir).map_err(|e| hidden(format!("{} : {e}", dir.display())))?;
    crate::os::make_private(&dir).map_err(hidden)?;
    let path = key_file(&node.state_dir, &provider.id);
    if !key.is_empty() {
        write_private(&path, key.as_bytes()).map_err(hidden)?;
    } else if path.symlink_metadata().is_ok() {
        // connecté sans clé : une clé d'avant ne doit plus partir
        std::fs::remove_file(&path).map_err(|e| hidden(format!("clé non effacée : {e}")))?;
    }
    let mut saved = load(&node.state_dir);
    saved.providers.insert(provider.id.clone());
    save(&node.state_dir, &saved)?;
    // une clé remplacée vaut tout de suite pour les modèles déjà activés : l'ancienne ne sert plus
    let Some(providers) = &node.providers else { return Ok(()) };
    let active = ACTIVE.lock().unwrap().clone();
    for (alias, model) in saved.models.iter().filter(|(alias, m)| m.provider == provider.id && active.contains(*alias)) {
        let (declared, _) = declare(&node.state_dir, provider, &model.model)?;
        let key = declared.api_key().map_err(|e| hidden(format!("{e:#}")))?;
        providers.remove(alias);
        providers.add(alias, &declared, key).map_err(|e| hidden(format!("{e:#}")))?;
    }
    Ok(())
}

/// Connecte un fournisseur avec la clé donnée. Une machine locale se connecte sans clé.
pub async fn connect(node: &Node, id: &str, key: &str) -> Result<Value, String> {
    let provider = known(id)?;
    let key = key.trim();
    let local = provider.trust == "L";
    // sans clé : une machine locale, ou un fournisseur qui répond aussi aux anonymes
    let bare = key.is_empty() && (local || provider.keyless);
    if !bare && !(graphic(key, 512) && key.len() >= 8) {
        return Err("clé invalide : collez la clé telle que le fournisseur la donne, sans espace".into());
    }
    let _guard = CHANGES.lock().await;
    let verified = verify(provider, key).await?;
    if local && !verified {
        return Err(format!("aucun serveur de modèles ne répond à {}", provider.endpoint));
    }
    if bare && !verified {
        return Err(format!("{} ne répond pas sans clé", provider.name));
    }
    store_key(node, provider, key)?;
    node.emit("status", format!("fournisseur connecté depuis l'interface : {}", provider.name));
    Ok(json!({ "connected": provider.id, "verified": verified }))
}

/// Les agents en vie qui détiennent cet alias.
fn holders(node: &Node, alias: &str) -> Vec<String> {
    let children = node.children.lock().unwrap();
    children.values().filter(|c| !c.stopped && c.caps.models.iter().any(|m| m == alias)).map(|c| crate::clean(&c.name, 80, false)).collect()
}

fn check_free(node: &Node, alias: &str) -> Result<(), String> {
    match holders(node, alias) {
        held if held.is_empty() => Ok(()),
        held => Err(format!("le modèle « {alias} » est accordé à {} : arrêtez d'abord ces agents", held.join(", "))),
    }
}

fn drop_model(node: &Node, alias: &str) {
    if let Some(providers) = &node.providers {
        providers.remove(alias);
    }
    node.revoke_model(alias);
}

/// Déconnecte un fournisseur connecté depuis l'interface : ses modèles activés d'ici sont
/// retirés, sa clé est effacée. Ce que la configuration déclare n'est pas touché. Vaut aussi
/// pour un fournisseur qui a quitté le catalogue depuis.
pub async fn disconnect(node: &Node, id: &str) -> Result<Value, String> {
    if !id_name(id) {
        return Err("fournisseur inconnu du catalogue".into());
    }
    let name = catalog::provider(id).map(|p| p.name.clone()).unwrap_or_else(|| id.to_string());
    let _guard = CHANGES.lock().await;
    let mut saved = load(&node.state_dir);
    if !saved.providers.contains(id) {
        return Err(match catalog::provider(id) {
            Some(_) => format!("{name} n'a pas été connecté depuis l'interface : sa connexion est dans la configuration du maître"),
            None => "fournisseur inconnu du catalogue".to_string(),
        });
    }
    let active = ACTIVE.lock().unwrap().clone();
    let aliases: Vec<String> = saved.models.iter().filter(|(_, m)| m.provider == id).map(|(alias, _)| alias.clone()).collect();
    for alias in aliases.iter().filter(|alias| active.contains(*alias)) {
        check_free(node, alias)?;
    }
    for alias in &aliases {
        // seulement ce qui a été activé d'ici : jamais un modèle de la configuration
        if ACTIVE.lock().unwrap().remove(alias) {
            drop_model(node, alias);
            unseat(node, &mut saved, alias);
        }
        if saved.master.as_deref() == Some(alias.as_str()) {
            saved.master = None;
        }
        saved.models.remove(alias);
    }
    saved.providers.remove(id);
    prune_calls(node, &mut saved);
    save(&node.state_dir, &saved)?;
    let key = key_file(&node.state_dir, id);
    if key.symlink_metadata().is_ok() {
        std::fs::remove_file(&key).map_err(|e| hidden(format!("clé non effacée : {e}")))?;
    }
    node.emit("status", format!("fournisseur déconnecté depuis l'interface : {name}"));
    Ok(json!({ "disconnected": id, "models": aliases }))
}

/// Active un modèle d'un fournisseur connecté : il devient appelable par le maître, qui peut
/// l'accorder. `calls` : pour un modèle de service, le nombre d'appels permis à la grappe.
pub async fn activate(node: &Node, id: &str, model: &str, alias: Option<&str>, calls: Option<u64>) -> Result<Value, String> {
    let provider = known(id)?;
    let providers = node.providers.as_ref().ok_or("seul le maître détient des modèles")?;
    if !model_name(model) {
        return Err("nom de modèle invalide : le nom exact attendu par le fournisseur (lettres, chiffres, . _ : / @ -)".into());
    }
    if alias.is_some_and(|a| !alias_name(a)) {
        return Err("alias invalide : minuscules, chiffres et tirets, 40 caractères au plus".into());
    }
    if calls.is_some_and(|c| c > MAX_CALLS) {
        return Err(format!("au plus {MAX_CALLS} appels"));
    }
    let _guard = CHANGES.lock().await;
    let mut saved = load(&node.state_dir);
    if !saved.providers.contains(id) {
        return Err(format!("connectez d'abord {}", provider.name));
    }
    let taken = providers.aliases();
    let alias = match alias {
        Some(alias) if taken.iter().any(|a| a == alias) => return Err(format!("un modèle porte déjà l'alias « {alias} »")),
        Some(alias) => alias.to_string(),
        None => {
            let base = slug(model);
            (1..100).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|a| !taken.contains(a)).ok_or("aucun alias libre : donnez-en un")?
        }
    };
    let (declared, nature) = declare(&node.state_dir, provider, model)?;
    let key = declared.api_key().map_err(|e| hidden(format!("{e:#}")))?;
    saved.models.insert(alias.clone(), SavedModel { provider: id.to_string(), model: model.to_string() });
    // les appels d'une nature que la configuration fixe déjà ne se changent pas d'ici
    let calls = calls.filter(|_| crate::policy::counted_nature(nature) && !node.calls_fixed(nature));
    if let Some(calls) = calls {
        saved.max_calls.insert(nature.to_string(), calls);
    }
    // un maître sans modèle prend le premier qui tient une conversation
    // seulement s'il n'en a jamais eu depuis son démarrage : sinon sa conversation partirait chez un
    // autre fournisseur sans qu'on l'ait demandé. Jamais d'office un serveur local, que tout
    // programme de la machine peut imiter : celui-là se donne par un geste.
    let seat = !node.model_fixed && node.model().is_empty() && !SEATED.load(Ordering::Relaxed) && provider.trust != "L" && matches!(nature, "llm" | "vision");
    if seat {
        saved.master = Some(alias.clone());
    }
    save(&node.state_dir, &saved)?;
    if let Err(e) = providers.add(&alias, &declared, key) {
        saved.models.remove(&alias);
        if seat {
            saved.master = None;
        }
        prune_calls(node, &mut saved);
        let _ = save(&node.state_dir, &saved);
        return Err(format!("{e:#}"));
    }
    node.grant_model(&alias, nature);
    ACTIVE.lock().unwrap().insert(alias.clone());
    if let Some(calls) = calls {
        node.set_calls(nature, calls);
    }
    if seat {
        seat_master(node, &alias);
    }
    node.emit("status", format!("modèle activé depuis l'interface : « {alias} » ({}, {nature}){}", provider.name, if seat { " ; il devient le modèle du maître" } else { "" }));
    Ok(json!({ "alias": alias, "nature": nature, "default": seat, "max_calls": node.rights().max_calls.get(nature) }))
}

/// Désactive un modèle activé depuis l'interface.
pub async fn deactivate(node: &Node, alias: &str) -> Result<Value, String> {
    let _guard = CHANGES.lock().await;
    let mut saved = load(&node.state_dir);
    if !ACTIVE.lock().unwrap().contains(alias) {
        return Err("ce modèle n'a pas été activé depuis l'interface : s'il est dans la configuration du maître, c'est là qu'il se retire".into());
    }
    check_free(node, alias)?;
    ACTIVE.lock().unwrap().remove(alias);
    drop_model(node, alias);
    unseat(node, &mut saved, alias);
    saved.models.remove(alias);
    prune_calls(node, &mut saved);
    save(&node.state_dir, &saved)?;
    node.emit("status", format!("modèle désactivé depuis l'interface : « {} »", crate::clean(alias, 40, false)));
    Ok(json!({ "deactivated": alias }))
}

/// Commence une autorisation dans le navigateur (OAuth avec PKCE). `callback` : la page du
/// maître où le fournisseur renverra ; elle doit être celle d'où vient la demande (`host`).
pub fn oauth_start(id: &str, callback: &str, host: &str) -> Result<Value, String> {
    use base64::Engine;
    use sha2::Digest;
    let provider = known(id)?;
    if provider.oauth_authorize.is_empty() {
        return Err(format!("{} ne propose pas d'autorisation par le navigateur : collez une clé", provider.name));
    }
    let back = reqwest::Url::parse(callback).map_err(|_| "adresse de retour invalide")?;
    let authority = match back.port() {
        Some(port) => format!("{}:{port}", back.host_str().unwrap_or("")),
        None => back.host_str().unwrap_or("").to_string(),
    };
    let plain = back.scheme() == "https" && back.query().is_none() && back.fragment().is_none() && back.username().is_empty() && back.path() == "/";
    if !plain || host.is_empty() || !authority.eq_ignore_ascii_case(host) {
        return Err("adresse de retour refusée : ce doit être la page du maître d'où vient la demande".into());
    }
    let verifier = crate::random_hex(2);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse(&provider.oauth_authorize).map_err(|_| "adresse d'autorisation invalide au catalogue")?;
    url.query_pairs_mut().append_pair("callback_url", back.as_str()).append_pair("code_challenge", &challenge).append_pair("code_challenge_method", "S256");
    *PENDING.lock().unwrap() = Some((id.to_string(), verifier, Instant::now()));
    Ok(json!({ "url": url.as_str() }))
}

/// Termine l'autorisation : échange le code reçu contre une clé, que seul le maître voit.
pub async fn oauth_finish(node: &Node, id: &str, code: &str) -> Result<Value, String> {
    let provider = known(id)?;
    if !graphic(code, 512) {
        return Err("code d'autorisation invalide".into());
    }
    // une autorisation ne sert qu'une fois, pour le fournisseur qui l'a commencée
    let verifier = match PENDING.lock().unwrap().take() {
        Some((pending, verifier, started)) if pending == id && started.elapsed() < OAUTH_WINDOW => verifier,
        _ => return Err("aucune autorisation en cours pour ce fournisseur, ou délai dépassé : recommencez".into()),
    };
    let _guard = CHANGES.lock().await;
    let body = json!({ "code": code, "code_verifier": verifier, "code_challenge_method": "S256" });
    let sent = client()?.post(&provider.oauth_exchange).json(&body).send().await;
    let mut response = sent.map_err(|e| format!("{} est injoignable : {}", provider.name, e.without_url()))?;
    if !response.status().is_success() {
        return Err(format!("{} a refusé l'autorisation (HTTP {})", provider.name, response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("réponse interrompue : {}", e.without_url()))? {
        if bytes.len() + chunk.len() > 64 * 1024 {
            return Err("réponse inattendue du fournisseur".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let reply: Value = serde_json::from_slice(&bytes).map_err(|_| "réponse inattendue du fournisseur")?;
    let key = reply["key"].as_str().map(str::trim).filter(|k| graphic(k, 512) && k.len() >= 8).ok_or("le fournisseur n'a pas rendu de clé")?;
    store_key(node, provider, key)?;
    node.emit("status", format!("fournisseur connecté par autorisation dans le navigateur : {}", provider.name));
    Ok(json!({ "connected": provider.id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_noms_sont_tenus() {
        assert!(model_name("mistral-large-latest") && model_name("anthropic/claude-sonnet-4.5") && model_name("qwen3:8b"));
        for bad in ["", "../v1/admin", "a b", "a?x=1", "a#b", "a\r\nX: y", "/abs", "a..b", "é"] {
            assert!(!model_name(bad), "{bad}");
        }
        assert!(alias_name("mistral-large") && !alias_name("Mistral") && !alias_name("-a") && !alias_name("a_b") && !alias_name(""));
        assert_eq!(slug("anthropic/Claude-Sonnet-4.5"), "anthropic-claude-sonnet-4-5");
        assert_eq!(slug("///"), "modele");
        assert!(alias_name(&slug("Qwen2.5 72B @ très long nom de modèle qui dépasse largement la limite")));
    }

    #[test]
    fn le_retour_d_autorisation_ne_va_que_chez_le_maitre() {
        let start = |callback: &str, host: &str| oauth_start("openrouter", callback, host);
        let url = start("https://maitre.exemple.fr:7800/", "maitre.exemple.fr:7800").unwrap()["url"].as_str().unwrap().to_string();
        assert!(url.starts_with("https://openrouter.ai/auth?callback_url=https%3A%2F%2Fmaitre.exemple.fr%3A7800%2F&code_challenge="), "{url}");
        assert!(url.ends_with("&code_challenge_method=S256"));
        for (callback, host) in [
            ("https://ailleurs.exemple.fr/", "maitre.exemple.fr:7800"),
            ("http://maitre.exemple.fr:7800/", "maitre.exemple.fr:7800"),
            ("https://maitre.exemple.fr:7800/?x=1", "maitre.exemple.fr:7800"),
            ("https://maitre.exemple.fr:7800/chemin", "maitre.exemple.fr:7800"),
            ("https://u@maitre.exemple.fr:7800/", "maitre.exemple.fr:7800"),
            ("https://maitre.exemple.fr:7800/", ""),
            ("pas une adresse", "maitre.exemple.fr:7800"),
        ] {
            assert!(start(callback, host).is_err(), "{callback}");
        }
        assert!(oauth_start("mistral", "https://m/", "m").unwrap_err().contains("collez une clé"));
        assert!(oauth_start("inconnu", "https://m/", "m").is_err());
    }
}
