//! Dépôt de paquets : des fichiers statiques derrière https (ou dans un dossier), avec un index
//! signé. Rien n'en est accepté sans signature d'une clé connue : celles de l'exécutable, ou
//! celles que l'exploitant donne lui-même.
//!
//! ```text
//! index.toml          la liste des paquets : nom, version, identifiant, empreinte de chaque fichier
//! index.toml.sig      la signature Ed25519 de l'index, en base64
//! <nom>/<version>/…   les fichiers du paquet
//! ```
//!
//! L'index est signé ; chaque fichier est vérifié par l'empreinte que l'index en donne ; le
//! paquet reconstitué doit avoir l'identifiant annoncé. Le serveur n'a donc pas à être de
//! confiance : il ne peut que refuser de servir.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::Engine;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};

use crate::pack::{sha256_hex, Pack};

/// Les clés publiques de Talixio (base64), pour le dépôt officiel. Aucune pour l'instant : tant
/// qu'il n'y en a pas, un dépôt n'est lu qu'avec une clé donnée par l'exploitant (`--key`).
pub const TRUSTED_KEYS: &[&str] = &[];

pub const INDEX: &str = "index.toml";
pub const SIGNATURE: &str = "index.toml.sig";
/// Ce qui est signé : ce préfixe, puis l'index. Une signature faite pour autre chose ne vaut pas ici.
const DOMAIN: &[u8] = b"talixio-paquets-v1\n";
const MAX_INDEX: u64 = 1024 * 1024;
const MAX_PACKS: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub version: String,
    pub description: String,
    /// L'identifiant du paquet (empreinte de tout son contenu), comme `pack show` le donne.
    pub id: String,
    #[serde(rename = "file")]
    pub files: Vec<FileEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    format: u32,
    /// Secondes depuis 1970, à la publication.
    issued: u64,
    #[serde(default, rename = "pack")]
    packs: Vec<Entry>,
}

/// D'où viennent les fichiers d'un dépôt.
pub trait Fetch {
    /// Le fichier à ce chemin relatif, `max` octets au plus.
    fn get(&self, rel: &str, max: u64) -> Result<Vec<u8>>;
}

/// Un dépôt dans un dossier : celui que `pack publish` vient d'écrire, ou une copie.
pub struct DirFetch(pub PathBuf);

impl Fetch for DirFetch {
    fn get(&self, rel: &str, max: u64) -> Result<Vec<u8>> {
        use std::io::Read;
        let path = self.0.join(rel);
        // ni lien, ni tube : un fichier ordinaire, ou rien
        if !path.symlink_metadata().is_ok_and(|m| m.is_file()) {
            bail!("{} : absent, ou pas un fichier ordinaire", path.display());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path).with_context(|| format!("{}", path.display()))?.take(max + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max {
            bail!("{} : trop gros", path.display());
        }
        Ok(bytes)
    }
}

/// Un dépôt derrière https. Aucune redirection n'est suivie, aucune autre adresse n'est jointe.
pub struct HttpsFetch {
    base: reqwest::Url,
    client: reqwest::Client,
    /// Tout le dépôt doit avoir répondu avant : un serveur qui traîne ne retient pas l'exploitant.
    deadline: std::time::Instant,
}

impl HttpsFetch {
    pub fn new(base: &str) -> Result<Self> {
        let url = reqwest::Url::parse(base).ok().filter(|u| {
            u.scheme() == "https" && u.host_str().is_some_and(|h| !h.is_empty()) && u.username().is_empty() && u.password().is_none() && u.query().is_none() && u.fragment().is_none()
        });
        let odd = base.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\');
        let host_written = base.strip_prefix("https://").is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'));
        let Some(base) = url.filter(|_| host_written && base.len() <= 500 && !odd) else {
            bail!("un dépôt de paquets s'écrit https://hôte/chemin (sans identifiants ni paramètres)");
        };
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { base, client, deadline: std::time::Instant::now() + std::time::Duration::from_secs(300) })
    }
}

impl Fetch for HttpsFetch {
    fn get(&self, rel: &str, max: u64) -> Result<Vec<u8>> {
        // chaque morceau du chemin est ajouté comme un segment : aucun ne devient hôte ni paramètre
        let mut url = self.base.clone();
        url.path_segments_mut().map_err(|_| anyhow::anyhow!("adresse de dépôt invalide"))?.pop_if_empty().extend(rel.split('/'));
        let job = async {
            let mut response = self.client.get(url.clone()).send().await.map_err(|e| anyhow::anyhow!("{url} injoignable : {}", crate::clean(&e.to_string(), 300, false)))?;
            if !response.status().is_success() {
                bail!("{url} : réponse {}", response.status().as_u16());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if (bytes.len() + chunk.len()) as u64 > max {
                    bail!("{url} : trop gros");
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        };
        let left = self.deadline.saturating_duration_since(std::time::Instant::now());
        let bounded = async { tokio::time::timeout(left, job).await.unwrap_or_else(|_| bail!("le dépôt met trop de temps à répondre (5 minutes en tout)")) };
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(bounded))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Les points d'ordre faible de la courbe, sous tous leurs encodages.
const WEAK_KEYS: &[&str] = &[
    "0100000000000000000000000000000000000000000000000000000000000000",
    "0100000000000000000000000000000000000000000000000000000000000080",
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000080",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
    "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    "edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    "eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
];

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// Une clé publique écrite en base64 : 32 octets.
fn public_key(text: &str) -> Result<Vec<u8>> {
    let bytes = b64().decode(text.trim()).ok().filter(|b| b.len() == 32);
    // une « clé » d'ordre faible laisserait n'importe qui signer : ce n'est pas une clé
    if bytes.as_ref().is_some_and(|b| WEAK_KEYS.iter().any(|weak| hex(b) == *weak)) {
        bail!("clé publique refusée : ce n'est pas une vraie clé (point d'ordre faible), n'importe qui pourrait signer pour elle");
    }
    bytes.with_context(|| format!("clé publique invalide (32 octets en base64) : « {} »", crate::clean(text, 80, false)))
}

/// Fabrique une clé de signature : la partie privée (PKCS#8 en base64), à garder fermée, et la
/// partie publique, à donner à ceux qui liront le dépôt.
pub fn keygen() -> Result<(String, String)> {
    let rng = ring::rand::SystemRandom::new();
    let private = Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| anyhow::anyhow!("génération de la clé impossible"))?;
    let pair = Ed25519KeyPair::from_pkcs8(private.as_ref()).map_err(|_| anyhow::anyhow!("clé illisible"))?;
    Ok((b64().encode(private.as_ref()), b64().encode(pair.public_key().as_ref())))
}

fn key_pair(private: &str) -> Result<Ed25519KeyPair> {
    let bytes = b64().decode(private.trim()).context("clé privée illisible (base64)")?;
    Ed25519KeyPair::from_pkcs8(&bytes).map_err(|_| anyhow::anyhow!("clé privée invalide : attendue telle que `pack keygen` l'écrit"))
}

/// La partie publique d'une clé privée.
pub fn public_of(private: &str) -> Result<String> {
    Ok(b64().encode(key_pair(private)?.public_key().as_ref()))
}

fn signed(index: &[u8]) -> Vec<u8> {
    [DOMAIN, index].concat()
}

/// Écrit un dépôt : les paquets du dossier `from` (un sous-dossier par paquet), leur index, sa
/// signature. Le dossier `out` est créé ; s'il existe, il doit être vide.
pub fn publish(from: &Path, out: &Path, private: &str, issued: u64) -> Result<Vec<Entry>> {
    let pair = key_pair(private)?;
    let packs = Pack::available(Some(from))?;
    if packs.is_empty() {
        bail!("aucun paquet dans {}", from.display());
    }
    std::fs::create_dir_all(out).with_context(|| format!("{}", out.display()))?;
    if std::fs::read_dir(out)?.next().is_some() {
        bail!("{} n'est pas vide : un dépôt s'écrit dans un dossier neuf", out.display());
    }
    let mut entries = Vec::new();
    for pack in &packs {
        let m = &pack.manifest;
        let dir = out.join(&m.name).join(&m.version);
        std::fs::create_dir_all(&dir)?;
        let mut files = Vec::new();
        for (name, bytes) in &pack.files {
            // une clé de signature rangée par mégarde dans un paquet ne part pas avec lui
            let text = String::from_utf8_lossy(bytes);
            if text.trim() == private.trim() || key_pair(&text).is_ok() {
                bail!("paquet « {} », fichier {name} : c'est une clé de signature. Rien n'est publié : sortez-la du dossier des paquets", m.name);
            }
            std::fs::write(dir.join(name), bytes)?;
            files.push(FileEntry { name: name.clone(), sha256: sha256_hex(bytes), size: bytes.len() as u64 });
        }
        entries.push(Entry { name: m.name.clone(), version: m.version.clone(), description: m.description.clone(), id: pack.id(), files });
    }
    let index = toml::to_string(&Index { format: 1, issued, packs: entries.clone() })?;
    std::fs::write(out.join(INDEX), &index)?;
    std::fs::write(out.join(SIGNATURE), format!("{}\n", b64().encode(pair.sign(&signed(index.as_bytes())).as_ref())))?;
    Ok(entries)
}

/// Un dépôt dont l'index a été vérifié.
pub struct Depot {
    pub entries: Vec<Entry>,
    /// La clé publique qui a signé l'index.
    pub signer: String,
    pub issued: u64,
}

/// Lit l'index d'un dépôt et vérifie sa signature contre les clés connues. Sans clé, ou sans
/// signature valable de l'une d'elles, rien n'est lu.
pub fn open(fetch: &dyn Fetch, keys: &[String]) -> Result<Depot> {
    let keys: Vec<&str> = TRUSTED_KEYS.iter().copied().chain(keys.iter().map(String::as_str)).collect();
    if keys.is_empty() {
        bail!("aucune clé de confiance : donnez la clé publique de ce dépôt avec `--key <clé>` (ou TALIXIO_PACK_KEY). Un dépôt n'est jamais lu sans vérifier sa signature");
    }
    let index = fetch.get(INDEX, MAX_INDEX)?;
    let signature = fetch.get(SIGNATURE, 1024).context("signature de l'index")?;
    let signature = b64().decode(String::from_utf8_lossy(&signature).trim()).context("signature de l'index illisible")?;
    let mut signer = None;
    for key in keys {
        let bytes = public_key(key)?;
        if UnparsedPublicKey::new(&ED25519, &bytes).verify(&signed(&index), &signature).is_ok() {
            signer = Some(key.trim().to_string());
            break;
        }
    }
    let signer = signer.context("l'index de ce dépôt n'est signé par aucune des clés de confiance : rien n'en est lu")?;
    // à partir d'ici, l'index vient bien du détenteur de la clé ; il reste à le lire avec prudence
    let index: Index = toml::from_str(std::str::from_utf8(&index).context("index illisible")?).map_err(|e| anyhow::anyhow!("index invalide : {}", crate::clean(&e.to_string(), 600, true)))?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if index.issued > now + 86_400 {
        bail!("index invalide : il est daté dans le futur");
    }
    if index.format != 1 {
        bail!("format d'index inconnu ({}) : mettez talixio-agent à jour", index.format);
    }
    if index.packs.len() > MAX_PACKS {
        bail!("index trop long");
    }
    let mut seen = std::collections::BTreeSet::new();
    for entry in &index.packs {
        let hex = |text: &str| text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        let plain = crate::pack::plain_name(&entry.name) && crate::pack::plain_name(&entry.version) && crate::pack::plain_line(&entry.description, 300);
        let files = !entry.files.is_empty()
            && entry.files.len() <= crate::pack::MAX_FILES
            && entry.files.iter().all(|f| crate::pack::plain_name(&f.name) && hex(&f.sha256) && f.size <= crate::pack::MAX_FILE);
        if !plain || !hex(&entry.id) || !files || !seen.insert(&entry.name) {
            bail!("index invalide : entrée « {} »", crate::clean(&entry.name, 80, false));
        }
    }
    Ok(Depot { entries: index.packs, signer, issued: index.issued })
}

/// Une date lisible (AAAA-MM-JJ, temps universel) pour des secondes depuis 1970.
pub fn day(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

impl Depot {
    /// Télécharge un paquet et le vérifie : chaque fichier contre l'empreinte de l'index, puis
    /// le paquet entier contre son identifiant.
    pub fn pack(&self, fetch: &dyn Fetch, name: &str) -> Result<Pack> {
        let entry = self.entries.iter().find(|e| e.name == name).with_context(|| format!("paquet « {} » absent de ce dépôt (talixio-agent pack list --from …)", crate::clean(name, 80, false)))?;
        let mut files = BTreeMap::new();
        for file in &entry.files {
            let bytes = fetch.get(&format!("{}/{}/{}", entry.name, entry.version, file.name), file.size)?;
            if bytes.len() as u64 != file.size || sha256_hex(&bytes) != file.sha256 {
                bail!("paquet « {name} », fichier {} : ce que le dépôt sert n'est pas ce que l'index signé annonce", file.name);
            }
            files.insert(file.name.clone(), bytes);
        }
        let mut pack = Pack::parse(files).with_context(|| format!("paquet « {name} »"))?;
        pack.origin = Some((self.signer.clone(), self.issued));
        if pack.id() != entry.id || pack.manifest.name != entry.name || pack.manifest.version != entry.version {
            bail!("paquet « {name} » : son contenu n'a pas l'identifiant que l'index signé annonce");
        }
        Ok(pack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(dir: &Path) {
        let pack = dir.join("bonjour");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join("pack.toml"), "name = \"bonjour\"\nversion = \"1\"\ndescription = \"Dit bonjour.\"\n\n[[command]]\nprogram = \"salut\"\nargs = [\"*\"]\n").unwrap();
        std::fs::write(pack.join("salut"), "#!/bin/sh\necho bonjour \"$1\"\n").unwrap();
    }

    #[test]
    fn un_depot_signe_se_lit_et_rien_d_autre() {
        let tmp = tempfile::tempdir().unwrap();
        source(&tmp.path().join("src"));
        let (private, public) = keygen().unwrap();
        assert_eq!(public_of(&private).unwrap(), public);
        let out = tmp.path().join("depot");
        let published = publish(&tmp.path().join("src"), &out, &private, 1_700_000_000).unwrap();
        assert_eq!(published.len(), 1);
        assert!(publish(&tmp.path().join("src"), &out, &private, 1).unwrap_err().to_string().contains("pas vide"));
        let fetch = DirFetch(out.clone());

        // sans clé, ou avec une autre clé : rien
        assert!(open(&fetch, &[]).err().unwrap().to_string().contains("aucune clé de confiance"));
        let (_, other) = keygen().unwrap();
        assert!(open(&fetch, std::slice::from_ref(&other)).err().unwrap().to_string().contains("aucune des clés"));
        assert!(open(&fetch, &["pas une clé".into()]).is_err());

        // avec la bonne (parmi d'autres) : le paquet est celui du dossier source
        let depot = open(&fetch, &[other, public.clone()]).unwrap();
        assert_eq!(depot.signer, public);
        let pack = depot.pack(&fetch, "bonjour").unwrap();
        assert_eq!(pack.id(), published[0].id);
        assert_eq!(pack.id(), Pack::find(Some(&tmp.path().join("src")), "bonjour").unwrap().id());
        assert!(depot.pack(&fetch, "absent").is_err());

        // un fichier changé chez l'hébergeur : refusé, même de la même taille
        let served = out.join("bonjour/1/salut");
        std::fs::write(&served, "#!/bin/sh\necho pirate! \"$1\"\n").unwrap();
        assert!(depot.pack(&fetch, "bonjour").err().unwrap().to_string().contains("n'est pas ce que l'index signé annonce"));

        // l'index retouché, ou une signature d'un autre index : refusé
        let index = std::fs::read_to_string(out.join(INDEX)).unwrap();
        std::fs::write(out.join(INDEX), index.replace("Dit bonjour.", "Dit bonsoir.")).unwrap();
        assert!(open(&fetch, std::slice::from_ref(&public)).err().unwrap().to_string().contains("aucune des clés"));
        std::fs::write(out.join(INDEX), &index).unwrap();
        assert!(open(&fetch, std::slice::from_ref(&public)).is_ok());
        std::fs::remove_file(out.join(SIGNATURE)).unwrap();
        assert!(open(&fetch, &[public]).is_err());
    }

    #[test]
    fn un_index_signe_reste_lu_avec_prudence() {
        let tmp = tempfile::tempdir().unwrap();
        let (private, public) = keygen().unwrap();
        let pair = key_pair(&private).unwrap();
        let sha = "a".repeat(64);
        for (name, file) in [("../etc", "salut"), ("bonjour", "../../x"), ("bonjour", ".cache"), ("-rf", "salut")] {
            let index = format!("format = 1\nissued = 1\n\n[[pack]]\nname = \"{name}\"\nversion = \"1\"\ndescription = \"d\"\nid = \"{sha}\"\n\n[[pack.file]]\nname = \"{file}\"\nsha256 = \"{sha}\"\nsize = 3\n");
            std::fs::write(tmp.path().join(INDEX), &index).unwrap();
            std::fs::write(tmp.path().join(SIGNATURE), b64().encode(pair.sign(&signed(index.as_bytes())).as_ref())).unwrap();
            assert!(open(&DirFetch(tmp.path().into()), std::slice::from_ref(&public)).err().unwrap().to_string().contains("index invalide"), "{name} {file}");
        }
        // une signature sans le préfixe du domaine ne vaut pas
        let index = "format = 1\nissued = 1\n";
        std::fs::write(tmp.path().join(INDEX), index).unwrap();
        std::fs::write(tmp.path().join(SIGNATURE), b64().encode(pair.sign(index.as_bytes()).as_ref())).unwrap();
        assert!(open(&DirFetch(tmp.path().into()), std::slice::from_ref(&public)).is_err());
        // une « clé » d'ordre faible, pour laquelle tout le monde sait signer : refusée
        let neutral = b64().encode([&[1u8][..], &[0u8; 31]].concat());
        std::fs::write(tmp.path().join(SIGNATURE), b64().encode([&[1u8][..], &[0u8; 63]].concat())).unwrap();
        assert!(open(&DirFetch(tmp.path().into()), &[neutral]).err().unwrap().to_string().contains("ordre faible"));
        // un index daté dans le futur
        let index = format!("format = 1\nissued = {}\n", u64::MAX / 2);
        std::fs::write(tmp.path().join(INDEX), &index).unwrap();
        std::fs::write(tmp.path().join(SIGNATURE), b64().encode(pair.sign(&signed(index.as_bytes())).as_ref())).unwrap();
        assert!(open(&DirFetch(tmp.path().into()), std::slice::from_ref(&public)).err().unwrap().to_string().contains("futur"));
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(1_795_305_600), "2026-11-22");
        assert_eq!(day(951_782_400), "2000-02-29");
        assert!(HttpsFetch::new("https://").is_err() && HttpsFetch::new("https:///x").is_err() && HttpsFetch::new("https://exemple.fr/p?x=1").is_err());
        assert!(HttpsFetch::new("http://exemple.fr/p").is_err() && HttpsFetch::new("https://u:p@exemple.fr/p").is_err() && HttpsFetch::new("https://exemple.fr/p/").is_ok());
    }
}
