use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use modama_common::{
    ca::Ca,
    config::{
        AgentConfigFile, AgentConfigFileCa, AgentConfigFileClientCert, AgentConfigFileServices,
        default_event_url, default_listen_local_api, default_mgmt_api_url, default_smb_domain,
        default_smb_host, default_smb_port, default_token_service_url,
    },
    logging::init_logging,
};
use rpassword::prompt_password;

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
    GenerateAgentConfig {
        #[arg(long)]
        listen_local_api: Option<String>,

        #[arg(long)]
        event_url: Option<String>,

        #[arg(long)]
        token_service_url: Option<String>,

        #[arg(long)]
        mgmt_api_url: Option<String>,

        #[arg(long)]
        smb_hostname: Option<String>,

        #[arg(long)]
        smb_port: Option<u32>,

        #[arg(long)]
        smb_domain: Option<String>,

        #[arg(long)]
        agent_name: Option<String>,

        #[arg(long)]
        resource_name: Option<String>,
    },
}

#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, default_value = "/certs/")]
    ca_path: PathBuf,

    #[arg(long)]
    insecure_no_password: bool,
}

fn ca_with_optional_password(args: &Cli) -> Result<Ca> {
    let password = if args.insecure_no_password {
        String::new()
    } else {
        prompt_password("Password: ")?
    };
    Ok(Ca::load(&args.ca_path, &password)?)
}

fn read_pem_raw(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?)
}

fn client_chain_path(ca_path: &Path, resource_name: &str, agent_name: &str) -> PathBuf {
    Ca::get_client_certs_path(ca_path)
        .join(resource_name)
        .join(format!("{agent_name}.chain.pem"))
}

fn client_privkey_path(ca_path: &Path, resource_name: &str, agent_name: &str) -> PathBuf {
    Ca::get_client_certs_path(ca_path)
        .join(resource_name)
        .join(format!("{agent_name}.priv.pem"))
}

fn validate_name(name: &str, kind: &str) -> Result<()> {
    if name.contains('/') || name.contains('\\') || name.contains('@') || name.contains('#') {
        bail!("invalid {kind}: {name}");
    }
    Ok(())
}

fn generate_agent_config(args: &Cli) -> Result<()> {
    let ca_cert = read_pem_raw(&Ca::get_cert_path(&args.ca_path))?;

    let (listen_local_api, services, client_cert) = if let Commands::GenerateAgentConfig {
        listen_local_api,
        event_url,
        token_service_url,
        mgmt_api_url,
        smb_hostname,
        smb_port,
        smb_domain,
        agent_name,
        resource_name,
    } = &args.command
    {
        let client_cert = match (agent_name, resource_name) {
            (Some(agent_name), Some(resource_name)) => {
                validate_name(agent_name, "agent name")?;
                validate_name(resource_name, "resource name")?;
                let chain =
                    read_pem_raw(&client_chain_path(&args.ca_path, resource_name, agent_name))?;
                let private_key = read_pem_raw(&client_privkey_path(
                    &args.ca_path,
                    resource_name,
                    agent_name,
                ))?;
                Some(AgentConfigFileClientCert { chain, private_key })
            }
            (None, None) => None,
            _ => bail!("both --agent-name and --resource-name must be provided together"),
        };

        (
            listen_local_api.clone(),
            AgentConfigFileServices {
                event_url: event_url.clone().unwrap_or_else(default_event_url),
                token_service_url: token_service_url
                    .clone()
                    .unwrap_or_else(default_token_service_url),
                mgmt_api_url: mgmt_api_url.clone().unwrap_or_else(default_mgmt_api_url),
                smb_hostname: smb_hostname.clone().unwrap_or_else(default_smb_host),
                smb_port: smb_port.unwrap_or_else(default_smb_port),
                smb_domain: smb_domain.clone().unwrap_or_else(default_smb_domain),
            },
            client_cert,
        )
    } else {
        bail!("generate-agent-config must be invoked with the correct command")
    };

    let config = AgentConfigFile {
        services,
        listen_local_api: listen_local_api.unwrap_or_else(default_listen_local_api),
        ca: AgentConfigFileCa { ca_cert },
        client_cert,
        linux: None,
    };

    let output = toml::to_string(&config)?;
    println!("{output}");
    Ok(())
}

/// Generate a new keypair and a CA root certificate using the private key.
fn initialize_ca(args: &Cli) -> Result<()> {
    let password = if args.insecure_no_password {
        String::new()
    } else {
        println!("Creating new CA. Please enter a safe password to encrypt the private key.");
        let password = prompt_password("Password: ")?;
        let password_again = prompt_password("Password (again): ")?;
        if password != password_again {
            bail!("Passwords don't match.");
        }
        password
    };
    let ca = Ca::new(Some(&args.ca_path), &password)?;
    ca.serialize_ca_to_files()?;

    Ok(())
}

/// Enroll the given agent for the given resource by creating a client certificate.
fn enroll_agent(args: &Cli, agent_name: &str, resource_name: &str) -> Result<()> {
    let ca = ca_with_optional_password(args)?;
    let key_and_cert = ca.client_cert(agent_name, resource_name)?;
    ca.save_client_cert(agent_name, resource_name, &key_and_cert)?;

    Ok(())
}

/// Enroll a server by creating a certificate.
fn enroll_server(args: &Cli, server_names: &[String]) -> Result<()> {
    let ca = ca_with_optional_password(args)?;
    let key_and_cert = ca.server_cert(server_names)?;
    ca.save_server_cert(server_names, &key_and_cert)?;

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
        Commands::GenerateAgentConfig { .. } => generate_agent_config(&args),
    }
}
