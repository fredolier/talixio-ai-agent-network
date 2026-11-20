//! Agents IA Talixio : une grappe de petits process autonomes.
//!
//! Chaque agent a un modèle, des instructions et des droits limités. Il peut créer des
//! enfants, qui ne parlent qu'à lui ; les droits d'un enfant ne dépassent jamais ceux du parent.

pub mod agent;
pub mod api;
pub mod config;
pub mod link;
pub mod model;
pub mod node;
pub mod policy;
pub mod protocol;
pub mod spawn;
pub mod tools;

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

/// Comparaison en temps constant, pour les jetons.
pub fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
