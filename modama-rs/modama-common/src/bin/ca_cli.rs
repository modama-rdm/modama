use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use modama_common::{ca::Ca, logging::init_logging};
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
}

#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, default_value = "/certs/")]
    ca_path: PathBuf,
}

fn ca_with_password(args: &Cli) -> Result<Ca> {
    let password = prompt_password("Password: ")?;
Ok(Ca::load(&args.ca_path, &password)?)
}

/// Generate a new keypair and a CA root certificate using the private key.
fn initialize_ca(args: &Cli) -> Result<()> {
    println!("Creating new CA. Please enter a safe password to encrypt the private key.");
    let password = prompt_password("Password: ")?;
    let password_again = prompt_password("Password (again): ")?;
    if password != password_again {
        bail!("Passwords don't match.");
    }
    let ca = Ca::new(Some(&args.ca_path), &password)?;
    ca.serialize_ca_to_files()?;

    Ok(())
}

/// Enroll the given agent for the given resource by creating a client certificate.
fn enroll_agent(args: &Cli, agent_name: &str, resource_name: &str) -> Result<()> {
    let ca = ca_with_password(args)?;
    let key_and_cert = ca.client_cert(agent_name, resource_name)?;
    ca.save_client_cert(agent_name, resource_name, &key_and_cert)?;

    Ok(())
}

/// Enroll a server by creating a certificate.
fn enroll_server(args: &Cli, server_names: &[String]) -> Result<()> {
    let ca = ca_with_password(args)?;
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
