use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{info, warn};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, SignatureScheme};
use sha2::{Digest, Sha256};

/// Trust-on-first-use certificate verifier.
///
/// On first connection, accepts any certificate and saves its SHA-256
/// fingerprint to disk. On subsequent connections, verifies that the
/// certificate fingerprint matches the pinned value.
#[derive(Debug)]
pub struct TofuVerifier {
    pin_path: PathBuf,
    pinned: Mutex<Option<String>>,
}

impl TofuVerifier {
    pub fn new(state_dir: &Path) -> Self {
        let pin_path = state_dir.join("router-cert.pin");
        let pinned = match fs::read_to_string(&pin_path) {
            Ok(fp) => {
                let fp = fp.trim().to_string();
                info!("loaded pinned certificate fingerprint from {}", pin_path.display());
                Some(fp)
            }
            Err(_) => None,
        };
        Self {
            pin_path,
            pinned: Mutex::new(pinned),
        }
    }

    fn fingerprint(cert: &CertificateDer<'_>) -> String {
        let hash = Sha256::digest(cert.as_ref());
        hex::encode(hash)
    }

    fn save_pin(&self, fingerprint: &str) {
        if let Some(parent) = self.pin_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(&self.pin_path, fingerprint) {
            warn!("failed to save certificate pin to {}: {e}", self.pin_path.display());
        }
    }
}

impl ServerCertVerifier for TofuVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let fp = Self::fingerprint(end_entity);
        let mut pinned = self.pinned.lock().unwrap();

        match pinned.as_deref() {
            Some(existing) if existing == fp => Ok(ServerCertVerified::assertion()),
            Some(existing) => {
                warn!(
                    "certificate fingerprint mismatch! pinned={existing}, got={fp}. \
                     If the router certificate was regenerated, delete {} to re-pin.",
                    self.pin_path.display()
                );
                Err(Error::General(
                    "certificate fingerprint does not match pinned value".to_string(),
                ))
            }
            None => {
                info!("pinning router certificate (sha256={fp})");
                self.save_pin(&fp);
                *pinned = Some(fp);
                Ok(ServerCertVerified::assertion())
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}
