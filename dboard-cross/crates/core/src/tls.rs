//! rustls configuration shared by the Postgres driver.

use crate::model::{ConnectionConfig, SslMode};
use crate::Error;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::PrivateKeyDer;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use std::sync::Arc;

pub fn client_config(c: &ConnectionConfig) -> crate::Result<ClientConfig> {
    let mode = c.ssl;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("protocol versions");

    let load_certs = |path: &str| -> crate::Result<Vec<CertificateDer<'static>>> {
        CertificateDer::pem_file_iter(path.trim())
            .map_err(|e| Error::Db(format!("Cannot read certificate file {path}: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Db(format!("Invalid certificate in {path}: {e}")))
    };
    let custom_ca = !c.ssl_ca.trim().is_empty();
    // Verify the server when asked to (Verify full), or whenever the user supplied a CA to trust.
    let verify = mode == SslMode::VerifyFull || (custom_ca && mode != SslMode::Disable);
    let wants_roots = if verify {
        let mut roots = RootCertStore::empty();
        if custom_ca {
            for cert in load_certs(&c.ssl_ca)? {
                roots.add(cert).map_err(|e| Error::Db(format!("Bad CA certificate: {e}")))?;
            }
        } else {
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        }
        builder.with_root_certificates(roots)
    } else {
        // Prefer / Require behave like libpq: encrypt, but don't authenticate the server.
        builder.dangerous().with_custom_certificate_verifier(Arc::new(NoVerify(provider.signature_verification_algorithms)))
    };
    if c.ssl_cert.trim().is_empty() {
        return Ok(wants_roots.with_no_client_auth());
    }
    let chain = load_certs(&c.ssl_cert)?;
    let key_path = if c.ssl_key.trim().is_empty() { c.ssl_cert.trim() } else { c.ssl_key.trim() };
    let key = PrivateKeyDer::from_pem_file(key_path).map_err(|e| Error::Db(format!("Cannot read private key {key_path}: {e}")))?;
    wants_roots.with_client_auth_cert(chain, key).map_err(|e| Error::Db(format!("Client certificate rejected: {e}")))
}

#[derive(Debug)]
struct NoVerify(rustls::crypto::WebPkiSupportedAlgorithms);

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.0)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.0)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBdjCCARugAwIBAgIUN5AK5OR1laGY4zvM+bnfaQeriSowCgYIKoZIzj0EAwIw
DzENMAsGA1UEAwwEdGVzdDAgFw0yNjEwMDMxNDQ5MjZaGA8yMTI2MDkwOTE0NDky
NlowDzENMAsGA1UEAwwEdGVzdDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABGEk
t7nb+5nRmDt6wcFesS6beAeUGv4sFKVY0d2MJTQilt4K6YebzqvDkZiGGdEuuBPn
z6TdHT5RBhmPpG3vLQ2jUzBRMB0GA1UdDgQWBBTpo5gwXXmenJM+4MgA9zLHDgnW
2zAfBgNVHSMEGDAWgBTpo5gwXXmenJM+4MgA9zLHDgnW2zAPBgNVHRMBAf8EBTAD
AQH/MAoGCCqGSM49BAMCA0kAMEYCIQDLAHXQ2C7OGvebd4qvMveS/sSoOu+AyBMr
G37DliO7fgIhAK1aE0oBgGZxqjbLRTAR/6AbTuV2JWAiArwJUi6lsDN0
-----END CERTIFICATE-----
";
    const KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg4gVjGvBUzafp3fAN
qq1nlyXxp4XqYPT184GgqbRHXxyhRANCAARhJLe52/uZ0Zg7esHBXrEum3gHlBr+
LBSlWNHdjCU0IpbeCumHm86rw5GYhhnRLrgT58+k3R0+UQYZj6Rt7y0N
-----END PRIVATE KEY-----
";

    fn write(name: &str, body: &str) -> String {
        let p = std::env::temp_dir().join(format!("dboard-tls-test-{}-{name}", std::process::id()));
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn loads_custom_ca_and_client_identity() {
        let ca = write("ca.pem", CERT);
        let key = write("key.pem", KEY);
        let c = ConnectionConfig { ssl: SslMode::Require, ssl_ca: ca.clone(), ssl_cert: ca, ssl_key: key, ..Default::default() };
        assert!(client_config(&c).is_ok());
    }

    #[test]
    fn reports_missing_files() {
        let c = ConnectionConfig { ssl: SslMode::VerifyFull, ssl_ca: "/no/such/ca.pem".into(), ..Default::default() };
        assert!(client_config(&c).unwrap_err().to_string().contains("ca.pem"));
    }
}
