use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use smbdrop::helpers::{is_immutable, set_immutable};

#[derive(Debug, Parser)]
struct Cli {
    filename: PathBuf,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let imm = is_immutable(&args.filename)?;
    println!("is immutable: {}", imm);
    set_immutable(&args.filename, !imm)?;
    let imm = is_immutable(&args.filename)?;
    println!("is immutable: {}", imm);
    Ok(())
}
