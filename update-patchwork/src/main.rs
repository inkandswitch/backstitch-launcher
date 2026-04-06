use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::copy;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;

const GITHUB_API: &str =
    "https://api.github.com/repos/inkandswitch/patchwork-godot-plugin/releases/latest";

const VERSION_FILE: &str = ".patchwork_version";
const GODOT_OUTPUT_DIR: &str = "./godot_editor";
const PLUGIN_OUTPUT_DIR: &str = "./addons/patchwork";

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

async fn ensure_empty_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        fs::remove_dir_all(path).await?;
    }
    fs::create_dir_all(path).await?;
    Ok(())
}

async fn download_file(
    client: &Client,
    url: &str,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let response = client.get(url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;

    let mut file = fs::File::create(output_path).await?;

    file.write_all(&bytes).await?;

    Ok(())
}

fn unzip_file(zip_path: &Path, dest: &Path) -> Result<(), Box<dyn Error>> {
    let file = File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let out_path = dest.join(file.name());

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&out_path)?;
            copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

async fn update_patchwork() -> Result<(), Box<dyn Error>> {
    let temp_dir = env::temp_dir().join("patchwork_update");
    let godot_zip_path = temp_dir.join("godot.zip");
    let plugin_zip_path = temp_dir.join("plugin.zip");

    let current_version = if Path::new(VERSION_FILE).exists() {
        fs::read_to_string(VERSION_FILE).await?.trim().to_string()
    } else {
        String::new()
    };

    println!("Current Patchwork version: {}", current_version);
    println!("Querying GitHub for latest release...");

    let client = Client::builder().user_agent("update-patchwork").build()?;

    let release: Release = client
        .get(GITHUB_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let latest_version = release.tag_name;
    println!("Latest Patchwork version: {}", latest_version);

    if current_version == latest_version {
        println!("Patchwork is already up to date. Exiting.");
        return Ok(());
    }

    // TODO: Make this cross-platform
    let godot_asset = release
        .assets
        .iter()
        .find(|a| a.name.contains("godot-with-patchwork-windows"))
        .ok_or("Godot asset not found")?;

    let plugin_asset = release
        .assets
        .iter()
        .find(|a| a.name.contains("patchwork-godot-plugin"))
        .ok_or("Plugin asset not found")?;

    ensure_empty_directory(&temp_dir).await?;

    println!("Downloading Godot editor...");
    download_file(&client, &godot_asset.browser_download_url, &godot_zip_path).await?;

    println!("Downloading Patchwork plugin...");
    download_file(
        &client,
        &plugin_asset.browser_download_url,
        &plugin_zip_path,
    )
    .await?;

    println!("Extracting Godot editor...");
    ensure_empty_directory(Path::new(GODOT_OUTPUT_DIR)).await?;
    unzip_file(&godot_zip_path, Path::new(GODOT_OUTPUT_DIR))?;

    println!("Extracting Patchwork plugin...");
    ensure_empty_directory(Path::new(PLUGIN_OUTPUT_DIR)).await?;
    unzip_file(&plugin_zip_path, Path::new(PLUGIN_OUTPUT_DIR))?;

    println!("Writing version file...");
    let mut version_file = fs::File::create(VERSION_FILE).await?;
    version_file.write_all(latest_version.as_bytes()).await?;

    fs::remove_dir_all(&temp_dir).await?;

    println!("Patchwork update complete.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    update_patchwork().await?;
    println!("Press Enter to continue...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    Ok(())
}
