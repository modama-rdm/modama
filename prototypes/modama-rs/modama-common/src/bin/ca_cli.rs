use anyhow::Result;
use clap::{Parser, Subcommand};
use modama_common::{ca::Ca, logging::init_logging};

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

/// Generate a new keypair and a CA root certificate using the private key.
fn initialize_ca(_args: &Cli) -> Result<()> {
    let ca = Ca::new(Some(CERT_PATH))?;
    ca.serialize_ca_to_files()?;

    Ok(())
}

/// Enroll the given agent for the given resource by creating a client certificate.
fn enroll_agent(_args: &Cli, agent_name: &str, resource_name: &str) -> Result<()> {
    let ca = Ca::load(CERT_PATH)?;
    let key_and_cert = ca.client_cert(agent_name, resource_name)?;
    ca.save_client_cert(agent_name, resource_name, &key_and_cert)?;

    Ok(())
}

/// Enroll a server by creating a certificate.
fn enroll_server(_args: &Cli, server_names: &[String]) -> Result<()> {
    let ca = Ca::load(CERT_PATH)?;
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
    }
}
