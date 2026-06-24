use std::path::PathBuf;

use reqwest::Client;
use tokio::fs;

use crate::{
    config::{CommandConfig, UseDotnet},
    godot_urls::get_godot_info,
    utils::{self, GetError},
};

const GODOT_OUTPUT_DIR: &str = "./godot_editor";
const CS_EXTS: [&str; 2] = ["csproj", "sln"];

pub async fn try_update(
    client: &Client,
    config: &CommandConfig,
    old_version: Option<&str>,
    new_version: &str,
) -> Result<PathBuf, GetError> {
    if let Some(path) = &config.godot_path {
        println!("Trying to use existing copy of Godot from {path:?}...");
        return Ok(path.clone());
    }

    let dotnet = match config.dotnet.as_ref().unwrap_or(&UseDotnet::Auto) {
        UseDotnet::True => true,
        UseDotnet::False => false,
        UseDotnet::Auto => is_dotnet().await?,
    };

    let mut godot_info =
        get_godot_info(new_version, dotnet).map_err(|e| GetError::Unknown(e.to_string()))?;

    if let Some(url) = config.godot_url.clone() {
        println!("Using Godot URL override: {url}");
        godot_info.url = url;
    }

    let exe_path = PathBuf::from(GODOT_OUTPUT_DIR).join(godot_info.exe_path);
    if old_version.is_some_and(|v| v == new_version) {
        let exists = tokio::fs::try_exists(&exe_path).await?;
        if exists {
            println!(
                "Godot already exists at the expected path, and we haven't requested an update!"
            );
            println!(
                "If you want to force Godot to re-download, delete the res://{GODOT_OUTPUT_DIR} folder."
            );
            return Ok(exe_path);
        }
    }

    println!("Re-acquiring Godot...");
    utils::download_and_extract_file(client, &godot_info.url, &PathBuf::from(GODOT_OUTPUT_DIR))
        .await?;

    // We do no validation to ensure that Godot is actually downloaded to the expected path.
    Ok(exe_path)
}

pub async fn is_dotnet() -> Result<bool, GetError> {
    // Iterate root files
    let mut entries = fs::read_dir(".").await?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let t = entry.file_type().await?;
        if !t.is_file() {
            continue;
        }
        let filename = PathBuf::from(entry.file_name());
        let Some(ext) = filename.extension() else {
            continue;
        };
        if CS_EXTS.contains(&ext.to_str().unwrap_or_default()) {
            return Ok(true);
        }
    }

    Ok(false)
}
