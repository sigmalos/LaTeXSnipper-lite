//! Loopback TLS identity. Office for Mac only loads add-in pages over HTTPS.
//!
//! The identity is one self-signed, non-CA leaf for `localhost` and
//! `127.0.0.1`: trusting it lets Office reach the local server, and because
//! it cannot sign other certificates a leaked key never extends trust beyond
//! this machine's loopback service. The files (`localhost.crt` /
//! `localhost.key`, PEM) are those of the original add-in, so a certificate
//! it created and the user trusted keeps working.

use anyhow::{anyhow, bail, Context, Result};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub const COMMON_NAME: &str = "LaTeXSnipper Office Add-in (localhost)";
/// Apple rejects TLS server certificates valid for more than 825 days.
const VALIDITY_DAYS: i64 = 800;
const RENEW_BEFORE_SECONDS: u64 = 30 * 86_400;
const SECURITY: &str = "/usr/bin/security";
const OPENSSL: &str = "/usr/bin/openssl";

#[derive(Debug, Clone)]
pub struct LocalCertificate {
    pub cert: PathBuf,
    pub key: PathBuf,
}

fn run(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("无法执行 {program}"))?;
    if !out.status.success() {
        let detail = String::from_utf8_lossy(if out.stderr.is_empty() {
            &out.stdout
        } else {
            &out.stderr
        })
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
        let name = Path::new(program)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(program);
        bail!(
            "{name} 失败：{}",
            detail.chars().take(300).collect::<String>()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn login_keychain() -> PathBuf {
    crate::paths::home().join("Library/Keychains/login.keychain-db")
}

impl LocalCertificate {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            cert: dir.join("localhost.crt"),
            key: dir.join("localhost.key"),
        }
    }

    pub fn exists(&self) -> bool {
        self.cert.is_file() && self.key.is_file()
    }

    /// Missing, unreadable, or expiring within 30 days.
    pub fn needs_renewal(&self) -> bool {
        if !self.exists() {
            return true;
        }
        let seconds = RENEW_BEFORE_SECONDS.to_string();
        let cert = self.cert.to_string_lossy();
        run(
            OPENSSL,
            &["x509", "-noout", "-checkend", &seconds, "-in", &cert],
        )
        .is_err()
    }

    pub fn der(&self) -> Result<CertificateDer<'static>> {
        CertificateDer::from_pem_file(&self.cert)
            .map_err(|e| anyhow!("无法读取 Office 加载项证书：{e:?}"))
    }

    /// SHA-256 of the DER certificate, as `security delete-certificate -Z` accepts.
    pub fn fingerprint(&self) -> Result<String> {
        Ok(Sha256::digest(self.der()?.as_ref())
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect())
    }

    /// Write a new certificate and key, replacing any previous pair.
    pub fn generate(&self) -> Result<()> {
        let dir = self.cert.parent().unwrap_or(Path::new("."));
        crate::paths::ensure_private_dir(dir)?;
        let mut params = CertificateParams::default();
        params.subject_alt_names = vec![
            SanType::DnsName("localhost".try_into()?),
            SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        ];
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, COMMON_NAME);
        dn.push(DnType::OrganizationName, "LaTeXSnipper");
        params.distinguished_name = dn;
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let now = time::OffsetDateTime::now_utc();
        params.not_before = now - time::Duration::days(1);
        params.not_after = now + time::Duration::days(VALIDITY_DAYS);
        let key = KeyPair::generate()?;
        let cert = params.self_signed(&key)?;
        write_private(&self.key, key.serialize_pem().as_bytes())?;
        crate::paths::write_atomic(&self.cert, cert.pem().as_bytes())?;
        Ok(())
    }

    pub fn tls_config(&self) -> Result<Arc<rustls::ServerConfig>> {
        let key = PrivateKeyDer::from_pem_file(&self.key)
            .map_err(|e| anyhow!("无法读取 Office 加载项私钥：{e:?}"))?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
            .with_no_client_auth()
            .with_single_cert(vec![self.der()?], key)?;
        Ok(Arc::new(config))
    }

    /// Whether macOS accepts the certificate for SSL to `localhost` with the user's settings.
    pub fn is_trusted(&self) -> bool {
        self.exists()
            && run(
                SECURITY,
                &[
                    "verify-cert",
                    "-c",
                    &self.cert.to_string_lossy(),
                    "-p",
                    "ssl",
                    "-s",
                    "localhost",
                    "-L",
                    "-q",
                ],
            )
            .is_ok()
    }

    /// Add a user-level SSL trust setting; macOS asks the user to authorize it.
    pub fn trust(&self) -> Result<()> {
        if !self.exists() {
            bail!("Office 加载项证书不存在。");
        }
        let keychain = login_keychain();
        run(
            SECURITY,
            &[
                "add-trusted-cert",
                "-r",
                "trustRoot",
                "-p",
                "ssl",
                "-k",
                &keychain.to_string_lossy(),
                &self.cert.to_string_lossy(),
            ],
        )?;
        Ok(())
    }

    /// Remove this add-in's trust setting and keychain item; absent items are fine.
    pub fn untrust(&self) {
        if !self.exists() {
            return;
        }
        let _ = run(
            SECURITY,
            &["remove-trusted-cert", &self.cert.to_string_lossy()],
        );
        if let Ok(hash) = self.fingerprint() {
            let _ = run(
                SECURITY,
                &[
                    "delete-certificate",
                    "-Z",
                    &hash,
                    &login_keychain().to_string_lossy(),
                ],
            );
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("key.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(bytes)?;
    drop(f);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Debug)]
pub enum Prepared {
    Ready(LocalCertificate),
    /// Missing, renewed or untrusted, and the caller may not prompt.
    NeedsTrust,
}

/// A trusted identity, created or renewed when needed. Without `interactive`
/// (the login agent) nothing prompts: an identity that needs the user's
/// approval reports `NeedsTrust`.
pub fn prepare(dir: &Path, interactive: bool) -> Result<Prepared> {
    let identity = LocalCertificate::in_dir(dir);
    let renewed = identity.needs_renewal();
    if renewed {
        if !interactive {
            return Ok(Prepared::NeedsTrust);
        }
        identity.untrust();
        identity.generate().context("无法创建 Office 加载项证书")?;
    }
    if renewed || !identity.is_trusted() {
        if !interactive {
            return Ok(Prepared::NeedsTrust);
        }
        identity.trust().map_err(|e| anyhow!("TRUST_DENIED:{e}"))?;
    }
    if !identity.is_trusted() {
        bail!("TRUST_DENIED:macOS 未信任 Office 加载项证书，Office 将无法加载插件页面。");
    }
    Ok(Prepared::Ready(identity))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_a_private_pem_pair_that_rustls_loads() {
        let dir = tempfile::tempdir().unwrap();
        let identity = LocalCertificate::in_dir(&dir.path().join("state"));
        assert!(!identity.exists() && identity.needs_renewal());
        identity.generate().unwrap();
        assert!(identity.exists() && !identity.needs_renewal());
        assert!(std::fs::read_to_string(&identity.cert)
            .unwrap()
            .starts_with("-----BEGIN CERTIFICATE-----"));
        identity.tls_config().unwrap();
        assert_eq!(identity.fingerprint().unwrap().len(), 64);
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&identity.key), 0o600);
        assert_eq!(mode(identity.cert.parent().unwrap()), 0o700);
        let before = identity.der().unwrap();
        identity.generate().unwrap();
        assert_ne!(identity.der().unwrap(), before);
    }

    /// An RSA identity in the original add-in's format (openssl req) still loads.
    #[test]
    fn loads_the_original_openssl_rsa_identity() {
        let dir = tempfile::tempdir().unwrap();
        let identity = LocalCertificate::in_dir(dir.path());
        let config = dir.path().join("openssl.cnf");
        std::fs::write(
            &config,
            format!("[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN={COMMON_NAME}\n[ext]\nbasicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n"),
        )
        .unwrap();
        let status = Command::new(OPENSSL)
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "800",
                "-config",
            ])
            .arg(&config)
            .arg("-keyout")
            .arg(&identity.key)
            .arg("-out")
            .arg(&identity.cert)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert!(!identity.needs_renewal());
        identity.tls_config().unwrap();
    }

    /// macOS's TLS policy accepts the certificate for `localhost` when it is
    /// the anchor; `verify-cert -r` never touches the user's trust settings.
    #[test]
    fn macos_accepts_the_certificate_for_localhost() {
        let dir = tempfile::tempdir().unwrap();
        let identity = LocalCertificate::in_dir(dir.path());
        identity.generate().unwrap();
        let verify = |name: &str| {
            Command::new(SECURITY)
                .args([
                    "verify-cert",
                    "-p",
                    "ssl",
                    "-L",
                    "-q",
                    "-N",
                    "-n",
                    name,
                    "-c",
                ])
                .arg(&identity.cert)
                .arg("-r")
                .arg(&identity.cert)
                .status()
                .unwrap()
                .success()
        };
        assert!(verify("localhost"));
        assert!(!verify("example.com"));
    }

    #[test]
    fn the_login_agent_never_prompts() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            prepare(dir.path(), false).unwrap(),
            Prepared::NeedsTrust
        ));
        assert!(!LocalCertificate::in_dir(dir.path()).exists());
    }
}
