use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::Client;
use semver::Version;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};
use std::io::IsTerminal;
use std::path::Path;
use std::{env, io};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::config::CommandConfig;
use crate::utils::{self, LauncherError};

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

fn deserialize_digest<'de, D>(deserializer: D) -> Result<Box<[u8; 32]>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let (algorithm, hex) = s
        .split_once(':')
        .ok_or_else(|| Error::custom("expected digest in the form '<algorithm>:<hex>'"))?;

    if algorithm != "sha256" {
        return Err(Error::custom(format!(
            "expected digest algorithm sha256; was {algorithm}"
        )));
    }

    let v = hex::decode(hex).map_err(Error::custom)?;
    v.into_boxed_slice()
        .try_into()
        .map_err(|_| Error::custom(format!("Digest expected to be 32 bytes. Digest: {s}")))
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: Url,
    #[serde(deserialize_with = "deserialize_digest")]
    digest: Box<[u8; 32]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
struct ReleaseMetadata {
    recommended_godot: String,
    minimum_launcher: String,
}

pub async fn get_current_version() -> Result<VersionFile, LauncherError> {
    let version_file = match fs::read(VERSION_FILE).await {
        Ok(bytes) => bytes,
        Err(e) => {
            return Err(match e.kind() {
                io::ErrorKind::NotFound => {
                    println!("Backstitch is not currently installed.");
                    LauncherError::NotInstalled
                }
                _ => LauncherError::Io(e),
            });
        }
    };

    serde_json::from_slice(&version_file).map_err(|e| LauncherError::BadVersionFile(e.to_string()))
}

async fn acquire_from_release(
    client: &Client,
    release: &Release,
    output_dir: &Path,
    prefix: &str,
) -> Result<(), LauncherError> {
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(prefix))
        .ok_or_else(|| LauncherError::ReleaseAssetNotFound(prefix.to_string()))?;

    utils::download_and_extract_file(
        client,
        &asset.browser_download_url,
        Some(&asset.digest),
        output_dir,
        false,
    )
    .await?;
    // check if `addons/backstitch` exists in the output directory
    let backstitch_dir = output_dir.join("addons/backstitch");
    if backstitch_dir.exists() {
        // move `addons/backstitch` to `<output_dir>/..`
        let temp_dir = output_dir
            .parent()
            .unwrap_or(Path::new(".."))
            .join("backstitch_temp");
        fs::rename(&backstitch_dir, &temp_dir).await?;
        fs::remove_dir_all(&output_dir).await?;
        fs::rename(&temp_dir, output_dir).await?;
    }
    Ok(())
}

async fn ensure_release(client: &Client, release: &Release) -> Result<(), LauncherError> {
    // we could check the plugin version file instead of just the directory's existence... but this is fine
    let backstitch_exists = tokio::fs::try_exists(Path::new(PLUGIN_OUTPUT_DIR)).await?;
    if !backstitch_exists {
        println!("Re-acquiring Backstitch...");
        acquire_from_release(
            client,
            release,
            Path::new(PLUGIN_OUTPUT_DIR),
            PLUGIN_ARTIFACT_PREFIX,
        )
        .await?;
    }
    Ok(())
}

async fn overwrite_release(client: &Client, release: &Release) -> Result<(), LauncherError> {
    let plugin_dir = Path::new(PLUGIN_OUTPUT_DIR);
    if plugin_dir.exists() {
        let _ = fs::remove_dir_all(plugin_dir).await;
    }
    acquire_from_release(client, release, plugin_dir, PLUGIN_ARTIFACT_PREFIX).await?;
    Ok(())
}

fn is_dev_version(version: &str) -> bool {
    // if the version is something like 209/fixes (i.e. a PR branch), return true
    version.contains("/")
}

async fn get_latest_release(client: &Client) -> Result<Release, LauncherError> {
    Ok(client
        .get(format!("{GITHUB_API}/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn get_latest_release_or_prerelease(client: &Client) -> Result<Release, LauncherError> {
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
    release.ok_or_else(|| LauncherError::ReleaseAssetNotFound("No releases found".to_string()))
}

fn extract_release_metadata(release: &Release) -> Result<ReleaseMetadata, LauncherError> {
    let re =
        Regex::new(r"(?ms)<!--\s*BEGIN_RELEASE_METADATA\s+(\{.*?\})\s+END_RELEASE_METADATA\s*-->")
            .unwrap();
    if !release.body.contains("BEGIN_RELEASE_METADATA") {
        if release.tag_name.starts_with("v1") {
            return Err(LauncherError::TooOld(release.tag_name.clone()));
        }
        // All new releases should have this, so throw an error
        return Err(LauncherError::BadMetadata(
            "No release metadata found".to_string(),
        ));
    }
    let caps = re
        .captures(&release.body)
        .ok_or_else(|| LauncherError::BadMetadata("could not apply regex".to_string()))?;
    let cap = caps
        .get(1)
        .ok_or_else(|| LauncherError::BadMetadata("could not get capture".to_string()))?
        .as_str();
    serde_json::from_str(cap).map_err(|e| LauncherError::BadMetadata(e.to_string()))
}

fn check_release(metadata: &ReleaseMetadata) -> Result<(), LauncherError> {
    let version = Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| {
        panic!(
            "CARGO_PKG_VERSION is bad! Value: {}",
            env!("CARGO_PKG_VERSION")
        )
    });
    let min_version = Version::parse(metadata.minimum_launcher.trim_start_matches("v"))
        .map_err(|e| LauncherError::BadMetadata(e.to_string()))?;
    if version < min_version {
        return Err(LauncherError::OutOfDate);
    }
    Ok(())
}

async fn get_release(client: &Client, version: &str) -> Result<Release, LauncherError> {
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
) -> Result<VersionFile, LauncherError> {
    let temp_dir = std::env::temp_dir().join("backstitch_update");

    tracing::info!("Temporary working directory: {temp_dir:?}");

    println!("Querying GitHub for latest release...");

    let latest_release = if config.allow_prerelease.unwrap_or(false) {
        get_latest_release_or_prerelease(client).await?
    } else {
        get_latest_release(client).await?
    };
    tracing::info!("Latest release: {latest_release:?}");

    let latest_metadata = match extract_release_metadata(&latest_release) {
        Ok(metadata) => metadata,
        Err(LauncherError::TooOld(version)) => {
            println!("ERROR: No supported versions available for Backstitch!");
            println!("Please relaunch with the --allow-prerelease=true flag to continue.");
            return Err(LauncherError::TooOld(version));
        }
        Err(e) => {
            return Err(e);
        }
    };
    tracing::info!("Latest metadata: {latest_metadata:?}");
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

    let new_version = VersionFile {
        godot_version: desired_metadata.recommended_godot,
        backstitch_version: desired_version,
    };

    println!("Writing version file...");
    let mut version_file = fs::File::create(VERSION_FILE).await?;
    version_file
        .write_all(
            serde_json::to_string(&new_version)
                .expect("Serde deserialization error; this should never happen")
                .as_bytes(),
        )
        .await?;

    // this is allowed to fail; maybe we didn't write anything?
    tracing::info!("Clearing temp directory...");
    let _ = fs::remove_dir_all(&temp_dir).await;

    println!("Backstitch update complete.");
    Ok(new_version)
}
