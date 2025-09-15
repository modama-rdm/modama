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
}

pub struct Ca {
    path_prefix: PathBuf,
    key_pair: KeyPair,
    ca_cert_pem: String,
    ca_params: CertificateParams,
}

pub struct KeyPairAndCert {
    pub cert: Certificate,
    pub key_pair: KeyPair,
}

impl Ca {
    pub fn new<P>(path_prefix: P) -> Result<Self, CAError>
    where
        P: Into<PathBuf>,
    {
        let (key_pair, cert) = Self::generate_ca_cert()?;

        Ok(Self {
            path_prefix: path_prefix.into(),
            key_pair,
            ca_cert_pem: cert.pem(),
            ca_params: Self::ca_params()?,
        })
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
            path_prefix: prefix,
            key_pair,
            ca_cert_pem: String::from_utf8(cert_pem)?,
            ca_params: Self::ca_params()?,
        })
    }

    fn get_ca_path(prefix: &Path) -> PathBuf {
        PathBuf::from(prefix).join("ca")
    }

    fn get_client_certs_path(prefix: &Path) -> PathBuf {
        PathBuf::from(prefix).join("clients")
    }

    fn get_server_certs_path(prefix: &Path) -> PathBuf {
        PathBuf::from(prefix).join("servers")
    }

    fn get_pubkey_path(prefix: &Path) -> PathBuf {
        Self::get_ca_path(prefix).join("ca.pub.pem")
    }

    fn get_privkey_path(prefix: &Path) -> PathBuf {
        Self::get_ca_path(prefix).join("ca.priv.pem")
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
        params.extended_key_usages = Vec::new();
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ClientAuth);
        params
            .subject_alt_names
            .push(rcgen::SanType::URI(Ia5String::from_str(&format!(
                "x-modama-agent://{resource_name}#{agent_name}"
            ))?));
        let client_cert = params.signed_by(&key_pair, &self.issuer())?;

        Ok(KeyPairAndCert { cert: client_cert, key_pair })
    }

    pub fn save_client_cert(
        &self,
        agent_name: &str,
        resource_name: &str,
        key_and_cert: &KeyPairAndCert,
    ) -> Result<(), CAError> {
        let resource_path = Self::get_client_certs_path(&self.path_prefix).join(resource_name);
        create_dir_all(&resource_path)?;

        self.write_signed_cert_files(&resource_path, agent_name, &key_and_cert.cert, &key_and_cert.key_pair)?;
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
        params.extended_key_usages = Vec::new();
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);
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

        let server_certs_path = Self::get_server_certs_path(&self.path_prefix);
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

        let ca_dir = Self::get_ca_path(&self.path_prefix);
        create_dir_all(ca_dir)?;

        let mut opts = File::options();
        opts.write(true).create_new(true).mode(0o600);

        opts.mode(0o644)
            .open(Self::get_pubkey_path(&self.path_prefix))?
            .write_all(pubkey_serialized.as_bytes())?;
        opts.open(Self::get_privkey_path(&self.path_prefix))?
            .write_all(privkey_serialized.as_bytes())?;
        opts.mode(0o644)
            .open(Self::get_cert_path(&self.path_prefix))?
            .write_all(self.ca_cert_pem.as_bytes())?;

        Ok(())
    }
}
