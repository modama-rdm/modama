use std::{
    fs::{create_dir_all, File, Permissions},
    io::{Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
    string::FromUtf8Error,
};

#[cfg(target_os = "linux")]
use std::{
    fs::set_permissions,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
};

#[cfg(target_os = "windows")]
impl NoopMode for OpenOptions {
    fn mode(&mut self, _mode: u32) -> &mut OpenOptions {
        self
    }
}

#[cfg(target_os = "linux")]
fn set_perms_priv(path: &Path) -> Result<(), CAError> {
    let perms = Permissions::from_mode(0o700);
    set_permissions(path, perms)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_perms_priv(path: &Path) -> Result<(), CAError> {
    Ok(())
}

use pkcs8::{der::EncodePem, DecodePrivateKey, LineEnding, PrivateKeyInfoOwned};
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

    #[error("invalid password")]
    InvalidPassword,

    #[error("conversion failed: {0}")]
    Conversion(#[from] pkcs8::Error),

    #[error("encryption failed: {0}")]
    Encryption(#[from] pkcs8::der::Error),

    #[error("permission error: {0}")]
    InvalidPermissions(String),

    #[error("no save path configured")]
    NoSavePath,
}

pub struct Ca {
    path_prefix: Option<PathBuf>,
    password: String,
    key_pair: KeyPair,
    ca_cert_pem: String,
    ca_params: CertificateParams,
}

pub struct KeyPairAndCert {
    pub cert: Certificate,
    pub key_pair: KeyPair,
}

impl Ca {
    pub fn new<P>(path_prefix: Option<P>, password: &str) -> Result<Self, CAError>
    where
        P: Into<PathBuf>,
    {
        let (key_pair, cert) = Self::generate_ca_cert()?;

        Ok(Self {
            path_prefix: path_prefix.map(|p| p.into()),
            key_pair,
            ca_cert_pem: cert.pem(),
            ca_params: Self::ca_params()?,
            password: password.to_owned(),
        })
    }

    fn get_path_prefix(&self) -> Result<PathBuf, CAError> {
        self.path_prefix.clone().ok_or(CAError::NoSavePath)
    }

    pub fn get_ca_cert_pem(&self) -> &str {
        &self.ca_cert_pem
    }

    fn check_priv_perms(path_prefix: &Path) -> Result<(), CAError> {
        let priv_path = Self::get_ca_priv_path(path_prefix);
        let mode = std::fs::metadata(&priv_path)?.mode();
        if mode & 0o77 != 0 {
            return Err(CAError::InvalidPermissions(format!(
                "group and other must not have access to private path {priv_path:?}; permission mask is 0o{:o}",
                mode & 0o777
            )));
        }
        Ok(())
    }

    pub fn load<P>(path_prefix: P, password: &str) -> Result<Self, CAError>
    where
        P: Into<PathBuf>,
    {
        let mut opts = File::options();
        opts.read(true);
        let prefix: PathBuf = path_prefix.into();
        Self::check_priv_perms(&prefix)?;
        let mut privkey_enc = Vec::new();
        opts.open(Self::get_privkey_path(&prefix))?
            .read_to_end(&mut privkey_enc)?;

        let privkey_enc = String::from_utf8(privkey_enc)?;

        let privkey = PrivateKeyInfoOwned::from_pkcs8_encrypted_pem(&privkey_enc, password)
            .map_err(|e| match e {
                pkcs8::Error::EncryptedPrivateKey(pkcs8::pkcs5::Error::DecryptFailed) => {
                    CAError::InvalidPassword
                }
                e => e.into(),
            })?;

        let mut pubkey = Vec::new();
        opts.open(Self::get_pubkey_path(&prefix))?
            .read_to_end(&mut pubkey)?;
        let mut key_pair_pem = Vec::new();
        key_pair_pem.extend(privkey.to_pem(LineEnding::LF)?.into_bytes());
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
            password: password.to_owned(),
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
        opts.write(true).create_new(true);
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
        opts.mode(0o600)
            .open(privkey_path)?
            .write_all(key_pair.serialize_pem().as_bytes())?;

        let chain_path = dir.join(format!("{name_prefix}.chain.pem"));
        let mut chain = opts.mode(0o644).open(chain_path)?;
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
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        params.use_authority_key_identifier_extension = true;
        params.key_identifier_method = rcgen::KeyIdMethod::Sha256;
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
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        params.key_identifier_method = rcgen::KeyIdMethod::Sha256;
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

    fn issuer<'a>(&'a self) -> Issuer<'a, &'a KeyPair> {
        Issuer::from_params(&self.ca_params, &self.key_pair)
    }

    pub fn serialize_ca_to_files(&self) -> Result<(), CAError> {
        let pubkey_serialized = self.key_pair.public_key_pem();
        let privkey_serialized = self.key_pair.serialize_pem();

        let pk_info = PrivateKeyInfoOwned::from_pkcs8_pem(&privkey_serialized)?;
        let pk_encrypted = pk_info.encrypt(&self.password)?;
        let privkey_encrypted = pk_encrypted.to_pem("ENCRYPTED PRIVATE KEY", LineEnding::LF)?;

        let prefix = &self.get_path_prefix()?;

        let ca_dir = Self::get_ca_path(prefix);
        let ca_dir_priv = Self::get_ca_priv_path(prefix);
        create_dir_all(&ca_dir)?;
        create_dir_all(&ca_dir_priv)?;
        set_perms_priv(&ca_dir_priv)?;

        let mut opts = File::options();
        opts.write(true).create_new(true);

        opts.mode(0o644)
            .open(Self::get_pubkey_path(prefix))?
            .write_all(pubkey_serialized.as_bytes())?;
        opts.mode(0o600)
            .open(Self::get_privkey_path(prefix))?
            .write_all(privkey_encrypted.as_bytes())?;
        opts.mode(0o644)
            .open(Self::get_cert_path(prefix))?
            .write_all(self.ca_cert_pem.as_bytes())?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::{os::unix::fs::MetadataExt, path::Path};

    use x509_parser::{
        pem::{self, Pem},
        prelude::ParsedExtension::SubjectKeyIdentifier,
    };

    use crate::{
        ca::{CAError, Ca},
        test_helpers::TempDir,
    };

    const TEST_PASSWORD: &str = "the-password";

    fn read_pem(path: &Path) -> Pem {
        let server_cert = std::fs::read(path).unwrap();
        let pem_parsed = pem::parse_x509_pem(&server_cert).unwrap().1;
        pem_parsed
    }

    #[test]
    fn test_new_ca() {
        let tmpdir = TempDir::new("test-ca");
        let ca = Ca::new(Some(tmpdir.to_owned()), TEST_PASSWORD).unwrap();
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

        let d = &*tmpdir.join("ca").join("priv");
        let mode = d.metadata().unwrap().mode();
        assert_eq!(mode & 0o777, 0o700);

        let d = &*tmpdir.join("ca").join("priv").join("ca.priv.pem");
        let mode = d.metadata().unwrap().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn test_ca_wrong_password() {
        let tmpdir = TempDir::new("test-ca");
        let ca = Ca::new(Some(tmpdir.to_owned()), "password-1").unwrap();
        ca.serialize_ca_to_files().unwrap();
        let err = Ca::load(tmpdir.to_owned(), "password-2");
        if let Err(e) = &err {
            println!("{e:?}");
        }
        assert!(matches!(err, Err(CAError::InvalidPassword)));
    }

    #[test]
    fn test_ca_make_client_and_server_certs() {
        let tmpdir = TempDir::new("test-ca");
        let ca = Ca::new(Some(tmpdir.to_owned()), TEST_PASSWORD).unwrap();

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

        let d = &*tmpdir
            .join("servers")
            .join("localhost")
            .join("localhost.priv.pem");
        let mode = d.metadata().unwrap().mode();
        assert_eq!(mode & 0o777, 0o600);

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

        let d = &*tmpdir
            .join("clients")
            .join("test_resource")
            .join("test_agent.priv.pem");
        let mode = d.metadata().unwrap().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn test_ca_load_and_make_certs() {
        let tmpdir = TempDir::new("test-ca");
        {
            let ca = Ca::new(Some(tmpdir.to_owned()), TEST_PASSWORD).unwrap();
            ca.serialize_ca_to_files().unwrap();
        }

        let ca = Ca::load(tmpdir.to_owned(), TEST_PASSWORD).unwrap();

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
    fn test_ca_includes_authority_key_identifier() {
        let tmpdir = TempDir::new("test-ca");
        {
            let ca = Ca::new(Some(tmpdir.to_owned()), TEST_PASSWORD).unwrap();
            ca.serialize_ca_to_files().unwrap();
        }

        let ca = Ca::load(tmpdir.to_owned(), TEST_PASSWORD).unwrap();

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

        let ca_pem_parsed = read_pem(&tmpdir.join("ca").join("pub").join("ca.cert.pem"));
        let cert = ca_pem_parsed.parse_x509().unwrap();
        let ca_ski = cert
            .iter_extensions()
            .find_map(|ext| match ext.parsed_extension() {
                SubjectKeyIdentifier(k) => Some(k),
                _ex => None,
            })
            .unwrap();
        let ca_ski_bytes = ca_ski.0.to_vec();

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

        let server_cert = read_pem(
            &tmpdir
                .join("servers")
                .join("localhost")
                .join("localhost.cert.pem"),
        );
        let cert = server_cert.parse_x509().unwrap();
        let server_aki = cert
            .iter_extensions()
            .find_map(|ext| match ext.parsed_extension() {
                x509_parser::prelude::ParsedExtension::AuthorityKeyIdentifier(aki) => {
                    aki.key_identifier.clone()
                }
                ex => {
                    eprintln!("other extension: {ex:?}");
                    None
                }
            })
            .unwrap();
        let aki_bytes = server_aki.0.to_vec();
        assert_eq!(&aki_bytes, &ca_ski_bytes);

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

        let client_cert = read_pem(
            &tmpdir
                .join("clients")
                .join("test_resource")
                .join("test_agent.cert.pem"),
        );
        let cert = client_cert.parse_x509().unwrap();
        let client_aki = cert
            .iter_extensions()
            .find_map(|ext| match ext.parsed_extension() {
                x509_parser::prelude::ParsedExtension::AuthorityKeyIdentifier(aki) => {
                    aki.key_identifier.clone()
                }
                ex => {
                    eprintln!("other extension: {ex:?}");
                    None
                }
            })
            .unwrap();
        let aki_bytes = client_aki.0.to_vec();
        assert_eq!(&aki_bytes, &ca_ski_bytes);
    }

    #[test]
    fn test_ca_must_have_one_server_name() {
        let tmpdir = TempDir::new("test-ca");
        let ca = Ca::new(Some(tmpdir.to_owned()), TEST_PASSWORD).unwrap();

        // both when making a cert..
        let server_names_empty = &[];
        assert!(matches!(
            ca.server_cert(server_names_empty),
            Err(CAError::InvalidParam(_))
        ));

        // ... and when saving it, an empty server name list is an error:
        let server_names = &["localhost".to_owned(), "smbdrop".to_owned()];
        let key_and_cert = ca.server_cert(server_names).unwrap();
        assert!(matches!(
            ca.save_server_cert(server_names_empty, &key_and_cert),
            Err(CAError::InvalidParam(_))
        ));
    }
}
