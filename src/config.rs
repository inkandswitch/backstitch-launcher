use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};
use serde::Deserialize;
use thiserror::Error;
use toml::Table;
use url::Url;

#[derive(ValueEnum, Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UseDotnet {
    True,
    False,
    Auto,
}

#[derive(Parser, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[command(rename_all = "kebab-case")]
pub struct CommandConfig {
    #[clap(help = "Whether we should download the .NET version of Godot. Defaults to auto.")]
    #[arg(long)]
    pub dotnet: Option<UseDotnet>,

    #[clap(help = "Use a custom download URL for Godot. \
        Downloads must contain the same .zip structure as the Godot website downloads for .NET or regular. \
        Cross-platform custom URLs are not currently supported!")]
    #[arg(long)]
    pub godot_url: Option<Url>,

    #[clap(help = "Use a custom executable path for Godot, instead of downloading.")]
    #[arg(conflicts_with = "godot_url")]
    #[arg(conflicts_with = "dotnet")]
    #[arg(long)]
    pub godot_path: Option<PathBuf>,

    #[clap(
        help = "Whether to download the latest prerelease version of Backstitch, instead of the stable version. Warning: Here be dragons!"
    )]
    #[arg(long)]
    pub allow_prerelease: Option<bool>,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("error reading backstitch.cfg {0}")]
    FileReadError(String),
    #[error("error parsing backstitch.cfg {0}")]
    FileParseError(String),
}

async fn get_file_config(file: &Path) -> Result<Option<CommandConfig>, ConfigError> {
    println!("Trying to get Backstitch config from {file:?}");
    let file = match tokio::fs::read(file).await {
        Ok(bytes) => bytes,
        Err(e) => match e.kind() {
            // This is OK! It just means there's no config file.
            std::io::ErrorKind::NotFound => return Ok(None),
            _ => return Err(ConfigError::FileReadError(e.to_string())),
        },
    };

    let table: Table = match toml::from_slice(&file) {
        Ok(t) => t,
        Err(e) => return Err(ConfigError::FileParseError(e.to_string())),
    };

    let value = match table.get("backstitch_launcher") {
        Some(val) => val.clone(),
        None => return Ok(None),
    };

    match value.try_into() {
        Ok(content) => Ok(content),
        Err(e) => Err(ConfigError::FileParseError(e.to_string())),
    }
}

pub async fn setup_config() -> Result<CommandConfig, ConfigError> {
    let mut command_config = CommandConfig::parse();
    let file_config = get_file_config(&PathBuf::from("backstitch.cfg")).await?;

    // Command config always takes precedence. We fill in missing properties from the file, one by one.
    if let Some(file_config) = file_config {
        if let Some(allow_prerelease) = file_config.allow_prerelease
            && command_config.allow_prerelease.is_none()
        {
            command_config.allow_prerelease = Some(allow_prerelease);
        }

        if let Some(godot_url) = file_config.godot_url
            && command_config.godot_url.is_none()
        {
            command_config.godot_url = Some(godot_url);
        }

        if let Some(dotnet) = file_config.dotnet
            && command_config.dotnet.is_none()
        {
            command_config.dotnet = Some(dotnet);
        }

        if let Some(godot_path) = file_config.godot_path
            && command_config.godot_path.is_none()
        {
            command_config.godot_path = Some(godot_path);
        }
    }

    Ok(command_config)
}
