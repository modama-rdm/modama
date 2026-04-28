use std::{
    fs::{create_dir_all, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
    string::FromUtf8Error,
};

#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;

#[cfg(target_os = "windows")]
impl NoopMode for OpenOptions {
    fn mode(&mut self, _mode: u32) -> &mut OpenOptions {
        self
    }
}

use rcgen::{
    string::Ia5String, Certificate, CertificateParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, PKCS_ED25519,
};

#[derive(thiserror::Error, Debug)]
pub enum CAError {
    #[error("i/o error: {0}")]
    IO(#[from] std::io::Error),

    #[error("certificate generation error: {0}")]
    RcGen(#[from] rcgen::Error),

    #[error("unicode error: {0}")]
    FromUtf8(#[from] FromUtf8Error),

    #[error("invalid parameter: {0}")]
    InvalidParam(String),

    #[error("no save path configured")]
    NoSavePath,
}

pub struct Ca {
    path_prefix: Option<PathBuf>,
    key_pair: KeyPair,
    ca_cert_pem: String,
    ca_params: CertificateParams,
}

pub struct KeyPairAndCert {
    pub cert: Certificate,
    pub key_pair: KeyPair,
}

impl Ca {
    pub fn new<P>(path_prefix: Option<P>) -> Result<Self, CAError>
    where
        P: Into<PathBuf>,
    {
        let (key_pair, cert) = Self::generate_ca_cert()?;

        Ok(Self {
            path_prefix: path_prefix.map(|p| p.into()),
            key_pair,
            ca_cert_pem: cert.pem(),
            ca_params: Self::ca_params()?,
        })
    }

    fn get_path_prefix(&self) -> Result<PathBuf, CAError> {
        self.path_prefix.clone().ok_or(CAError::NoSavePath)
    }

    pub fn get_ca_cert_pem(&self) -> &str {
        &self.ca_cert_pem
    }

    pub fn load<P>(path_prefix: P) -> Result<Self, CAError>
    where
        P: Into<PathBuf>,
    {
        let mut opts = File::options();
        opts.read(true);
        let prefix: PathBuf = path_prefix.into();
        let mut privkey = Vec::new();
        opts.open(Self::get_privkey_path(&prefix))?
            .read_to_end(&mut privkey)?;
        let mut pubkey = Vec::new();
        opts.open(Self::get_pubkey_path(&prefix))?
            .read_to_end(&mut pubkey)?;
        let mut key_pair_pem = Vec::new();
        key_pair_pem.extend(privkey);
        key_pair_pem.extend(pubkey);
        let key_pair = KeyPair::from_pem(&String::from_utf8(key_pair_pem)?)?;
        let mut cert_pem = Vec::new();
        opts.open(Self::get_cert_path(&prefix))?
            .read_to_end(&mut cert_pem)?;

        Ok(Self {
            path_prefix: Some(prefix),
            key_pair,
            ca_cert_pem: String::from_utf8(cert_pem)?,
            ca_params: Self::ca_params()?,
        })
    }

    fn get_ca_path(prefix: &Path) -> PathBuf {
        PathBuf::from(prefix).join("ca").join("pub")
    }

    fn get_ca_priv_path(prefix: &Path) -> PathBuf {
        PathBuf::from(prefix).join("ca").join("priv")
    }

    fn get_client_certs_path(prefix: &Path) -> PathBuf {
        PathBuf::from(prefix).join("clients")
    }

    fn get_server_certs_path(prefix: &Path, hostname: &str) -> PathBuf {
        PathBuf::from(prefix).join("servers").join(hostname)
    }

    fn get_pubkey_path(prefix: &Path) -> PathBuf {
        Self::get_ca_path(prefix).join("ca.pub.pem")
    }

    fn get_privkey_path(prefix: &Path) -> PathBuf {
        Self::get_ca_priv_path(prefix).join("ca.priv.pem")
    }
    fn get_cert_path(prefix: &Path) -> PathBuf {
        Self::get_ca_path(prefix).join("ca.cert.pem")
    }

    /// Serialize certificate and keypair to filesystem as PEM.
    ///
    /// Will write files into `dir`, with a prefix of `name_prefix`
    /// and suffixes `.cert.pem`, `.pub.pem`, `.chain.pem`, and `.priv.pem`.
    fn write_signed_cert_files(
        &self,
        dir: &Path,
        name_prefix: &str,
        cert: &Certificate,
        key_pair: &KeyPair,
    ) -> Result<(), CAError> {
        let cert_signed_pem = cert.pem();
        let mut opts = File::options();
        opts.write(true).create_new(true).mode(0o600);
        create_dir_all(dir)?;

        let cert_path = dir.join(format!("{name_prefix}.cert.pem"));
        opts.mode(0o644)
            .open(cert_path)?
            .write_all(cert_signed_pem.as_bytes())?;

        let pubkey_path = dir.join(format!("{name_prefix}.pub.pem"));
        opts.mode(0o644)
            .open(pubkey_path)?
            .write_all(key_pair.public_key_pem().as_bytes())?;

        let privkey_path = dir.join(format!("{name_prefix}.priv.pem"));
        opts.open(privkey_path)?
            .write_all(key_pair.serialize_pem().as_bytes())?;

        let chain_path = dir.join(format!("{name_prefix}.chain.pem"));
        let mut chain = opts.open(chain_path)?;
        chain.write_all(cert_signed_pem.as_bytes())?;
        chain.write_all(self.ca_cert_pem.as_bytes())?;

        Ok(())
    }

    pub fn client_cert(
        &self,
        agent_name: &str,
        resource_name: &str,
    ) -> Result<KeyPairAndCert, CAError> {
        let mut params = CertificateParams::new(vec![])?;
        assert!(!agent_name.contains('@'));
        assert!(!agent_name.contains('/'));
        assert!(!agent_name.contains('#'));
        assert!(!resource_name.contains('@'));
        assert!(!resource_name.contains('/'));
        assert!(!resource_name.contains('#'));
        let full_name: String = format!("{agent_name}@{resource_name}");
        // FIXME: instead of generating a key pair here, generate it on the
        // agent itself and only generate the final certificate from a CSR

        let key_pair = KeyPair::generate_for(&PKCS_ED25519)?;
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, full_name);
        params.is_ca = IsCa::NoCa;
        // FIXME: constrain key usage?
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        params
            .subject_alt_names
            .push(rcgen::SanType::URI(Ia5String::from_str(&format!(
                "x-modama-agent://{resource_name}#{agent_name}"
            ))?));
        let client_cert = params.signed_by(&key_pair, &self.issuer())?;

        Ok(KeyPairAndCert {
            cert: client_cert,
            key_pair,
        })
    }

    pub fn save_client_cert(
        &self,
        agent_name: &str,
        resource_name: &str,
        key_and_cert: &KeyPairAndCert,
    ) -> Result<(), CAError> {
        let resource_path =
            Self::get_client_certs_path(&self.get_path_prefix()?).join(resource_name);
        create_dir_all(&resource_path)?;

        self.write_signed_cert_files(
            &resource_path,
            agent_name,
            &key_and_cert.cert,
            &key_and_cert.key_pair,
        )?;
        Ok(())
    }

    pub fn server_cert(&self, server_names: &[String]) -> Result<KeyPairAndCert, CAError> {
        let mut params = CertificateParams::new(server_names)?;

        // FIXME: instead of generating a key pair here, generate it on the
        // server system itself and only generate the final certificate from a
        // CSR
        let key_pair = KeyPair::generate_for(&PKCS_ED25519)?;
        let first_name = server_names.first().ok_or_else(|| {
            CAError::InvalidParam("must have at least one server name".to_owned())
        })?;
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, first_name);
        params.is_ca = IsCa::NoCa;
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
        ];
        let server_cert: Certificate = params.signed_by(&key_pair, &self.issuer())?;

        Ok(KeyPairAndCert {
            cert: server_cert,
            key_pair,
        })
    }

    pub fn save_server_cert(
        &self,
        server_names: &[String],
        key_and_cert: &KeyPairAndCert,
    ) -> Result<(), CAError> {
        let first_name = server_names.first().ok_or_else(|| {
            CAError::InvalidParam("must have at least one server name".to_owned())
        })?;

        let server_certs_path = Self::get_server_certs_path(&self.get_path_prefix()?, first_name);
        self.write_signed_cert_files(
            &server_certs_path,
            first_name,
            &key_and_cert.cert,
            &key_and_cert.key_pair,
        )?;

        Ok(())
    }

    fn generate_ca_cert() -> Result<(KeyPair, Certificate), CAError> {
        let key_pair = KeyPair::generate_for(&PKCS_ED25519)?;
        let cert = Self::ca_params()?.self_signed(&key_pair)?;
        Ok((key_pair, cert))
    }

    fn ca_params() -> Result<CertificateParams, CAError> {
        let mut params = CertificateParams::new(vec![])?;
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, "modama CA root certificate");
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Constrained(1));
        // FIXME: constrain key usage?
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::Any,
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        Ok(params)
    }

    fn issuer<'a>(&'a self) -> Issuer<'a, KeyPair> {
        Issuer::from_params(&self.ca_params, &self.key_pair)
    }

    pub fn serialize_ca_to_files(&self) -> Result<(), CAError> {
        let pubkey_serialized = self.key_pair.public_key_pem();
        let privkey_serialized = self.key_pair.serialize_pem();

        let prefix = &self.get_path_prefix()?;

        let ca_dir = Self::get_ca_path(prefix);
        let ca_dir_priv = Self::get_ca_priv_path(prefix);
        create_dir_all(ca_dir)?;
        create_dir_all(ca_dir_priv)?;

        let mut opts = File::options();
        opts.write(true).create_new(true).mode(0o600);

        opts.mode(0o644)
            .open(Self::get_pubkey_path(prefix))?
            .write_all(pubkey_serialized.as_bytes())?;
        opts.open(Self::get_privkey_path(prefix))?
            .write_all(privkey_serialized.as_bytes())?;
        opts.mode(0o644)
            .open(Self::get_cert_path(prefix))?
            .write_all(self.ca_cert_pem.as_bytes())?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::{ca::{CAError, Ca}, test_helpers::TempDir};

    #[test]
    fn test_new_ca() {
        let tmpdir = TempDir::new("test-ca");
        let ca = Ca::new(Some(tmpdir.to_owned())).unwrap();
        ca.serialize_ca_to_files().unwrap();

        let ca_dir = std::fs::read_dir(&*tmpdir.join("ca").join("pub")).unwrap();
        let mut ca_files = ca_dir
            .into_iter()
            .map(Result::unwrap)
            .map(|e| e.file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        ca_files.sort();
        assert_eq!(ca_files, &["ca.cert.pem", "ca.pub.pem"]);
        let ca_dir_priv = std::fs::read_dir(&*tmpdir.join("ca").join("priv")).unwrap();
        let mut ca_files_priv = ca_dir_priv
            .into_iter()
            .map(Result::unwrap)
            .map(|e| e.file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        ca_files_priv.sort();
        assert_eq!(ca_files_priv, &["ca.priv.pem"]);
    }

    #[test]
    fn test_ca_make_client_and_server_certs() {
        let tmpdir = TempDir::new("test-ca");
        let ca = Ca::new(Some(tmpdir.to_owned())).unwrap();

        let server_names = &["localhost".to_owned(), "smbdrop".to_owned()];
        let key_and_cert = ca.server_cert(server_names).unwrap();
        ca.save_server_cert(server_names, &key_and_cert).unwrap();

        let agent_name = "test_agent";
        let resource_name = "test_resource";
        let key_and_cert = ca.client_cert(agent_name, resource_name).unwrap();
        ca.save_client_cert(agent_name, resource_name, &key_and_cert)
            .unwrap();

        ca.serialize_ca_to_files().unwrap();

        let ca_dir = std::fs::read_dir(&*tmpdir.join("ca").join("pub")).unwrap();
        let mut ca_files = ca_dir
            .into_iter()
            .map(Result::unwrap)
            .map(|e| e.file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        ca_files.sort();
        assert_eq!(ca_files, &["ca.cert.pem", "ca.pub.pem"]);

        let ca_dir_priv = std::fs::read_dir(&*tmpdir.join("ca").join("priv")).unwrap();
        let mut ca_files_priv = ca_dir_priv
            .into_iter()
            .map(Result::unwrap)
            .map(|e| e.file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        ca_files_priv.sort();
        assert_eq!(ca_files_priv, &["ca.priv.pem"]);

        let servers_dir = std::fs::read_dir(&*tmpdir.join("servers").join("localhost")).unwrap();
        let mut servers_files = servers_dir
            .into_iter()
            .map(Result::unwrap)
            .map(|e| e.file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        servers_files.sort();
        assert_eq!(
            servers_files,
            &[
                "localhost.cert.pem",
                "localhost.chain.pem",
                "localhost.priv.pem",
                "localhost.pub.pem"
            ]
        );

        let clients_dir =
            std::fs::read_dir(&*tmpdir.join("clients").join("test_resource")).unwrap();
        let mut clients_files = clients_dir
            .into_iter()
            .map(Result::unwrap)
            .map(|e| e.file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        clients_files.sort();
        assert_eq!(
            clients_files,
            &[
                "test_agent.cert.pem",
                "test_agent.chain.pem",
                "test_agent.priv.pem",
                "test_agent.pub.pem"
            ]
        );
    }

    #[test]
    fn test_ca_load_and_make_certs() {
        let tmpdir = TempDir::new("test-ca");
        {
            let ca = Ca::new(Some(tmpdir.to_owned())).unwrap();
            ca.serialize_ca_to_files().unwrap();
        }

        let ca = Ca::load(tmpdir.to_owned()).unwrap();

        let server_names = &["localhost".to_owned(), "smbdrop".to_owned()];
        let key_and_cert = ca.server_cert(server_names).unwrap();
        ca.save_server_cert(server_names, &key_and_cert).unwrap();

        let agent_name = "test_agent";
        let resource_name = "test_resource";
        let key_and_cert = ca.client_cert(agent_name, resource_name).unwrap();
        ca.save_client_cert(agent_name, resource_name, &key_and_cert)
            .unwrap();

        let ca_dir = std::fs::read_dir(&*tmpdir.join("ca").join("pub")).unwrap();
        let mut ca_files = ca_dir
            .into_iter()
            .map(Result::unwrap)
            .map(|e| e.file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        ca_files.sort();
        assert_eq!(ca_files, &["ca.cert.pem", "ca.pub.pem"]);

        let servers_dir = std::fs::read_dir(&*tmpdir.join("servers").join("localhost")).unwrap();
        let mut servers_files = servers_dir
            .into_iter()
            .map(Result::unwrap)
            .map(|e| e.file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        servers_files.sort();
        assert_eq!(
            servers_files,
            &[
                "localhost.cert.pem",
                "localhost.chain.pem",
                "localhost.priv.pem",
                "localhost.pub.pem"
            ]
        );

        let clients_dir =
            std::fs::read_dir(&*tmpdir.join("clients").join("test_resource")).unwrap();
        let mut clients_files = clients_dir
            .into_iter()
            .map(Result::unwrap)
            .map(|e| e.file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        clients_files.sort();
        assert_eq!(
            clients_files,
            &[
                "test_agent.cert.pem",
                "test_agent.chain.pem",
                "test_agent.priv.pem",
                "test_agent.pub.pem"
            ]
        );
    }

    #[test]
    fn test_ca_must_have_one_server_name() {
        let tmpdir = TempDir::new("test-ca");
        let ca = Ca::new(Some(tmpdir.to_owned())).unwrap();

        // both when making a cert..
        let server_names_empty = &[];
        assert!(matches!(ca.server_cert(server_names_empty), Err(CAError::InvalidParam(_))));

        // ... and when saving it, an empty server name list is an error:
        let server_names = &["localhost".to_owned(), "smbdrop".to_owned()];
        let key_and_cert = ca.server_cert(server_names).unwrap();
        assert!(matches!(
            ca.save_server_cert(server_names_empty, &key_and_cert),
            Err(CAError::InvalidParam(_)
        )));
    }
}
