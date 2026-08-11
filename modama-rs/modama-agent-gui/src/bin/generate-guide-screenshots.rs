use std::{error::Error, path::PathBuf};

use anyhow::Context;
use clap::Parser;
use modama_agent_gui::guide_screenshots::{self, GuideScene};

#[derive(Debug, Parser)]
struct Cli {
    /// Directory in which to write the canonical guide PNGs.
    #[arg(long)]
    output_dir: Option<PathBuf>,
}

fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .find(|path| path.join(".git").exists())
        .map(PathBuf::from)
        .ok_or_else(|| "could not find the repository root".into())
}

fn output_dir(cli: &Cli) -> Result<PathBuf, Box<dyn Error>> {
    match &cli.output_dir {
        Some(path) => Ok(path.clone()),
        None => Ok(repository_root()?.join("docs/guide/src/user/images/agent")),
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let output_dir = output_dir(&cli)?;
    std::fs::create_dir_all(&output_dir)?;

    for scene in GuideScene::ALL {
        let path = output_dir.join(scene.filename());
        guide_screenshots::render(scene)
            .map_err(|error| format!("could not render {}: {error}", scene.filename()))?
            .save(&path)?;
        println!("wrote {}", path.display());
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let val = std::env::var("WGPU_DEBUG").context("WGPU_DEBUG should be set to 0")?;
    assert_eq!(val, "0");

    run(Cli::parse())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_is_in_the_mdbook_tree() -> Result<(), Box<dyn Error>> {
        let cli = Cli::try_parse_from(["generate-guide-screenshots"])?;

        assert_eq!(
            output_dir(&cli)?,
            repository_root()?.join("docs/guide/src/user/images/agent")
        );
        Ok(())
    }

    #[test]
    fn output_directory_can_be_overridden() -> Result<(), Box<dyn Error>> {
        let cli = Cli::try_parse_from([
            "generate-guide-screenshots",
            "--output-dir",
            "custom/screenshots",
        ])?;

        assert_eq!(output_dir(&cli)?, PathBuf::from("custom/screenshots"));
        Ok(())
    }
}
