use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::{env, io};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::config::CommandConfig;
use crate::utils::{self, GetError};

const GITHUB_API: &str = "https://api.github.com/repos/inkandswitch/backstitch/releases";

const VERSION_FILE: &str = ".backstitch_version";
const PLUGIN_ARTIFACT_PREFIX: &str = "backstitch";
const PLUGIN_OUTPUT_DIR: &str = "./addons/backstitch";

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
    published_at: DateTime<Utc>,
    body: String,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: Url,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
struct ReleaseMetadata {
    recommended_godot: String,
    minimum_launcher: String,
}

pub async fn get_current_version() -> Result<VersionFile, GetError> {
    let version_file = match fs::read(VERSION_FILE).await {
        Ok(bytes) => bytes,
        Err(e) => {
            return Err(match e.kind() {
                io::ErrorKind::NotFound => {
                    println!("Backstitch is not currently installed.");
                    GetError::NotInstalled
                }
                _ => GetError::Unknown(e.to_string()),
            });
        }
    };

    serde_json::from_slice(&version_file).map_err(|e| GetError::BadVersionFile(e.to_string()))
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
) -> Result<(), GetError> {
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(prefix.as_str()))
        .ok_or(GetError::Unknown(format!(
            "Asset containing {prefix} not found"
        )))?;

    utils::download_and_extract_file(client, &asset.browser_download_url, output_dir).await
}

async fn ensure_release(client: &Client, release: &Release) -> Result<(), GetError> {
    // we could check the plugin version file instead of just the directory's existence... but this is fine
    let backstitch_exists = tokio::fs::try_exists(Path::new(PLUGIN_OUTPUT_DIR)).await?;
    if !backstitch_exists {
        println!("Re-acquiring Backstitch...");
        acquire_from_release(
            client,
            release,
            Path::new(PLUGIN_OUTPUT_DIR),
            &PLUGIN_ARTIFACT_PREFIX.to_string(),
        )
        .await?;
    }
    Ok(())
}

async fn overwrite_release(client: &Client, release: &Release) -> Result<(), GetError> {
    acquire_from_release(
        client,
        release,
        Path::new(PLUGIN_OUTPUT_DIR),
        &PLUGIN_ARTIFACT_PREFIX.to_string(),
    )
    .await?;
    Ok(())
}

fn is_dev_version(version: &str) -> bool {
    // if the version is something like 209/fixes (i.e. a PR branch), return true
    version.contains("/")
}

async fn get_latest_release(client: &Client) -> Result<Release, GetError> {
    Ok(client
        .get(format!("{GITHUB_API}/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn get_latest_release_or_prerelease(client: &Client) -> Result<Release, GetError> {
    let releases: Vec<Release> = client
        .get(GITHUB_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let release = releases
        .into_iter()
        .max_by_key(|release| release.published_at);
    release.ok_or_else(|| GetError::Unknown("No releases found".to_string()))
}

fn extract_release_metadata(release: &Release) -> Result<ReleaseMetadata, GetError> {
    let re =
        Regex::new(r"<!--\s*BEGIN_RELEASE_METADATA\s+(\{.*?\})\s+END_RELEASE_METADATA\s*-->/gmus")
            .unwrap();
    let caps = re
        .captures(&release.body)
        .ok_or_else(|| GetError::BadMetadata("could not apply regex".to_string()))?;
    let cap = caps
        .get(1)
        .ok_or_else(|| GetError::BadMetadata("could not get capture".to_string()))?
        .as_str();
    serde_json::from_str(cap).map_err(|e| GetError::BadMetadata(e.to_string()))
}

fn check_release(metadata: &ReleaseMetadata) -> Result<(), GetError> {
    let version =
        Version::parse(env!("CARGO_PKG_VERSION")).map_err(|e| GetError::Unknown(e.to_string()))?;
    let min_version = Version::parse(metadata.minimum_launcher.trim_start_matches("v"))
        .map_err(|e| GetError::BadMetadata(e.to_string()))?;
    if version < min_version {
        return Err(GetError::OutOfDate);
    }
    Ok(())
}

async fn get_release(client: &Client, version: &str) -> Result<Release, GetError> {
    Ok(client
        .get(format!("{GITHUB_API}/tags/{version}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

fn desired_version(current_version: Option<&String>, latest_version: &str) -> String {
    let Some(current_version) = current_version else {
        println!("Backstitch is not installed. Installing...");
        return latest_version.to_string();
    };

    if current_version == latest_version {
        println!("Backstitch is up-to-date.");
        return current_version.to_string();
    }

    // HACK: if we're not in a terminal, just update anyways, as long as it's not a dev version
    if !std::io::stdin().is_terminal() {
        return if is_dev_version(current_version) {
            println!("Not updating dev version...");
            current_version.to_string()
        } else {
            latest_version.to_string()
        };
    }

    if utils::prompt_yes_no("Backstitch is out of date. Update?") {
        latest_version.to_string()
    } else {
        current_version.to_string()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct VersionFile {
    pub backstitch_version: String,
    pub godot_version: String,
}

pub async fn try_update(
    client: &Client,
    config: &CommandConfig,
    current_version: Option<&VersionFile>,
) -> Result<VersionFile, GetError> {
    let temp_dir = env::temp_dir().join("backstitch_update");

    println!("Querying GitHub for latest release...");

    let latest_release = if config.allow_prerelease.unwrap_or(false) {
        get_latest_release_or_prerelease(client).await?
    } else {
        get_latest_release(client).await?
    };

    let latest_metadata = extract_release_metadata(&latest_release)?;
    check_release(&latest_metadata)?;

    let latest_version = latest_release.tag_name.clone();
    println!("Latest Backstitch version: {}", latest_version);

    let mut desired_version = desired_version(
        current_version.map(|v| &v.backstitch_version),
        &latest_version,
    );

    let desired_release = if desired_version == latest_version {
        latest_release
    } else {
        // If the current release isn't actually a valid version from Github, force the update
        match get_release(client, &desired_version).await {
            // Just make sure we've gotten the current release OK
            Ok(release) => release,
            // Change our mind
            Err(e) => {
                println!("Error getting release {desired_version}: {e}");
                println!("Updating to latest release...");
                desired_version = latest_version;
                latest_release
            }
        }
    };

    let desired_metadata = extract_release_metadata(&desired_release)?;
    check_release(&desired_metadata)?;

    if current_version.is_none_or(|v| v.backstitch_version != desired_version) {
        overwrite_release(client, &desired_release).await?;
    } else {
        ensure_release(client, &desired_release).await?;
    }

    #[cfg(not(target_os = "windows"))]
    make_folder_contents_executable(Path::new(GODOT_OUTPUT_DIR)).await?;

    let new_version = VersionFile {
        godot_version: desired_metadata.recommended_godot,
        backstitch_version: desired_version,
    };

    println!("Writing version file...");
    let mut version_file = fs::File::create(VERSION_FILE).await?;
    version_file
        .write_all(
            serde_json::to_string(&new_version)
                .map_err(|e| GetError::Unknown(e.to_string()))?
                .as_bytes(),
        )
        .await?;

    // this is allowed to fail; maybe we didn't write anything?
    let _ = fs::remove_dir_all(&temp_dir).await;

    println!("Backstitch update complete.");
    Ok(new_version)
}
