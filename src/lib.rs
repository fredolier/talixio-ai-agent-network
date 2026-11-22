//! Agents IA Talixio : une grappe de petits process autonomes.
//!
//! Chaque agent a un modèle, des instructions et des droits limités. Il peut créer des
//! enfants, qui ne parlent qu'à lui ; les droits d'un enfant ne dépassent jamais ceux du parent.

pub mod agent;
pub mod api;
pub mod catalog;
pub mod config;
pub mod service;
pub mod connect;
pub mod link;
pub mod model;
pub mod node;
pub mod os;
pub mod depot;
pub mod pack;
pub mod policy;
pub mod protocol;
pub mod releases;
pub mod schedule;
pub mod spawn;
pub mod store;
pub mod tls;
pub mod tools;
pub mod tui;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Identifiant ou jeton aléatoire (122 bits d'aléa par UUID v4).
pub fn random_hex(uuids: usize) -> String {
    (0..uuids).map(|_| uuid::Uuid::new_v4().simple().to_string()).collect()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
}

/// Preuve qu'on détient un secret, sans le montrer : HMAC-SHA256 du rôle (`parent` ou
/// `child`) et des éléments de l'échange, chacun précédé de sa longueur. Ces éléments sont
/// l'aléa de l'enfant, celui du parent, l'identifiant de l'enfant et l'empreinte du certificat
/// du parent : la preuve ne vaut que sur la connexion chiffrée où elle a été faite.
pub fn proof(secret: &str, side: &str, parts: [&str; 4]) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepte toute taille de clé");
    for part in std::iter::once(side).chain(parts) {
        mac.update(&(part.len() as u64).to_be_bytes());
        mac.update(part.as_bytes());
    }
    mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

/// Texte venu d'ailleurs, rendu inoffensif à l'affichage : sans caractères de contrôle
/// (sauts de ligne gardés si `multiline`), sans caractères invisibles ni marques qui retournent
/// le sens de lecture (un nom ne doit pas pouvoir en imiter un autre), et coupé à `max` caractères.
pub fn clean(text: &str, max: usize, multiline: bool) -> String {
    // ce qui ne se voit pas, ou change le sens de lecture : largeur nulle, bidirectionnel,
    // « étiquettes », sélecteurs de variante, usage privé
    let hidden = |c: char| {
        matches!(c,
            '\u{AD}' | '\u{34F}' | '\u{61C}' | '\u{115F}' | '\u{1160}' | '\u{180E}' | '\u{3164}' | '\u{FFA0}'
            | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
            | '\u{FE00}'..='\u{FE0F}' | '\u{FFF9}'..='\u{FFFB}' | '\u{E000}'..='\u{F8FF}' | '\u{E0000}'..='\u{E0FFF}' | '\u{F0000}'..='\u{10FFFF}')
    };
    let keep = |c: &char| !hidden(*c) && (!c.is_control() || (multiline && (*c == '\n' || *c == '\t')));
    let mut out: String = text.chars().filter(keep).take(max).collect();
    if text.chars().filter(keep).nth(max).is_some() {
        out.push_str("… [tronqué]");
    }
    out
}

/// Comparaison en temps constant, pour les jetons.
pub fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
