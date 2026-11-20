//! Chiffrement des connexions : TLS 1.3, avec certificat épinglé.
//!
//! Chaque agent qui écoute a son certificat, signé par lui-même. Personne ne fait confiance
//! à une autorité : celui qui se connecte connaît d'avance l'empreinte (SHA-256) du
//! certificat attendu, reçue par un canal sûr — l'entrée standard pour un enfant, le dossier
//! d'état pour la ligne de commande — et refuse tout autre certificat.

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, ServerConfig, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::config::ensure_private;

/// Nom porté par les certificats que les agents se fabriquent. Il n'est pas vérifié : seule
/// l'empreinte compte.
pub const NAME: &str = "talixio-agent";

/// Le certificat d'un agent qui écoute.
#[derive(Clone)]
pub struct Identity {
    pub server: Arc<ServerConfig>,
    /// Empreinte SHA-256 du certificat, en hexadécimal : ce que les correspondants épinglent.
    pub fingerprint: String,
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

pub fn fingerprint(cert: &[u8]) -> String {
    Sha256::digest(cert).iter().map(|b| format!("{b:02x}")).collect()
}

fn identity(certs: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>) -> Result<Identity> {
    let leaf = certs.first().ok_or_else(|| anyhow!("aucun certificat"))?;
    let fingerprint = fingerprint(leaf);
    let server = ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("certificat ou clé inutilisable")?;
    Ok(Identity { server: Arc::new(server), fingerprint })
}

fn generate() -> Result<rcgen::CertifiedKey> {
    rcgen::generate_simple_self_signed(vec![NAME.to_string()]).context("création du certificat")
}

/// Un certificat neuf, gardé en mémoire : pour un agent enfant qui a lui-même des enfants.
pub fn ephemeral() -> Result<Identity> {
    let made = generate()?;
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(made.key_pair.serialize_der()));
    identity(vec![made.cert.der().clone()], key)
}

/// Le certificat du maître : celui du dossier d'état, créé au premier démarrage. Son
/// empreinte est écrite à côté, pour la ligne de commande.
pub fn load_or_create(state_dir: &Path) -> Result<Identity> {
    let (cert_path, key_path) = (state_dir.join("tls-cert.pem"), state_dir.join("tls-key.pem"));
    // une paire incomplète (arrêt entre les deux écritures) est refaite
    if key_path.symlink_metadata().is_err() || cert_path.symlink_metadata().is_err() {
        let _ = std::fs::remove_file(&key_path);
        let made = generate()?;
        write_private(&key_path, &made.key_pair.serialize_pem())?;
        std::fs::write(&cert_path, made.cert.pem()).with_context(|| format!("{}", cert_path.display()))?;
    }
    let identity = from_files(&cert_path, &key_path)?;
    let print = state_dir.join("tls-fingerprint");
    std::fs::write(&print, &identity.fingerprint).with_context(|| format!("{}", print.display()))?;
    Ok(identity)
}

/// Un certificat fourni par l'exploitant (par exemple délivré par une autorité reconnue,
/// pour que les navigateurs l'acceptent). La clé doit être fermée aux autres comptes.
pub fn from_files(cert: &Path, key: &Path) -> Result<Identity> {
    ensure_private(key)?;
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .map_err(|e| anyhow!("{} : {e}", cert.display()))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow!("{} : {e}", cert.display()))?;
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| anyhow!("{} : {e}", key.display()))?;
    identity(certs, key)
}

fn write_private(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).and_then(|mut f| f.write_all(content.as_bytes())).with_context(|| format!("{}", path.display()))
}

/// N'accepte qu'un certificat : celui dont on connaît l'empreinte.
#[derive(Debug)]
struct Pinned {
    fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        if crate::secret_eq(&fingerprint(end_entity), &self.fingerprint) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("certificat inattendu : ce n'est pas le correspondant prévu".into()))
        }
    }

    // la possession de la clé du certificat épinglé est bien vérifiée : c'est la signature de la poignée de main
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Configuration d'un client qui n'accepte que le certificat d'empreinte donnée.
pub fn pinned_client(fingerprint: &str) -> Result<ClientConfig> {
    let fingerprint = fingerprint.trim().to_ascii_lowercase();
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("empreinte de certificat invalide : 64 caractères hexadécimaux attendus");
    }
    let provider = provider();
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned { fingerprint, provider }))
        .with_no_client_auth();
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_master_certificate_is_kept_and_its_key_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create(dir.path()).unwrap();
        let again = load_or_create(dir.path()).unwrap();
        assert_eq!(first.fingerprint, again.fingerprint);
        assert_eq!(std::fs::read_to_string(dir.path().join("tls-fingerprint")).unwrap(), first.fingerprint);
        assert_ne!(ephemeral().unwrap().fingerprint, first.fingerprint);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("tls-key.pem")).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
        assert!(pinned_client("pas-une-empreinte").is_err());
        assert!(pinned_client(&first.fingerprint).is_ok());
    }
}
