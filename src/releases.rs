//! Les notes de version, embarquées dans l'exécutable. Source : `releases.toml`.

use serde::{Deserialize, Serialize};

const SOURCE: &str = include_str!("../releases.toml");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub version: String,
    pub date: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub kind: Kind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    #[serde(rename = "nouveau")]
    New,
    #[serde(rename = "sécurité")]
    Security,
    #[serde(rename = "amélioration")]
    Improved,
    #[serde(rename = "correction")]
    Fixed,
    #[serde(rename = "attention")]
    Notice,
}

impl Kind {
    /// Étiquette, et sa couleur dans un terminal.
    fn label(self) -> (&'static str, &'static str) {
        match self {
            Kind::New => ("nouveau", "32"),
            Kind::Security => ("sécurité", "35"),
            Kind::Improved => ("amélioration", "36"),
            Kind::Fixed => ("correction", "34"),
            Kind::Notice => ("attention", "33"),
        }
    }
}

#[derive(Deserialize)]
struct File {
    release: Vec<Release>,
}

/// Toutes les versions, la plus récente d'abord.
pub fn all() -> Vec<Release> {
    toml::from_str::<File>(SOURCE).map(|f| f.release).unwrap_or_default()
}

/// Les notes mises en page pour un terminal. `color` : avec couleurs.
pub fn render(releases: &[Release], color: bool) -> String {
    let paint = |code: &str, text: &str| if color { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_string() };
    let mut out = String::new();
    for (index, release) in releases.iter().enumerate() {
        let current = if release.version == VERSION { "  ← cette version" } else { "" };
        let head = format!("v{} — {}", release.version, release.title);
        out.push_str(&format!("{}{}\n", paint("1", &head), paint("2", &format!("  {}{current}", release.date))));
        out.push_str(&format!("{}\n\n", wrap(&release.summary, "  ", "  ")));
        for change in &release.changes {
            let (label, code) = change.kind.label();
            let tag = format!("{label:>12}");
            out.push_str(&format!("{}  {}\n", paint(code, &tag), wrap(&change.text, "", "              ")));
        }
        if index + 1 < releases.len() {
            out.push_str(&format!("\n{}\n\n", paint("2", &"─".repeat(72))));
        }
    }
    out
}

/// Replie un texte à 72 colonnes environ.
fn wrap(text: &str, first: &str, next: &str) -> String {
    let mut out = String::from(first);
    let mut width = 0;
    for word in text.split_whitespace() {
        let len = word.chars().count();
        // jamais de ligne qui commence par une ponctuation
        if width > 0 && width + len + 1 > 58 && !matches!(word, ":" | ";" | "!" | "?" | "»") {
            out.push('\n');
            out.push_str(next);
            width = 0;
        } else if width > 0 {
            out.push(' ');
            width += 1;
        }
        out.push_str(word);
        width += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_notes_sont_lisibles_et_a_jour() {
        let file: File = toml::from_str(SOURCE).expect("releases.toml invalide");
        let releases = file.release;
        assert_eq!(releases[0].version, VERSION, "la première note doit être celle de la version de Cargo.toml");
        let number = |v: &str| v.split('.').map(|n| n.parse::<u32>().expect("numéro de version")).collect::<Vec<_>>();
        for pair in releases.windows(2) {
            assert!(number(&pair[0].version) > number(&pair[1].version), "versions dans le désordre");
        }
        for release in &releases {
            assert!(!release.changes.is_empty() && !release.title.is_empty() && release.date.len() == 10);
        }
        assert!(render(&releases, false).contains("← cette version"));
    }
}
