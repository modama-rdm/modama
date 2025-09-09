use std::{
    fs::{create_dir_all, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use modama_common::logging::init_logging;
use rcgen::{
    Certificate, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, PKCS_ED25519,
};

#[derive(Debug, Subcommand)]
enum Commands {
    InitializeCa {},
    EnrollServer {
        server_names: Vec<String>,
    },
    EnrollAgent {
        agent_name: String,
        resource_name: String,
    },
}

#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

const CERT_PATH: &str = "./certs/";

trait NoopMode {
    fn mode(&mut self, mode: u32) -> &mut OpenOptions;
}

#[cfg(target_os = "windows")]
impl NoopMode for OpenOptions {
    fn mode(&mut self, _mode: u32) -> &mut OpenOptions {
        self
    }
}

struct Ca {
    path_prefix: PathBuf,
    cert: Certificate,
}

impl Ca {
    pub fn new<P>(path_prefix: P) -> Result<Self>
    where
        P: Into<PathBuf>,
    {
        let cert = Self::generate_ca_cert()?;
        Ok(Self {
            path_prefix: path_prefix.into(),
            cert,
        })
    }

    pub fn load<P>(path_prefix: P) -> Result<Self>
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

        let params = CertificateParams::from_ca_cert_pem(&String::from_utf8(cert_pem)?, key_pair)?;

        let cert = Certificate::from_params(params)?;

        Ok(Self {
            path_prefix: prefix,
            cert,
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
    ) -> Result<()> {
        let cert_signed_pem = cert.serialize_pem_with_signer(&self.cert)?;
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
            .write_all(cert.get_key_pair().public_key_pem().as_bytes())?;

        let privkey_path = dir.join(format!("{name_prefix}.priv.pem"));
        opts.open(privkey_path)?
            .write_all(cert.get_key_pair().serialize_pem().as_bytes())?;

        let chain_path = dir.join(format!("{name_prefix}.chain.pem"));
        let mut chain = opts.open(chain_path)?;
        chain.write_all(cert_signed_pem.as_bytes())?;
        chain.write_all(self.cert.serialize_pem()?.as_bytes())?;

        Ok(())
    }

    pub fn client_cert(&self, agent_name: &str, resource_name: &str) -> Result<Certificate> {
        let mut params = CertificateParams::new(vec![]);
        assert!(!agent_name.contains('@'));
        assert!(!agent_name.contains('/'));
        assert!(!resource_name.contains('@'));
        assert!(!resource_name.contains('/'));
        let full_name: String = format!("{agent_name}@{resource_name}");
        // FIXME: instead of generating a key pair here, generate it on the
        // agent itself and only generate the final certificate from a CSR
        let key_pair = KeyPair::generate(&PKCS_ED25519).context("generate client key pair")?;
        params.key_pair = Some(key_pair);
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, full_name);
        params.is_ca = IsCa::NoCa;
        params.alg = &PKCS_ED25519;
        // FIXME: constrain key usage?
        params.extended_key_usages = Vec::new();
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ClientAuth);
        params.subject_alt_names.push(rcgen::SanType::URI(format!(
            "x-modama-agent://{resource_name}#{agent_name}"
        )));
        let client_cert: Certificate = Certificate::from_params(params)?;

        let mut opts = File::options();
        opts.write(true).create_new(true).mode(0o644);
        let resource_path = Self::get_client_certs_path(&self.path_prefix).join(resource_name);
        create_dir_all(&resource_path)?;

        self.write_signed_cert_files(&resource_path, agent_name, &client_cert)?;

        Ok(client_cert)
    }

    pub fn server_cert(&self, server_names: &[String]) -> Result<Certificate> {
        let mut params = CertificateParams::new(server_names);

        // FIXME: instead of generating a key pair here, generate it on the
        // server system itself and only generate the final certificate from a
        // CSR
        let key_pair = KeyPair::generate(&PKCS_ED25519).context("generate server key pair")?;
        let first_name = server_names.first().context("get first server name")?;
        params.key_pair = Some(key_pair);
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, first_name);
        params.is_ca = IsCa::NoCa;
        params.alg = &PKCS_ED25519;
        params.extended_key_usages = Vec::new();
        params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);
        let server_cert: Certificate = Certificate::from_params(params)?;
        let server_certs_path = Self::get_server_certs_path(&self.path_prefix);
        self.write_signed_cert_files(&server_certs_path, first_name, &server_cert)?;

        Ok(server_cert)
    }

    fn generate_ca_cert() -> Result<Certificate> {
        let mut params = CertificateParams::new(vec![]);
        let key_pair = KeyPair::generate(&PKCS_ED25519).context("generate key pair")?;
        params.key_pair = Some(key_pair);
        params.distinguished_name = DistinguishedName::new();
        params.alg = &PKCS_ED25519;
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
        let ca_cert = Certificate::from_params(params).context("generate CA certificate")?;
        Ok(ca_cert)
    }

    pub fn serialize_ca_to_files(&self) -> Result<()> {
        let pubkey_serialized = self.cert.get_key_pair().public_key_pem();
        let privkey_serialized = self.cert.get_key_pair().serialize_pem();
        let ca_cert_serialized = self.cert.serialize_pem()?;

        let ca_dir = Self::get_ca_path(&self.path_prefix);
        create_dir_all(ca_dir)?;

        let mut opts = File::options();
        opts.write(true).create_new(true).mode(0o600);

        opts.mode(0o644)
            .open(Self::get_pubkey_path(&self.path_prefix))
            .context("create pubkey file")?
            .write_all(pubkey_serialized.as_bytes())?;
        opts.open(Self::get_privkey_path(&self.path_prefix))
            .context("create privkey file")?
            .write_all(privkey_serialized.as_bytes())?;
        opts.mode(0o644)
            .open(Self::get_cert_path(&self.path_prefix))
            .context("create ca cert file")?
            .write_all(ca_cert_serialized.as_bytes())?;

        Ok(())
    }
}

/// Generate a new keypair and a CA root certificate using the private key.
fn initialize_ca(_args: &Cli) -> Result<()> {
    let ca = Ca::new(CERT_PATH)?;
    ca.serialize_ca_to_files()?;

    Ok(())
}

/// Enroll the given agent for the given resource by creating a client certificate.
fn enroll_agent(_args: &Cli, agent_name: &str, resource_name: &str) -> Result<()> {
    let ca = Ca::load(CERT_PATH)?;
    ca.client_cert(agent_name, resource_name)?;

    Ok(())
}

/// Enroll a server by creating a certificate.
fn enroll_server(_args: &Cli, server_names: &[String]) -> Result<()> {
    let ca = Ca::load(CERT_PATH)?;
    ca.server_cert(server_names)?;

    Ok(())
}

fn main() -> Result<()> {
    init_logging();

    let args = Cli::parse();

    match &args.command {
        Commands::InitializeCa {} => initialize_ca(&args),
        Commands::EnrollServer { server_names } => enroll_server(&args, server_names),
        Commands::EnrollAgent {
            resource_name,
            agent_name,
        } => enroll_agent(&args, agent_name, resource_name),
    }
}
