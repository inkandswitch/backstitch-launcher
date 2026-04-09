use reqwest::Client;
use serde::Deserialize;
use std::error::Error;
use std::fs::File;
use std::io::{Write, copy};
#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::{env, io};
use tokio::fs;
use tokio::io::AsyncWriteExt;

const GITHUB_API: &str = "https://api.github.com/repos/inkandswitch/backstitch/releases";

const VERSION_FILE: &str = ".backstitch_version";
const GODOT_OUTPUT_DIR: &str = "./godot_editor";
const PLUGIN_ARTIFACT_PREFIX: &str = "backstitch";
const PLUGIN_OUTPUT_DIR: &str = "./addons/backstitch";

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
        let current = fs::read_to_string(VERSION_FILE)
            .await
            .ok()?
            .trim()
            .to_string();
        println!("Current Backstitch version: {}", current);
        return Some(current);
    } else {
        println!("Backstitch is not currently installed.");
        return None;
    };
}

pub fn get_godot_path() -> PathBuf {
    let exe_name = if cfg!(target_os = "windows") {
        "godot.windows.editor.x86_64.exe"
    } else if cfg!(target_os = "linux") {
        "godot.linuxbsd.editor.x86_64"
    } else if cfg!(target_os = "macos") {
        // Godot macOS builds are inside an .app bundle
        "godot_macos_editor.app/Contents/MacOS/Godot"
    } else {
        panic!("Unsupported OS");
    };

    return std::env::current_dir().unwrap().join("godot_editor").join(exe_name);

}

fn godot_artifact_prefix() -> String {
    if cfg!(target_os = "windows") {
        return "godot-with-backstitch-windows".to_string();
    } else if cfg!(target_os = "linux") {
        return "godot-with-backstitch-linux".to_string();
    } else if cfg!(target_os = "macos") {
        return "godot-with-backstitch-macos".to_string();
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

async fn acquire_from_release(
    client: &Client,
    release: &Release,
    output_dir: &Path,
    prefix: &String,
) -> Result<(), Box<dyn Error>> {
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(prefix.as_str()))
        .ok_or(format!("Asset containing {prefix} not found"))?;

    let temp_dir = env::temp_dir()
        .join("backstitch_update");
    ensure_empty_directory(&temp_dir).await?;
    
    let temp_dir = temp_dir.join(asset.name.clone());

    println!("Downloading {}", asset.name);
    download_file(&client, &asset.browser_download_url, &temp_dir).await?;

    println!("Extracting {}...", asset.name);
    ensure_empty_directory(output_dir).await?;
    unzip_file(&temp_dir, output_dir)?;

    Ok(())
}

async fn ensure_release(client: &Client, release: &Release) -> Result<(), Box<dyn Error>> {
    let godot_exists = tokio::fs::try_exists(get_godot_path()).await?;
    // we could check the plugin version file instead of just the directory's existence... but this is fine
    let backstitch_exists = tokio::fs::try_exists(Path::new(PLUGIN_OUTPUT_DIR)).await?;

    if !godot_exists {
        println!("Re-acquiring Godot...");
        acquire_from_release(
            client,
            &release,
            Path::new(GODOT_OUTPUT_DIR),
            &godot_artifact_prefix(),
        )
        .await?;
    }
    if !backstitch_exists {
        println!("Re-acquiring Backstitch...");
        acquire_from_release(
            client,
            &release,
            Path::new(PLUGIN_OUTPUT_DIR),
            &PLUGIN_ARTIFACT_PREFIX.to_string(),
        )
        .await?;
    }
    Ok(())
}

async fn overwrite_release(client: &Client, release: &Release) -> Result<(), Box<dyn Error>> {
    acquire_from_release(
        client,
        &release,
        Path::new(GODOT_OUTPUT_DIR),
        &godot_artifact_prefix(),
    )
    .await?;
    acquire_from_release(
        client,
        &release,
        Path::new(PLUGIN_OUTPUT_DIR),
        &PLUGIN_ARTIFACT_PREFIX.to_string(),
    )
    .await?;
    Ok(())
}

pub async fn try_update(mut current_version: Option<String>) -> Result<(), Box<dyn Error>> {
    let temp_dir = env::temp_dir().join("backstitch_update");

    println!("Querying GitHub for latest release...");

    let client = Client::builder()
        .user_agent("backstitch-launcher")
        .build()?;

    let latest_release: Release = client
        .get(format!("{GITHUB_API}/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let latest_version = latest_release.tag_name.clone();
    println!("Latest Backstitch version: {}", latest_version);

    let mut updating = false;
    if current_version
        .as_ref()
        .is_some_and(|v| v == &latest_version)
    {
        println!("Backstitch is already up to date!");
    } else {
        // If the current version is empty, force an update. Otherwise, prompt.
        if current_version.is_some() {
            if prompt_yes_no("Backstitch is out of date. Update?") {
                current_version = Some(latest_version);
                updating = true;
            }
        } else {
            current_version = Some(latest_version);
            updating = true;
        }
    }

    let current_version = current_version.unwrap();

    if updating {
        overwrite_release(&client, &latest_release).await?;
    } else {
        let current_release: Release = client
            .get(format!("{GITHUB_API}/tags/{current_version}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        ensure_release(&client, &current_release).await?;
    }

    #[cfg(not(target_os = "windows"))]
    make_folder_contents_executable(Path::new(GODOT_OUTPUT_DIR)).await?;

    println!("Writing version file...");
    let mut version_file = fs::File::create(VERSION_FILE).await?;
    version_file.write_all(current_version.as_bytes()).await?;

    // this is allowed to fail; maybe we didn't write anything? 
    let _ = fs::remove_dir_all(&temp_dir).await;

    println!("Backstitch update complete.");
    Ok(())
}
