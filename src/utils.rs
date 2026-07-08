use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::{Client, header};
use sha2::{Digest, Sha256};
#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::PermissionsExt;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::ExitStatus,
};
use tokio::{
    fs::{self},
    io::AsyncWriteExt,
};
use url::Url;

#[derive(thiserror::Error, Debug)]
pub enum LauncherError {
    #[error(
        "the Backstitch Launcher is out of date, and cannot acquire the latest release. \
        Update at https://backstitch.dev/docs/installation/launcher"
    )]
    OutOfDate,
    #[error("unsupported version of Backstitch: {0}")]
    TooOld(String),
    #[error("the release had no attached metadata, or the metadata was invalid: {0}")]
    BadMetadata(String),
    #[error("the release didn't include an expected asset {0}")]
    ReleaseAssetNotFound(String),
    #[error("the version file was not found (this is OK)")]
    NotInstalled,
    #[error("malformed version file {0}")]
    MalformedVersion(String),
    #[error("the version file was malformed")]
    BadVersionFile(String),
    #[error("the file downloaded from {0} did not match the expected hash")]
    DigestMismatch(Url),
    #[error("the file at {0} couldn't be downloaded, because it failed too many times")]
    TooManyRetries(Url),
    #[error("there was a problem launching Godot: {0}")]
    Launch(String),
    #[error("Godot exited with error code: {0}")]
    Exit(ExitStatus),
    #[error("network {0}")]
    Network(#[from] reqwest::Error),
    #[error("unzip {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io {0}")]
    Io(#[from] std::io::Error),
}

pub fn fail() {
    println!("Press Enter to continue...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}

async fn ensure_directory(path: &Path) -> Result<(), LauncherError> {
    if !path.exists() {
        fs::create_dir_all(path).await?;
    }
    Ok(())
}

async fn ensure_empty_directory(path: &Path) -> Result<(), LauncherError> {
    if path.exists() {
        fs::remove_dir_all(path).await?;
    }
    fs::create_dir_all(path).await?;
    Ok(())
}

fn unzip_file(zip_path: &Path, dest: &Path, skip_root_dir: bool) -> Result<(), LauncherError> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let Some(name) = file.enclosed_name() else {
            continue;
        };

        let out_path = if skip_root_dir {
            let oneup = name.components().skip(1).collect::<PathBuf>();
            if oneup.as_os_str().is_empty() {
                continue;
            }
            dest.join(oneup)
        } else {
            dest.join(name)
        };

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&out_path)?;
            // get permissions from the source file
            #[cfg(not(target_os = "windows"))]
            {
                let permissions = file.unix_mode().unwrap_or_default();
                let _ = outfile.set_permissions(std::fs::Permissions::from_mode(permissions));
            }
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

async fn get(client: &Client, url: &Url, seek: usize) -> Result<reqwest::Response, LauncherError> {
    let mut request = client.get(url.to_string());
    if seek != 0 {
        request = request.header(header::RANGE, format!("bytes={}-", seek));
    }

    tracing::info!("GET {url}");
    Ok(request.send().await?.error_for_status()?)
}

pub async fn download_and_extract_file(
    client: &Client,
    url: &Url,
    expected_digest: Option<&[u8; 32]>,
    output_dir: &Path,
    skip_root_dir: bool,
) -> Result<(), LauncherError> {
    let temp_dir = std::env::temp_dir().join("backstitch_update");
    ensure_empty_directory(&temp_dir).await?;

    let mut response = get(client, url, 0).await?;

    let total_size = response.content_length().unwrap_or(0);
    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] \
            {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );

    let mut bytes = Vec::with_capacity(total_size as usize);
    let mut hasher = expected_digest.map(|_| Sha256::new());
    let mut tries = 0;

    tracing::info!("Expecting to download {total_size} bytes");

    loop {
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(e) => {
                    // If it's a decoding error, keep trying...
                    if e.is_decode() {
                        tracing::warn!("Interrupt {e:?}");
                        break;
                    }
                    return Err(e.into());
                }
            };
            bytes.extend_from_slice(&chunk);
            if let Some(hasher) = hasher.as_mut() {
                hasher.update(&chunk);
            }
            pb.inc(chunk.len() as u64);
        }

        // If we're complete, we're done.
        if bytes.len() as u64 >= total_size {
            break;
        }

        tries += 1;

        if tries > 10 {
            return Err(LauncherError::TooManyRetries(url.clone()));
        }
        response = get(client, url, bytes.len()).await?;
    }

    let digest = hasher.map(|h| h.finalize());
    pb.finish_with_message("Download complete");

    tracing::info!("Downloaded {} bytes", bytes.len());

    let temp_filepath = temp_dir.join("download.zip");
    tracing::info!("Writing zip to filepath {temp_filepath:?}");
    let mut temp_file = fs::File::create(&temp_filepath).await?;
    temp_file.write_all(&bytes).await?;

    if bytes.len() as u64 != total_size {
        return Err(LauncherError::DigestMismatch(url.clone()));
    }

    if let (Some(digest), Some(expected)) = (digest, expected_digest)
        && digest.as_slice() != expected
    {
        return Err(LauncherError::DigestMismatch(url.clone()));
    }

    println!("Extracting to {output_dir:?}...");
    ensure_directory(output_dir).await?;
    unzip_file(&temp_filepath, output_dir, skip_root_dir)?;

    Ok(())
}

pub fn prompt_yes_no(prompt: &str) -> bool {
    loop {
        print!("{} (y/n): ", prompt);
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => continue,
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub async fn make_folder_contents_executable(path: &Path) -> Result<(), LauncherError> {
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
