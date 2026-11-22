//! Parcourir les paquets à l'écran : la liste à gauche, ce que le paquet choisi donnerait à
//! droite, et l'installation au bout d'une touche. Une façade sur `pack list/show/install/remove` :
//! mêmes contrôles, mêmes dossiers, rien que le maître puisse lancer lui-même.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::pack::{self, Pack};

/// D'où viennent les paquets montrés : l'exécutable, un dossier, ou un dépôt signé.
pub trait Source {
    /// Nom, version, description et identifiant de chaque paquet.
    fn list(&self) -> Result<Vec<(String, String, String, String)>>;
    /// D'où viennent ces paquets, en une ligne : montré en tête d'écran.
    fn label(&self) -> String;
    fn load(&self, name: &str) -> Result<Pack>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    Absent,
    /// Installé, au même contenu.
    Same,
    /// Installé, avec un autre contenu (autre version, ou modifié).
    Other,
    /// Les paquets installés n'ont pas pu être vérifiés : on ne sait pas.
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub version: String,
    pub description: String,
    pub id: String,
    pub mark: Mark,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Up,
    Down,
    PageUp,
    PageDown,
    Char(char),
    Other,
}

/// Ce que l'écran demande de faire après une touche.
#[derive(Debug, PartialEq)]
pub enum Ask {
    Nothing,
    Quit,
    /// Le paquet choisi a changé : son détail est à lire.
    Load,
    Install { name: String, replace: bool },
    Remove { name: String },
    Verify,
}

#[derive(Debug, PartialEq)]
enum Pending {
    Install,
    Remove,
}

pub struct Browser {
    pub items: Vec<Item>,
    pub selected: usize,
    detail: Vec<String>,
    scroll: usize,
    pub status: String,
    pending: Option<Pending>,
    /// Le dossier où l'on installe, s'il est connu.
    pub dir: Option<PathBuf>,
    /// D'où viennent les paquets (signés ou non) : toujours à l'écran.
    pub origin: String,
    size: (usize, usize),
}

/// Ce qui s'affiche tient en une colonne par caractère, et se voit : tout le reste (caractères
/// larges, invisibles, de commande, d'autres écritures) devient « ? ». Un texte ne peut ainsi ni
/// déborder de sa ligne, ni cacher une partie de lui-même.
pub fn shown(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{a0}' | '\u{202f}' | '\t' => ' ',
            c if c == ' ' || c.is_ascii_graphic() => c,
            '\u{ad}' => '?',
            '\u{a1}'..='\u{24f}' => c,
            '…' | '—' | '–' | '’' | '‘' | '“' | '”' | '€' | '✓' | '≠' | '▸' | '│' | '↑' | '↓' => c,
            _ => '?',
        })
        .collect()
}

const KEYS: &str = "↑↓ choisir · PgPréc PgSuiv lire · i installer · r retirer · v vérifier · q quitter";

impl Browser {
    pub fn new(items: Vec<Item>, dir: Option<PathBuf>) -> Self {
        Self { items, selected: 0, detail: Vec::new(), scroll: 0, status: String::new(), pending: None, dir, origin: String::new(), size: (80, 24) }
    }

    pub fn current(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    pub fn set_detail(&mut self, text: &str) {
        // rien qui commande le terminal, ni qui se cache, n'atteint l'écran
        self.detail = text.lines().take(20_000).map(|line| shown(&line.replace('\t', "    "))).collect();
        self.scroll = 0;
    }

    /// La taille du terminal : le défilement en dépend.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.size = (width.max(40), height);
        self.scroll = self.scroll.min(self.last_scroll());
    }

    /// Une question attend-elle sa réponse ?
    pub fn asking(&self) -> bool {
        self.pending.is_some()
    }

    fn body(&self) -> usize {
        self.size.1.saturating_sub(4).max(1)
    }

    fn columns(&self) -> (usize, usize) {
        let left = (self.size.0 / 3).clamp(24, 40);
        (left, self.size.0 - left - 3)
    }

    /// Le défilement le plus bas : la dernière ligne du détail y est à l'écran.
    fn last_scroll(&self) -> usize {
        self.wrapped().len().saturating_sub(self.body())
    }

    pub fn key(&mut self, key: Key) -> Ask {
        // une question est posée : seule « o » y répond oui
        if let Some(pending) = self.pending.take() {
            let Some(item) = self.current().cloned() else { return Ask::Nothing };
            if key != Key::Char('o') && key != Key::Char('O') {
                self.status = "Rien n'a été fait.".into();
                return Ask::Nothing;
            }
            return match pending {
                Pending::Install => Ask::Install { name: item.name, replace: item.mark != Mark::Absent },
                Pending::Remove => Ask::Remove { name: item.name },
            };
        }
        let last = self.items.len().saturating_sub(1);
        match key {
            Key::Up if self.selected > 0 => {
                self.selected -= 1;
                Ask::Load
            }
            Key::Down if self.selected < last => {
                self.selected += 1;
                Ask::Load
            }
            // une page moins une ligne : rien n'est sauté, et la fin s'atteint toujours
            Key::PageUp => {
                self.scroll = self.scroll.saturating_sub(self.body().saturating_sub(1).max(1));
                Ask::Nothing
            }
            Key::PageDown => {
                self.scroll = (self.scroll + self.body().saturating_sub(1).max(1)).min(self.last_scroll());
                Ask::Nothing
            }
            Key::Char('q') | Key::Char('Q') => Ask::Quit,
            Key::Char('v') | Key::Char('V') => Ask::Verify,
            Key::Char('i') | Key::Char('I') => {
                let Some(item) = self.current() else { return Ask::Nothing };
                let Some(dir) = &self.dir else {
                    self.status = "Où installer ? Relancez avec --dir <dossier> (celui que packs_dir désigne).".into();
                    return Ask::Nothing;
                };
                // la question d'abord : un long chemin ne la pousse pas hors de l'écran
                self.status = match item.mark {
                    Mark::Absent => format!("Installer « {} » ? (o/n) — dans {}", item.name, dir.display()),
                    Mark::Same => format!("Réinstaller « {} » ? (o/n) — déjà installé, au même contenu", item.name),
                    Mark::Other => format!("Remplacer « {} » ? (o/n) — installé avec un autre contenu", item.name),
                    Mark::Unknown => format!("Installer « {} » ? (o/n) — l'état des paquets installés est inconnu", item.name),
                };
                self.pending = Some(Pending::Install);
                Ask::Nothing
            }
            Key::Char('r') | Key::Char('R') => {
                let Some(item) = self.current() else { return Ask::Nothing };
                if item.mark == Mark::Absent || self.dir.is_none() {
                    self.status = format!("« {} » n'est pas installé.", item.name);
                    return Ask::Nothing;
                }
                self.status = format!("Retirer « {} » ? (o/n) — le maître le perdra à son prochain démarrage", item.name);
                self.pending = Some(Pending::Remove);
                Ask::Nothing
            }
            _ => Ask::Nothing,
        }
    }

    /// Les paquets installés n'ont pas pu être vérifiés : aucune marque n'est affirmée.
    pub fn unmark(&mut self) {
        for item in &mut self.items {
            item.mark = Mark::Unknown;
        }
    }

    /// Le détail, replié entre les mots à la largeur de sa colonne ; la suite d'une ligne garde
    /// son retrait.
    fn wrapped(&self) -> Vec<String> {
        let right = self.columns().1;
        let mut wrapped = Vec::new();
        for line in &self.detail {
            let indent: String = line.chars().take_while(|c| *c == ' ').take(right / 2).collect();
            let mut current = String::new();
            for word in line.split(' ').filter(|w| !w.is_empty()) {
                let mut word: Vec<char> = word.chars().collect();
                loop {
                    let used = if current.is_empty() { indent.chars().count() } else { current.chars().count() + 1 };
                    if used + word.len() <= right {
                        current = if current.is_empty() { format!("{indent}{}", word.iter().collect::<String>()) } else { format!("{current} {}", word.iter().collect::<String>()) };
                        break;
                    }
                    if !current.is_empty() {
                        wrapped.push(std::mem::take(&mut current));
                        continue;
                    }
                    // un mot plus long que la colonne : coupé
                    let room = right.saturating_sub(indent.chars().count()).max(1);
                    wrapped.push(format!("{indent}{}", word.drain(..room.min(word.len())).collect::<String>()));
                    if word.is_empty() {
                        break;
                    }
                }
            }
            if !current.is_empty() || line.trim().is_empty() {
                wrapped.push(current);
            }
        }
        wrapped
    }

    /// Les marques, d'après ce qui est installé.
    pub fn mark(&mut self, installed: &std::collections::BTreeMap<String, String>) {
        for item in &mut self.items {
            item.mark = match installed.get(&item.name) {
                None => Mark::Absent,
                Some(id) if *id == item.id => Mark::Same,
                Some(_) => Mark::Other,
            };
        }
    }

    /// L'écran, ligne par ligne ; avec chaque ligne, si elle est en évidence.
    pub fn view(&self) -> Vec<(String, bool)> {
        let width = self.size.0;
        let body = self.body();
        let (left, right) = self.columns();
        // tout passe par `shown` : une colonne par caractère, rien d'invisible
        let cut = |text: &str, max: usize| -> String {
            let text = shown(text);
            let mut out: String = text.chars().take(max).collect();
            if text.chars().count() > max && max > 0 {
                out.pop();
                out.push('…');
            }
            format!("{out:<max$}")
        };
        let wrapped = self.wrapped();
        let first = self.selected.saturating_sub(body.saturating_sub(1));
        let place = self.dir.as_ref().map(|d| format!(" · installés dans {}", d.display())).unwrap_or_else(|| " · lecture seule (pas de --dir)".into());
        let mut lines = vec![(cut(&format!(" Paquets : {}{place}", self.origin), width), true)];
        for row in 0..body {
            let item = self.items.get(first + row);
            let label = match item {
                Some(item) => format!("{} {} {}", match item.mark { Mark::Same => "✓", Mark::Other => "≠", Mark::Unknown => "?", Mark::Absent => " " }, item.name, item.version),
                None => String::new(),
            };
            let chosen = item.is_some() && first + row == self.selected;
            let detail = wrapped.get(self.scroll + row).map(String::as_str).unwrap_or("");
            lines.push((format!("{}{} │ {}", if chosen { "▸" } else { " " }, cut(&label, left - 1), cut(detail, right)), false));
        }
        // où en est la lecture, d'abord : cela se voit même sur un terminal étroit
        let read = if wrapped.len() > self.scroll + body { format!("lu : {} lignes sur {} (PgSuiv : la suite)", self.scroll + body, wrapped.len()) } else { "lu jusqu'au bout".into() };
        lines.push((cut(&format!(" {read} · ✓ installé · ≠ autre contenu · ? inconnu"), width), false));
        lines.push((cut(&format!(" {}", self.status), width), !self.status.is_empty()));
        lines.push((cut(&format!(" {KEYS}"), width), false));
        lines
    }
}

/// Remet le terminal en état, quoi qu'il arrive.
struct Screen;

impl Screen {
    fn enter() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen, crossterm::cursor::Hide, crossterm::event::EnableBracketedPaste)?;
        // une panique rend aussi le terminal
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            Self::leave();
            before(info);
        }));
        Ok(Self)
    }

    fn leave() {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste, crossterm::cursor::Show, crossterm::terminal::LeaveAlternateScreen);
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        Self::leave();
    }
}

fn draw(browser: &mut Browser) -> Result<()> {
    use crossterm::{cursor::MoveTo, queue, style::{Attribute, Print, SetAttribute}, terminal::{Clear, ClearType}};
    let (width, height) = crossterm::terminal::size()?;
    browser.resize(width as usize, height as usize);
    let mut out = std::io::stdout();
    queue!(out, Clear(ClearType::All))?;
    for (row, (line, strong)) in browser.view().into_iter().enumerate() {
        queue!(out, MoveTo(0, row as u16))?;
        if strong {
            queue!(out, SetAttribute(Attribute::Reverse), Print(line), SetAttribute(Attribute::Reset))?;
        } else {
            queue!(out, Print(line))?;
        }
    }
    out.flush()?;
    Ok(())
}

fn installed(dir: Option<&Path>) -> Result<std::collections::BTreeMap<String, String>> {
    Ok(match dir {
        Some(dir) => pack::load_installed(dir, false)?.ids,
        None => Default::default(),
    })
}

/// Une erreur sur une ligne, pour la ligne d'état.
fn brief(error: &anyhow::Error) -> String {
    crate::clean(&format!("{error:#}"), 400, false)
}

/// L'écran des paquets. `dir` : où installer (sans lui, on ne fait que lire).
pub fn run(source: &dyn Source, dir: Option<PathBuf>) -> Result<()> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        bail!("`pack tui` demande un terminal. Sans terminal : pack list, pack show, pack install");
    }
    let items = source.list()?.into_iter().map(|(name, version, description, id)| Item { name, version, description, id, mark: Mark::Absent }).collect::<Vec<_>>();
    if items.is_empty() {
        bail!("aucun paquet à montrer");
    }
    let mut browser = Browser::new(items, dir);
    browser.origin = source.label();
    // les marques après une action : si la vérification échoue, on le dit sans effacer le
    // compte rendu de l'action, et on n'affirme plus rien
    let remark = |browser: &mut Browser| match installed(browser.dir.as_deref()) {
        Ok(ids) => browser.mark(&ids),
        Err(error) => {
            browser.unmark();
            browser.status = format!("{} Paquets installés invérifiables : {}", browser.status, brief(&error)).trim().to_string();
        }
    };
    // le paquet lu est gardé : c'est lui, et pas une relecture, qui sera installé
    let load = |browser: &mut Browser| -> Option<Pack> {
        let name = browser.current().map(|item| item.name.clone())?;
        match source.load(&name) {
            Ok(pack) => {
                browser.set_detail(&pack.describe());
                // la marque se fonde sur le contenu lu, pas sur ce qu'une liste annonce
                let id = pack.id();
                if let Some(item) = browser.items.get_mut(browser.selected) {
                    item.id = id;
                }
                Some(pack)
            }
            Err(error) => {
                browser.set_detail(&format!("Ce paquet ne peut pas être lu :\n{}", crate::clean(&format!("{error:#}"), 2000, true)));
                None
            }
        }
    };
    let mut loaded = load(&mut browser);
    remark(&mut browser);
    let mut changed = false;
    let screen = Screen::enter()?;
    loop {
        draw(&mut browser)?;
        if browser.asking() {
            // la question est à l'écran : ce qui a été tapé ou collé avant ne lui répond pas
            while crossterm::event::poll(std::time::Duration::ZERO)? {
                crossterm::event::read()?;
            }
        }
        let key = match crossterm::event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Key::Char('q'),
                // Alt-o, Ctrl-o… ne sont pas un « o »
                _ if !(key.modifiers - KeyModifiers::SHIFT).is_empty() => Key::Other,
                KeyCode::Up | KeyCode::Char('k') => Key::Up,
                KeyCode::Down | KeyCode::Char('j') => Key::Down,
                KeyCode::PageUp => Key::PageUp,
                KeyCode::PageDown | KeyCode::Char(' ') => Key::PageDown,
                KeyCode::Esc => Key::Char('q'),
                KeyCode::Char(c) => Key::Char(c),
                _ => Key::Other,
            },
            // un texte collé n'est pas une suite de touches
            Event::Paste(_) => Key::Other,
            _ => continue,
        };
        match browser.key(key) {
            Ask::Nothing => {}
            Ask::Quit => break,
            Ask::Load => {
                browser.status.clear();
                loaded = load(&mut browser);
                remark(&mut browser);
            }
            Ask::Install { name, replace } => {
                browser.status = match (&loaded, browser.dir.clone()) {
                    (Some(pack), Some(dir)) if pack.manifest.name == name => match pack.install(&dir, replace, false) {
                        Ok(dest) => {
                            changed = true;
                            format!("Installé dans {}. Le maître le prend à son prochain démarrage.", dest.display())
                        }
                        Err(error) => format!("Non installé : {}", brief(&error)),
                    },
                    _ => "Non installé : ce paquet n'a pas pu être lu.".into(),
                };
                remark(&mut browser);
            }
            Ask::Remove { name } => {
                browser.status = match browser.dir.clone().map(|dir| pack::remove(&dir, &name)) {
                    Some(Ok(())) => {
                        changed = true;
                        format!("« {name} » retiré. Le maître le perd à son prochain démarrage.")
                    }
                    Some(Err(error)) => format!("Non retiré : {}", brief(&error)),
                    None => "Non retiré : pas de dossier de paquets.".into(),
                };
                remark(&mut browser);
            }
            Ask::Verify => {
                browser.status = match installed(browser.dir.as_deref()) {
                    Ok(_) if browser.dir.is_none() => "Aucun dossier de paquets donné (--dir) : lecture seule.".into(),
                    Ok(ids) => format!("{} paquet(s) installé(s), empreintes intactes.", ids.len()),
                    Err(error) => format!("Vérification : {}", brief(&error)),
                };
            }
        }
    }
    drop(screen);
    if changed {
        println!("Les paquets installés ont changé : relancez le maître pour qu'il les prenne (./tools.sh run, ou sudo systemctl restart talixio-agent).");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser() -> Browser {
        let item = |name: &str, id: &str| Item { name: name.into(), version: "1".into(), description: "d".into(), id: id.into(), mark: Mark::Absent };
        let mut b = Browser::new(vec![item("alpha", "a"), item("beta", "b"), item("gamma", "c")], Some("/opt/talixio/packs".into()));
        b.mark(&[("alpha".to_string(), "a".to_string()), ("beta".to_string(), "autre".to_string())].into_iter().collect());
        b
    }

    #[test]
    fn rien_ne_s_installe_sans_un_oui() {
        let mut b = browser();
        assert_eq!([b.items[0].mark, b.items[1].mark, b.items[2].mark], [Mark::Same, Mark::Other, Mark::Absent]);
        assert_eq!(b.key(Key::Up), Ask::Nothing);
        assert_eq!(b.key(Key::Down), Ask::Load);
        // « i » pose la question ; toute autre touche que « o » l'annule
        assert_eq!(b.key(Key::Char('i')), Ask::Nothing);
        assert!(b.status.contains("Remplacer « beta »"));
        assert_eq!(b.key(Key::Char('i')), Ask::Nothing);
        assert!(b.status.contains("Rien n'a été fait"));
        assert_eq!(b.key(Key::Char('o')), Ask::Nothing, "un « o » sans question ne fait rien");
        b.key(Key::Char('i'));
        assert_eq!(b.key(Key::Char('o')), Ask::Install { name: "beta".into(), replace: true });
        b.key(Key::Down);
        b.key(Key::Char('i'));
        assert_eq!(b.key(Key::Char('o')), Ask::Install { name: "gamma".into(), replace: false });
        // retirer : seulement ce qui est installé, et après un oui
        assert_eq!(b.key(Key::Char('r')), Ask::Nothing);
        assert!(b.status.contains("n'est pas installé"));
        b.key(Key::Up);
        b.key(Key::Char('r'));
        assert_eq!(b.key(Key::Char('n')), Ask::Nothing);
        b.key(Key::Char('r'));
        assert_eq!(b.key(Key::Char('o')), Ask::Remove { name: "beta".into() });
        assert_eq!(b.key(Key::Char('q')), Ask::Quit);
        // sans dossier : on lit, on n'installe pas
        let mut reading = Browser::new(b.items.clone(), None);
        assert_eq!(reading.key(Key::Char('i')), Ask::Nothing);
        assert!(reading.status.contains("--dir"));
        assert_eq!(reading.key(Key::Char('o')), Ask::Nothing);
    }

    #[test]
    fn l_ecran_tient_dans_sa_taille_et_reste_propre() {
        let mut b = browser();
        b.set_detail(&format!("titre\n\u{1b}[31mrouge\u{7}\n{}\n\tretrait", "x".repeat(300)));
        for (width, height) in [(80, 24), (40, 8), (200, 50), (10, 3)] {
            b.resize(width, height);
            let view = b.view();
            assert!(view.len() <= height.max(5), "{width}x{height}");
            for (line, _) in &view {
                assert_eq!(line.chars().count(), width.max(40), "{line:?}");
                assert!(line.chars().all(|c| shown(&c.to_string()) == c.to_string()), "{line:?}");
            }
        }
        b.resize(80, 24);
        let view = b.view();
        assert!(view[1].0.starts_with("▸✓ alpha 1") && view[2].0.contains("≠ beta 1") && view[1].0.contains("│ titre"));
        assert!(view[2].0.contains("?[31mrouge?"), "ce qui commande le terminal se voit, et ne commande plus");
    }

    #[test]
    fn tout_le_texte_d_un_paquet_peut_etre_lu() {
        // une seule ligne très longue, des caractères larges, invisibles, d'autres écritures :
        // la fin reste atteignable, et rien ne déborde ni ne se cache
        let mut b = browser();
        let long = format!("debut {} FIN_DU_TEXTE", "mot ".repeat(4000));
        b.set_detail(&format!("{long}\n表表表表表表表表表表表表表表表表表表表表表表表表表表表表表表 CACHE\ninvisible\u{e0041}\u{e0042}\u{200b}\u{ad}ici\nfausse\u{a0}\u{a0}ligne"));
        for (width, height) in [(80, 24), (50, 9), (120, 6)] {
            b.resize(width, height);
            b.scroll = 0;
            let mut seen = String::new();
            for _ in 0..5000 {
                for (line, _) in &b.view()[1..] {
                    if let Some((_, detail)) = line.split_once("│ ") {
                        seen.push_str(detail.trim_end());
                        seen.push(' ');
                    }
                }
                let before = b.scroll;
                b.key(Key::PageDown);
                if b.scroll == before {
                    break;
                }
            }
            assert!(seen.contains("FIN_DU_TEXTE") && seen.contains("CACHE") && seen.contains("invisible????ici") && seen.contains("fausse ligne"), "{width}x{height}");
            assert!(b.view().iter().any(|(line, _)| line.contains("lu jusqu'au bout")));
            assert!(seen.matches("mot").count() >= 4000, "aucune ligne sautée ({width}x{height})");
        }
        // sans vérification possible, aucune marque n'est affirmée, et l'on peut encore retirer
        b.unmark();
        assert!(b.view()[1].0.starts_with("▸? alpha"));
        b.key(Key::Char('r'));
        assert_eq!(b.key(Key::Char('o')), Ask::Remove { name: "alpha".into() });
    }
}
