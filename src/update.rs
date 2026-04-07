use reqwest::Client;
use serde::Deserialize;
#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::PermissionsExt;
use std::{env, io};
use std::error::Error;
use std::fs::File;
use std::io::{Write, copy};
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;

const GITHUB_API: &str =
    "https://api.github.com/repos/inkandswitch/patchwork-godot-plugin/releases/latest";

const VERSION_FILE: &str = ".backstitch_version";
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

fn prompt_yes_no(prompt: &str) -> bool {
    loop {
        print!("{} (y/n): ", prompt);
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => continue,
        }
    }
}

pub async fn get_current_version() -> Option<String> {
    if Path::new(VERSION_FILE).exists() {
        let current = fs::read_to_string(VERSION_FILE).await.ok()?.trim().to_string();
        println!("Current Backstitch version: {}", current);
        return Some(current);
    } else {
        println!("Backstitch is not currently installed.");
        return None;
    };
}

fn godot_artifact_prefix() -> String {
    if cfg!(target_os = "windows") {
        return "godot-with-patchwork-windows".to_string()
    } else if cfg!(target_os = "linux") {
        return "godot-with-patchwork-linux".to_string()
    } else if cfg!(target_os = "macos") {
        return "godot-with-patchwork-macos".to_string()
    } else {
        panic!("Unsupported OS");
    };

}

#[cfg(not(target_os = "windows"))]
async fn make_folder_contents_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut entries = fs::read_dir(path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if path.is_file() {
            let metadata = fs::metadata(&path).await?;
            let mut perms = metadata.permissions();

            let mode = perms.mode();
            perms.set_mode(mode | 0o111); // add execute bits
            fs::set_permissions(&path, perms).await?;
        }
    }

    Ok(())
}

pub async fn try_update(current_version: Option<String>) -> Result<(), Box<dyn Error>> {
    let temp_dir = env::temp_dir().join("backstitch_update");
    let godot_zip_path = temp_dir.join("godot.zip");
    let plugin_zip_path = temp_dir.join("plugin.zip");

    println!("Querying GitHub for latest release...");

    let client = Client::builder().user_agent("backstitch-launcher").build()?;

    let release: Release = client
        .get(GITHUB_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let latest_version = release.tag_name;
    println!("Latest Backstitch version: {}", latest_version);

    if current_version.as_ref().is_some_and(|v| v == &latest_version) {
        println!("Backstitch is already up to date!");
        return Ok(());
    }

    // If the current version is empty, force an update. Otherwise, prompt.
    if current_version.is_some() {
        if !prompt_yes_no("Backstitch is out of date. Update?")  {
            return Ok(());
        }
    }

    let godot_asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(godot_artifact_prefix().as_str()))
        .ok_or("Godot asset not found")?;

    let plugin_asset = release
        .assets
        .iter()
        .find(|a| a.name.contains("patchwork-godot-plugin"))
        .ok_or("Plugin asset not found")?;

    ensure_empty_directory(&temp_dir).await?;

    println!("Downloading Godot editor...");
    download_file(&client, &godot_asset.browser_download_url, &godot_zip_path).await?;

    println!("Downloading Backstitch plugin...");
    download_file(
        &client,
        &plugin_asset.browser_download_url,
        &plugin_zip_path,
    )
    .await?;

    println!("Extracting Godot editor...");
    let godot_folder = Path::new(GODOT_OUTPUT_DIR);
    ensure_empty_directory(godot_folder).await?;
    unzip_file(&godot_zip_path, Path::new(GODOT_OUTPUT_DIR))?;

    #[cfg(not(target_os = "windows"))]
    make_folder_contents_executable(godot_folder).await?;

    println!("Extracting Backstitch plugin...");
    ensure_empty_directory(Path::new(PLUGIN_OUTPUT_DIR)).await?;
    unzip_file(&plugin_zip_path, Path::new(PLUGIN_OUTPUT_DIR))?;

    println!("Writing version file...");
    let mut version_file = fs::File::create(VERSION_FILE).await?;
    version_file.write_all(latest_version.as_bytes()).await?;

    fs::remove_dir_all(&temp_dir).await?;

    println!("Backstitch update complete.");
    Ok(())
}
