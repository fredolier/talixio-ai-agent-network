//! Le catalogue des fournisseurs et des modèles, embarqué dans l'exécutable. Source : `catalog.toml`.
//!
//! Il sert à montrer : quels fournisseurs existent (pays, Europe ou non, niveau de confiance),
//! lesquels sont connectés chez ce maître, quels modèles ils offrent et lesquels sont activés.
//! Il ne donne aucun droit : seuls les modèles de la configuration du maître sont appelables.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::model::ModelInfo;

/// Le catalogue embarqué, tel qu'il est dans le dépôt.
pub const SOURCE: &str = include_str!("../catalog.toml");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub country: String,
    /// Hébergé et régi en Europe.
    pub europe: bool,
    /// `1` à `5`, ou `L` pour une machine locale.
    pub trust: String,
    pub kind: String,
    pub billing: String,
    /// Les familles d'API par lesquelles le maître sait l'appeler : `anthropic`, `openai` (toute
    /// API compatible), `elevenlabs`, `timesfm`, `systemone`. Vide : pas encore appelable.
    pub apis: Vec<String>,
    /// Adresse publique à donner dans la configuration ; vide si elle dépend du compte.
    pub endpoint: String,
    pub hosts: Vec<String>,
    /// La page où créer une clé chez ce fournisseur.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keys_url: String,
    /// Le fournisseur répond aussi sans clé, avec des limites basses : on peut s'y connecter sans rien coller.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keyless: bool,
    /// OAuth avec PKCE : la page d'autorisation, et l'adresse où échanger le code contre une clé.
    #[serde(default, skip_serializing)]
    pub oauth_authorize: String,
    #[serde(default, skip_serializing)]
    pub oauth_exchange: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub name: String,
    /// Le nom exact attendu par l'API du fournisseur.
    pub id: String,
    pub provider: String,
    /// `LLM`, `Vision`, `Voice`, `Image`, `Embedding`, `Forecast` (prévision) ou `Decision`.
    pub nature: String,
    /// Qualité relative, de 1 à 10.
    pub rank: u8,
    /// Prix par million de tokens, en dollars.
    pub price_in: Option<f64>,
    pub price_out: Option<f64>,
    /// Prix à l'unité (`min`, `img`, `Mtok`) pour la voix, l'image et les plongements.
    pub unit_price: Option<f64>,
    pub unit: Option<String>,
    pub currency: Option<String>,
    #[serde(default)]
    pub estimated: bool,
    #[serde(default)]
    pub deprecated: bool,
    /// Annoncé, pas encore proposé par le fournisseur ou par la pile qui le sert.
    #[serde(default)]
    pub soon: bool,
    /// `Voice` : le modèle parle (synthèse) au lieu d'écouter (transcription).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub speech: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    provider: Vec<Provider>,
    model: Vec<Model>,
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| toml::from_str(SOURCE).expect("catalog.toml illisible (vérifié par un test)"))
}

/// Remplace le catalogue embarqué par celui d'un fichier (`catalog_file` dans la configuration du
/// maître), avant tout usage. Qui écrit ce fichier choisit les adresses où iront les clés : il
/// se protège comme la configuration elle-même.
pub fn install(text: &str) -> Result<(), String> {
    let parsed: Catalog = toml::from_str(text).map_err(|e| format!("catalogue illisible : {e}"))?;
    check(&parsed)?;
    CATALOG.set(parsed).map_err(|_| "catalogue déjà chargé".to_string())
}

/// Un catalogue fixe où vont les clés et où la page envoie l'utilisateur : identifiants sans
/// surprise (ils nomment des fichiers), adresses en HTTPS, ou sur cette machine.
fn check(catalog: &Catalog) -> Result<(), String> {
    let address = |text: &str, local_ok: bool| {
        text.is_empty()
            || reqwest::Url::parse(text).is_ok_and(|url| {
                let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
                url.username().is_empty() && url.password().is_none() && (url.scheme() == "https" || (local_ok && local && url.scheme() == "http"))
            })
    };
    for p in &catalog.provider {
        let id_ok = !p.id.is_empty() && p.id.len() <= 40 && p.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !p.id.starts_with('-');
        if !id_ok {
            return Err(format!("catalogue : identifiant de fournisseur invalide « {} » (minuscules, chiffres, tirets)", crate::clean(&p.id, 60, false)));
        }
        if catalog.provider.iter().filter(|other| other.id == p.id).count() > 1 {
            return Err(format!("catalogue : fournisseur « {} » déclaré deux fois", p.id));
        }
        if !address(&p.endpoint, true) || !address(&p.oauth_exchange, true) {
            return Err(format!("catalogue : fournisseur « {} » : une adresse où part une clé doit être en https (ou sur cette machine)", p.id));
        }
        if !address(&p.keys_url, false) || !address(&p.oauth_authorize, false) {
            return Err(format!("catalogue : fournisseur « {} » : `keys_url` et `oauth_authorize` doivent être en https", p.id));
        }
        if p.oauth_authorize.is_empty() != p.oauth_exchange.is_empty() {
            return Err(format!("catalogue : fournisseur « {} » : `oauth_authorize` et `oauth_exchange` vont ensemble", p.id));
        }
    }
    Ok(())
}

/// Ce que tout catalogue doit respecter pour être servi tel quel : valeurs connues, modèles
/// rattachés à un fournisseur, rien en double.
fn coherent(catalog: &Catalog) -> Result<(), String> {
    for p in &catalog.provider {
        if !matches!(p.trust.as_str(), "1" | "2" | "3" | "4" | "5" | "L") {
            return Err(format!("fournisseur « {} » : confiance « {} » inconnue (1 à 5, ou L)", p.id, crate::clean(&p.trust, 20, false)));
        }
        if let Some(api) = p.apis.iter().find(|a| !matches!(a.as_str(), "anthropic" | "openai" | "elevenlabs" | "timesfm" | "systemone" | "claude-code")) {
            return Err(format!("fournisseur « {} » : famille d'API « {} » inconnue", p.id, crate::clean(api, 30, false)));
        }
        // sans machine connue, rien ne peut s'y rattacher : permis seulement pour ce qu'on ne sait pas appeler
        if p.hosts.is_empty() && !p.apis.is_empty() {
            return Err(format!("fournisseur « {} » : `hosts` manque", p.id));
        }
        // « européen » veut dire souverain : jamais accordé à un niveau de confiance moindre
        if p.europe != (p.trust == "5") {
            return Err(format!("fournisseur « {} » : `europe` va avec la confiance 5, et seulement elle", p.id));
        }
        if p.name.trim().is_empty() || p.country.trim().is_empty() {
            return Err(format!("fournisseur « {} » : nom ou pays vide", p.id));
        }
    }
    for m in &catalog.model {
        let label = crate::clean(&m.name, 60, false);
        if m.name.trim().is_empty() || m.id.trim().is_empty() {
            return Err(format!("modèle « {label} » : nom ou identifiant vide"));
        }
        if !catalog.provider.iter().any(|p| p.id == m.provider) {
            return Err(format!("modèle « {label} » : fournisseur « {} » inconnu", crate::clean(&m.provider, 40, false)));
        }
        if !(1..=10).contains(&m.rank) {
            return Err(format!("modèle « {label} » : `rank` va de 1 à 10"));
        }
        if !matches!(m.nature.as_str(), "LLM" | "Vision" | "Voice" | "Image" | "Embedding" | "Forecast" | "Decision") {
            return Err(format!("modèle « {label} » : nature « {} » inconnue", crate::clean(&m.nature, 30, false)));
        }
        if [m.price_in, m.price_out, m.unit_price].into_iter().flatten().any(|price| !(0.0..=100_000.0).contains(&price)) {
            return Err(format!("modèle « {label} » : prix hors de toute vraisemblance"));
        }
        if catalog.model.iter().filter(|other| other.provider == m.provider && other.id == m.id && other.nature == m.nature && other.speech == m.speech).count() > 1 {
            return Err(format!("modèle « {} » déclaré deux fois chez « {} »", crate::clean(&m.id, 80, false), m.provider));
        }
    }
    Ok(())
}

/// Vérifie un catalogue candidat sans le charger, et dit ce qu'il change par rapport à celui de
/// cet exécutable. C'est le contrôle à passer avant de le proposer au dépôt.
pub fn audit(text: &str) -> Result<String, String> {
    use std::collections::HashMap;
    // un catalogue tient en quelques dizaines de milliers d'octets : au-delà, ce n'en est pas un
    if text.len() > 1_000_000 {
        return Err("catalogue trop gros (1 Mo au plus)".into());
    }
    let candidate: Catalog = toml::from_str(text).map_err(|e| format!("catalogue illisible : {}", crate::clean(&e.to_string(), 600, true)))?;
    check(&candidate).map_err(|e| crate::clean(&e, 600, true))?;
    coherent(&candidate).map_err(|e| crate::clean(&e, 600, true))?;
    let current: Catalog = toml::from_str(SOURCE).map_err(|e| e.to_string())?;
    // ce qui sort d'ici est lu par un agent, puis par une personne : rien de ce que le fichier
    // candidat porte n'y passe sans être nettoyé et borné
    let said = |text: &str| crate::clean(text, 200, false);
    let key = |m: &Model| format!("{} / {}", m.provider, m.id);
    let old_providers: HashMap<&str, &Provider> = current.provider.iter().map(|p| (p.id.as_str(), p)).collect();
    let new_providers: HashMap<&str, &Provider> = candidate.provider.iter().map(|p| (p.id.as_str(), p)).collect();
    let old_models: HashMap<String, &Model> = current.model.iter().map(|m| (key(m), m)).collect();
    let new_models: HashMap<String, &Model> = candidate.model.iter().map(|m| (key(m), m)).collect();

    // là où part une clé, et par quelle porte : un changement ici se dit en tête
    let mut routes = Vec::new();
    let mut touched = Vec::new();
    for p in &candidate.provider {
        let Some(old) = old_providers.get(p.id.as_str()) else { continue };
        let mut fields = Vec::new();
        if old.endpoint != p.endpoint {
            fields.push(format!("endpoint « {} » → « {} »", said(&old.endpoint), said(&p.endpoint)));
        }
        if old.hosts != p.hosts {
            fields.push(format!("hosts {} → {}", said(&old.hosts.join(", ")), said(&p.hosts.join(", "))));
        }
        if old.apis != p.apis {
            fields.push(format!("apis {} → {}", said(&old.apis.join(", ")), said(&p.apis.join(", "))));
        }
        if old.keys_url != p.keys_url {
            fields.push(format!("keys_url « {} » → « {} »", said(&old.keys_url), said(&p.keys_url)));
        }
        if (&old.oauth_authorize, &old.oauth_exchange) != (&p.oauth_authorize, &p.oauth_exchange) {
            fields.push("adresses OAuth".to_string());
        }
        if old.keyless != p.keyless {
            fields.push(format!("keyless {} → {}", old.keyless, p.keyless));
        }
        if !fields.is_empty() {
            routes.push(format!("{} : {}", said(&p.id), fields.join(" ; ")));
        } else if format!("{old:?}") != format!("{p:?}") {
            touched.push(said(&p.id));
        }
    }
    let mut prices = Vec::new();
    let mut changed = Vec::new();
    for m in &candidate.model {
        let Some(old) = old_models.get(&key(m)) else { continue };
        if format!("{old:?}") == format!("{m:?}") {
            continue;
        }
        let mut what = Vec::new();
        if (old.price_in, old.price_out, old.unit_price) != (m.price_in, m.price_out, m.unit_price) {
            prices.push(said(&key(m)));
            what.push("prix");
        }
        if old.rank != m.rank {
            what.push("rank");
        }
        if (&old.nature, old.speech) != (&m.nature, m.speech) {
            what.push("nature");
        }
        if old.name != m.name {
            what.push("nom");
        }
        // le reste (soon, estimated, deprecated, contexte…) : dit sans détail
        let other = { let mut a = (*old).clone(); a.price_in = m.price_in; a.price_out = m.price_out; a.unit_price = m.unit_price; a.rank = m.rank; a.nature = m.nature.clone(); a.speech = m.speech; a.name = m.name.clone(); format!("{a:?}") != format!("{m:?}") };
        if other {
            what.push("autres champs");
        }
        if what != ["prix"] {
            changed.push(format!("{} ({})", said(&key(m)), what.join(", ")));
        }
    }

    let mut lines = Vec::new();
    if !routes.is_empty() {
        lines.push("ATTENTION : l'adresse ou la porte d'un fournisseur change (c'est là que part sa clé) — à relire par une personne :".to_string());
        lines.extend(routes.iter().map(|item| format!("  {item}")));
    }
    lines.push(format!("catalogue valide : {} fournisseurs, {} modèles", candidate.provider.len(), candidate.model.len()));
    let mut list = |title: &str, items: Vec<String>| {
        if !items.is_empty() {
            lines.push(format!("{title} ({}) :", items.len()));
            lines.extend(items.into_iter().map(|item| format!("  {item}")));
        }
    };
    list("fournisseurs ajoutés (adresse et hôtes à relire)", candidate.provider.iter().filter(|p| !old_providers.contains_key(p.id.as_str())).map(|p| format!("{} : endpoint « {} », hosts {}", said(&p.id), said(&p.endpoint), said(&p.hosts.join(", ")))).collect());
    list("fournisseurs retirés", current.provider.iter().filter(|p| !new_providers.contains_key(p.id.as_str())).map(|p| said(&p.id)).collect());
    list("fournisseurs modifiés (fiche)", touched);
    list("modèles ajoutés", candidate.model.iter().filter(|m| !old_models.contains_key(&key(m))).map(|m| said(&key(m))).collect());
    list("modèles retirés", current.model.iter().filter(|m| !new_models.contains_key(&key(m))).map(|m| said(&key(m))).collect());
    list("prix modifiés", prices);
    list("modèles modifiés", changed);
    Ok(lines.join("\n"))
}

impl Provider {
    /// Servi par la commande `claude` de la machine du maître : ni adresse, ni clé.
    pub fn cli(&self) -> bool {
        self.apis.iter().any(|api| api == "claude-code")
    }
}

pub fn provider(id: &str) -> Option<&'static Provider> {
    catalog().provider.iter().find(|p| p.id == id)
}

/// Comment déclarer ce modèle de ce fournisseur : `(famille d'API, nature, lit les images)`.
/// Un modèle absent du catalogue est tenu pour un modèle de texte, si le fournisseur en sert.
pub fn declaration(provider: &Provider, model_id: &str) -> Option<(&'static str, &'static str, bool)> {
    match catalog().model.iter().find(|m| m.provider == provider.id && m.id == model_id) {
        Some(model) => declared(model, &provider.apis).map(|(api, nature)| (api, nature, model.nature == "Vision")),
        None => ["anthropic", "openai", "claude-code"].into_iter().find(|api| provider.apis.iter().any(|a| a == api)).map(|api| (api, "llm", false)),
    }
}

/// Comment déclarer ce modèle dans la configuration du maître : `(provider, nature)`, si le
/// maître a un connecteur pour lui chez ce fournisseur. (La famille d'API est connue ; l'essai
/// réel chez chaque fournisseur reste à faire.)
fn declared(model: &Model, apis: &[String]) -> Option<(&'static str, &'static str)> {
    let (nature, wanted): (&str, &[&'static str]) = match model.nature.as_str() {
        "LLM" | "Vision" => ("llm", &["anthropic", "openai", "claude-code"]),
        "Voice" if model.speech => ("speech", &["openai", "elevenlabs"]),
        "Voice" => ("transcription", &["openai"]),
        "Image" => ("image", &["openai"]),
        "Embedding" => ("embedding", &["openai"]),
        "Forecast" => ("forecast", &["timesfm"]),
        "Decision" => ("decision", &["systemone"]),
        _ => return None,
    };
    if model.soon {
        return None;
    }
    wanted.iter().find(|api| apis.iter().any(|a| a == **api)).map(|api| (*api, nature))
}

/// La nature d'un modèle configuré, dans le vocabulaire du catalogue.
fn listed_nature(model: &ModelInfo) -> &'static str {
    match model.nature.as_str() {
        "transcription" | "speech" => "Voice",
        "image" => "Image",
        "embedding" => "Embedding",
        "forecast" => "Forecast",
        "decision" => "Decision",
        _ if model.vision => "Vision",
        _ => "LLM",
    }
}

impl Provider {
    /// Cette machine (telle que la configuration la nomme, port compris) est-elle à ce fournisseur ?
    fn serves(&self, host: &str) -> bool {
        let name = match host.rsplit_once(':') {
            Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
            _ => host,
        };
        let name = name.trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();
        self.hosts.iter().any(|h| match h.strip_prefix('.') {
            Some(suffix) => name.strip_suffix(suffix).is_some_and(|rest| rest.ends_with('.')),
            None => name == *h,
        })
    }
}

/// Ce que l'interface montre : les fournisseurs (connectés ou non) et les modèles (activés ou
/// non), à partir du catalogue et de ce que la configuration du maître déclare. Jamais de clé.
pub fn view(configured: &[ModelInfo], linked: &[String]) -> Value {
    let catalog = catalog();
    // le fournisseur du catalogue auquel chaque modèle configuré se rattache, par sa machine
    let owner: Vec<Option<&Provider>> = configured.iter().map(|m| catalog.provider.iter().find(|p| p.serves(&m.host))).collect();
    let mut providers: Vec<Value> = catalog
        .provider
        .iter()
        .map(|p| {
            let mine: Vec<&ModelInfo> = configured.iter().zip(&owner).filter(|(_, o)| o.is_some_and(|o| o.id == p.id)).map(|(m, _)| m).collect();
            let mut value = serde_json::to_value(p).unwrap_or_default();
            // connecté : au moins un modèle configuré chez lui, avec sa clé (une machine locale n'en demande pas)
            value["connected"] = json!(linked.contains(&p.id) || mine.iter().any(|m| m.has_key || p.trust == "L" || p.cli()));
            // connecté depuis l'interface : on peut alors activer ses modèles et le déconnecter d'ici
            value["linked"] = json!(linked.contains(&p.id));
            // ce que la page propose : une clé à coller, une autorisation dans le navigateur, ou rien
            value["connect"] = json!(match () {
                _ if p.cli() => "cli",
                _ if p.endpoint.is_empty() || p.apis.is_empty() => "config",
                _ if !p.oauth_authorize.is_empty() => "oauth",
                _ if p.trust == "L" => "local",
                _ => "key",
            });
            value["configured"] = json!(mine.len());
            value["listed"] = json!(true);
            value
        })
        .collect();
    let mut models: Vec<Value> = catalog
        .model
        .iter()
        .map(|m| {
            let aliases: Vec<&str> = configured
                .iter()
                .zip(&owner)
                .filter(|(c, o)| o.is_some_and(|o| o.id == m.provider) && c.model == m.id)
                .map(|(c, _)| c.alias.as_str())
                .collect();
            let apis = catalog.provider.iter().find(|p| p.id == m.provider).map(|p| p.apis.as_slice()).unwrap_or_default();
            let mut value = serde_json::to_value(m).unwrap_or_default();
            value["aliases"] = json!(aliases);
            let declared = declared(m, apis);
            value["callable"] = json!(declared.is_some());
            // les lignes à écrire dans la configuration du maître pour l'activer
            if let Some((provider, nature)) = declared {
                value["declare"] = json!({ "provider": provider, "nature": nature, "vision": m.nature == "Vision" });
            }
            value["listed"] = json!(true);
            value
        })
        .collect();
    // ce que la configuration déclare et que le catalogue ne connaît pas : montré quand même
    for (model, owner) in configured.iter().zip(&owner) {
        let provider = match owner {
            Some(p) => p.id.clone(),
            None => {
                let id = format!("autre:{}", model.host);
                match providers.iter_mut().find(|p| p["id"] == id.as_str()) {
                    Some(known) => {
                        known["configured"] = json!(known["configured"].as_u64().unwrap_or(0) + 1);
                        known["connected"] = json!(known["connected"] == true || model.has_key);
                    }
                    None => providers.push(json!({
                        "id": id, "name": model.host, "country": "", "europe": null, "trust": "", "kind": "Hors catalogue",
                        "billing": "", "apis": [model.provider], "endpoint": "", "hosts": [model.host],
                        "connected": model.has_key, "configured": 1, "listed": false,
                    })),
                }
                id
            }
        };
        let known = catalog.model.iter().any(|m| m.provider == provider && m.id == model.model);
        if !known {
            match models.iter_mut().find(|m| m["listed"] == false && m["provider"] == provider.as_str() && m["id"] == model.model.as_str()) {
                Some(same) => {
                    if let Some(aliases) = same["aliases"].as_array_mut() {
                        aliases.push(json!(model.alias));
                    }
                }
                None => models.push(json!({
                    "name": model.model, "id": model.model, "provider": provider, "nature": listed_nature(model), "rank": null,
                    "price_in": null, "price_out": null, "unit_price": null, "unit": null, "currency": null,
                    "estimated": false, "deprecated": false, "soon": false, "aliases": [model.alias], "callable": true, "listed": false,
                })),
            }
        }
    }
    json!({ "providers": providers, "catalog": models })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured(alias: &str, model: &str, host: &str, has_key: bool) -> ModelInfo {
        ModelInfo { alias: alias.into(), provider: "openai".into(), model: model.into(), host: host.into(), effort: None, max_tokens: None, has_key, nature: "llm".into(), vision: false }
    }

    #[test]
    fn le_catalogue_est_lisible_et_coherent() {
        let catalog = catalog();
        assert!(catalog.provider.len() >= 14 && catalog.model.len() >= 60);
        for nature in ["Forecast", "Decision"] {
            assert!(catalog.model.iter().any(|m| m.nature == nature), "{nature}");
        }
        coherent(catalog).unwrap();
        assert!(audit(SOURCE).unwrap().starts_with("catalogue valide"));
        // un candidat est comparé à celui de l'exécutable ; une incohérence est dite, pas chargée
        let fewer = SOURCE.replacen("rank = ", "rank = 1", 0);
        assert_eq!(audit(&fewer).unwrap().lines().count(), 1, "identique : rien à signaler");
        let added = format!("{SOURCE}\n[[model]]\nname = \"Neuf\"\nid = \"neuf-1\"\nprovider = \"mistral\"\nnature = \"LLM\"\nrank = 5\n");
        assert!(audit(&added).unwrap().contains("modèles ajoutés (1) :\n  mistral / neuf-1"));
        assert!(audit(&added.replace("provider = \"mistral\"\nnature = \"LLM\"\nrank = 5", "provider = \"inconnu\"\nnature = \"LLM\"\nrank = 5")).unwrap_err().contains("fournisseur « inconnu » inconnu"));
        assert!(audit(&added.replace("rank = 5\n", "rank = 11\n")).is_err());
        // un fournisseur connu dont l'adresse change : dit en tête, c'est là que part sa clé
        let mistral = provider("mistral").unwrap();
        let moved = SOURCE.replace(&format!("endpoint = \"{}\"", mistral.endpoint), "endpoint = \"https://ailleurs.example/v1\"").replace(&format!("\"{}\"", mistral.hosts[0]), "\"ailleurs.example\"");
        let report = audit(&moved).unwrap();
        assert!(report.starts_with("ATTENTION") && report.contains("mistral : endpoint") && report.contains("ailleurs.example"), "{report}");
        // un modèle dont autre chose que le prix change est dit aussi
        let first = &catalog.model[0];
        let ranked = audit(&SOURCE.replacen(&format!("rank = {}\n", first.rank), &format!("rank = {}\n", if first.rank > 1 { first.rank - 1 } else { 2 }), 1)).unwrap();
        assert!(ranked.contains("modèles modifiés (1) :") && ranked.contains("(rank)"), "{ranked}");
        // ce que le fichier porte ne sort pas tel quel : ni caractère de contrôle, ni texte sans fin
        let odd = audit(&added.replace("id = \"neuf-1\"", &format!("id = \"neuf\\u001b[31m{}\"", "x".repeat(900))));
        assert!(odd.as_ref().map_or_else(|e| !e.contains('\u{1b}') && e.len() < 2_000, |r| !r.contains('\u{1b}') && r.lines().all(|l| l.chars().count() < 300)), "{odd:?}");
        assert!(audit(&format!("{SOURCE}\n#{}", "x".repeat(1_000_000))).unwrap_err().contains("trop gros"));
        assert!(audit(&format!("{added}{}", added.rsplit_once("\n[[model]]").map(|(_, m)| format!("\n[[model]]{m}")).unwrap())).unwrap_err().contains("déclaré deux fois"));
    }

    #[test]
    fn un_modele_configure_est_rattache_a_son_fournisseur_par_sa_machine() {
        let view = view(&[
            configured("grand", "mistral-large-latest", "api.mistral.ai", true),
            configured("ovh", "gpt-oss-120b", "oai.endpoints.kepler.ai.cloud.ovh.net", false),
            configured("maison", "qwen3:8b", "127.0.0.1:11434", false),
            configured("ailleurs", "x", "modeles.exemple.fr:8443", true),
            // un nom qui finit comme celui d'un fournisseur n'est pas ce fournisseur
            configured("faux", "mistral-large-latest", "api.mistral.ai.exemple.fr", true),
            configured("faux2", "gpt-oss-120b", "evilendpoints.kepler.ai.cloud.ovh.net", true),
        ], &["scaleway".to_string()]);
        let provider = |id: &str| view["providers"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
        assert_eq!((provider("mistral")["connected"].clone(), provider("mistral")["configured"].clone()), (json!(true), json!(1)));
        assert_eq!(provider("ovhcloud")["connected"], false, "sans clé, pas connecté");
        assert_eq!(provider("local")["connected"], true, "une machine locale n'a pas besoin de clé");
        assert_eq!(provider("anthropic")["connected"], false);
        assert_eq!((provider("scaleway")["connected"].clone(), provider("scaleway")["linked"].clone()), (json!(true), json!(true)));
        assert_eq!(provider("openrouter")["connect"], "oauth");
        assert_eq!(provider("mistral")["connect"], "key");
        assert_eq!(provider("infomaniak")["connect"], "config");
        assert!(!view.to_string().contains("oauth_exchange"));
        assert_eq!(provider("autre:modeles.exemple.fr:8443")["listed"], false);
        assert_eq!(provider("autre:api.mistral.ai.exemple.fr")["connected"], true);
        assert_eq!(provider("autre:evilendpoints.kepler.ai.cloud.ovh.net")["configured"], 1);
        let models = view["catalog"].as_array().unwrap();
        let large = models.iter().find(|m| m["id"] == "mistral-large-latest" && m["provider"] == "mistral").unwrap();
        assert_eq!(large["aliases"], json!(["grand"]));
        assert!(models.iter().any(|m| m["id"] == "qwen3:8b" && m["provider"] == "local" && m["listed"] == false));
        let callable = |name: &str| models.iter().find(|m| m["name"] == name).unwrap()["callable"].clone();
        assert_eq!(callable("Voxtral"), true, "transcription par l'API compatible OpenAI");
        assert_eq!(callable("ElevenLabs v3"), true);
        let declare = |name: &str| models.iter().find(|m| m["name"] == name).unwrap()["declare"].clone();
        assert_eq!(declare("Voxtral"), json!({ "provider": "openai", "nature": "transcription", "vision": false }));
        assert_eq!(declare("ElevenLabs v3"), json!({ "provider": "elevenlabs", "nature": "speech", "vision": false }));
        assert_eq!(declare("Jev"), json!({ "provider": "systemone", "nature": "decision", "vision": false }));
        assert_eq!(declare("TimeGPT"), Value::Null);
        assert_eq!(callable("Jev"), true);
        assert_eq!(callable("TimesFM 2.5 (local)"), true);
        assert_eq!(callable("TimeGPT"), false, "annoncé, et aucun connecteur");
        assert_eq!(callable("TimesFM (BigQuery AI.FORECAST)"), false);
        let text = view.to_string();
        assert!(!text.contains("api_key_file") && !text.contains("api_key_env"));
    }

    #[test]
    fn un_catalogue_ne_peut_pas_detourner_une_cle() {
        check(&toml::from_str(SOURCE).unwrap()).unwrap();
        let with = |id: &str, endpoint: &str, more: &str| {
            let text = format!(
                "model = []\n[[provider]]\nid = \"{id}\"\nname = \"Essai\"\ncountry = \"\"\neurope = true\ntrust = \"3\"\nkind = \"\"\nbilling = \"\"\napis = [\"openai\"]\nendpoint = \"{endpoint}\"\nhosts = []\n{more}\n"
            );
            check(&toml::from_str::<Catalog>(&text).unwrap())
        };
        let good = "https://api.exemple.fr/v1";
        with("essai", good, "keys_url = \"https://exemple.fr/cles\"").unwrap();
        with("essai", "http://127.0.0.1:11434/v1", "").unwrap();
        with("essai", good, "oauth_authorize = \"https://exemple.fr/auth\"\noauth_exchange = \"https://exemple.fr/cles\"").unwrap();
        for (id, endpoint, more) in [
            ("../../ailleurs", good, ""),
            ("Majuscule", good, ""),
            ("essai", "http://api.exemple.fr/v1", ""),
            ("essai", "https://u:p@api.exemple.fr/v1", ""),
            ("essai", good, "keys_url = \"javascript:alert(1)\""),
            ("essai", good, "keys_url = \"http://127.0.0.1/cles\""),
            ("essai", good, "oauth_authorize = \"https://exemple.fr/auth\""),
            ("essai", good, "oauth_authorize = \"https://exemple.fr/auth\"\noauth_exchange = \"http://exemple.fr/cles\""),
        ] {
            assert!(with(id, endpoint, more).is_err(), "{id} {endpoint} {more}");
        }
    }
}
