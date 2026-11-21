//! Le catalogue des fournisseurs et des modèles, embarqué dans l'exécutable. Source : `catalog.toml`.
//!
//! Il sert à montrer : quels fournisseurs existent (pays, Europe ou non, niveau de confiance),
//! lesquels sont connectés chez ce maître, quels modèles ils offrent et lesquels sont activés.
//! Il ne donne aucun droit : seuls les modèles de la configuration du maître sont appelables.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::model::ModelInfo;

const SOURCE: &str = include_str!("../catalog.toml");

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
    CATALOG.set(parsed).map_err(|_| "catalogue déjà chargé".to_string())
}

pub fn provider(id: &str) -> Option<&'static Provider> {
    catalog().provider.iter().find(|p| p.id == id)
}

/// Comment déclarer ce modèle de ce fournisseur : `(famille d'API, nature, lit les images)`.
/// Un modèle absent du catalogue est tenu pour un modèle de texte, si le fournisseur en sert.
pub fn declaration(provider: &Provider, model_id: &str) -> Option<(&'static str, &'static str, bool)> {
    match catalog().model.iter().find(|m| m.provider == provider.id && m.id == model_id) {
        Some(model) => declared(model, &provider.apis).map(|(api, nature)| (api, nature, model.nature == "Vision")),
        None => ["anthropic", "openai"].into_iter().find(|api| provider.apis.iter().any(|a| a == api)).map(|api| (api, "llm", false)),
    }
}

/// Comment déclarer ce modèle dans la configuration du maître : `(provider, nature)`, si le
/// maître a un connecteur pour lui chez ce fournisseur. (La famille d'API est connue ; l'essai
/// réel chez chaque fournisseur reste à faire.)
fn declared(model: &Model, apis: &[String]) -> Option<(&'static str, &'static str)> {
    let (nature, wanted): (&str, &[&'static str]) = match model.nature.as_str() {
        "LLM" | "Vision" => ("llm", &["anthropic", "openai"]),
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
            value["connected"] = json!(linked.contains(&p.id) || mine.iter().any(|m| m.has_key || p.trust == "L"));
            // connecté depuis l'interface : on peut alors activer ses modèles et le déconnecter d'ici
            value["linked"] = json!(linked.contains(&p.id));
            // ce que la page propose : une clé à coller, une autorisation dans le navigateur, ou rien
            value["connect"] = json!(match () {
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
        for p in &catalog.provider {
            assert!(matches!(p.trust.as_str(), "1" | "2" | "3" | "4" | "5" | "L"), "{}", p.id);
            assert!(p.apis.iter().all(|a| matches!(a.as_str(), "anthropic" | "openai" | "elevenlabs" | "timesfm" | "systemone")), "{}", p.id);
            // sans machine connue, rien ne peut s'y rattacher : permis seulement pour ce qu'on ne sait pas appeler
            assert!(!p.hosts.is_empty() || p.apis.is_empty(), "{}", p.id);
            // « européen » veut dire souverain : jamais accordé à un niveau de confiance moindre
            assert_eq!(p.europe, p.trust == "5", "{}", p.id);
        }
        for m in &catalog.model {
            assert!(catalog.provider.iter().any(|p| p.id == m.provider), "{} : fournisseur inconnu", m.name);
            assert!((1..=10).contains(&m.rank), "{}", m.name);
            assert!(matches!(m.nature.as_str(), "LLM" | "Vision" | "Voice" | "Image" | "Embedding" | "Forecast" | "Decision"), "{}", m.name);
        }
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
}
