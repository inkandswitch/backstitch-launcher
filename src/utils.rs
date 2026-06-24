use reqwest::Client;
#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::PermissionsExt;
use std::{io::Write, path::Path, process::ExitStatus};
use tokio::{
    fs::{self},
    io::AsyncWriteExt,
};
use url::Url;

#[derive(thiserror::Error, Debug)]
pub enum LauncherError {
    #[error("unknown {0}")]
    Unknown(String),
    #[error(
        "the Backstitch Launcher is out of date, and cannot acquire the latest release. \
        Update at https://backstitch.dev/docs/installation/launcher"
    )]
    OutOfDate,
    #[error("the release had no attached metadata, or the metadata was invalid {0}")]
    BadMetadata(String),
    #[error("the version file was not found (this is OK)")]
    NotInstalled,
    #[error("the version file was malformed")]
    BadVersionFile(String),
    #[error("there was a problem launching Godot {0}")]
    Launch(String),
    #[error("Godot exited with error code {0}")]
    Exit(ExitStatus),
}

impl From<reqwest::Error> for LauncherError {
    fn from(value: reqwest::Error) -> Self {
        LauncherError::Unknown(value.to_string())
    }
}

impl From<std::io::Error> for LauncherError {
    fn from(value: std::io::Error) -> Self {
        LauncherError::Unknown(value.to_string())
    }
}

impl From<zip::result::ZipError> for LauncherError {
    fn from(value: zip::result::ZipError) -> Self {
        LauncherError::Unknown(value.to_string())
    }
}

pub fn fail() {
    println!("Press Enter to continue...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}

async fn ensure_empty_directory(path: &Path) -> Result<(), LauncherError> {
    if path.exists() {
        fs::remove_dir_all(path).await?;
    }
    fs::create_dir_all(path).await?;
    Ok(())
}

fn unzip_file(zip_path: &Path, dest: &Path) -> Result<(), LauncherError> {
    let file = std::fs::File::open(zip_path)?;
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

pub async fn download_and_extract_file(
    client: &Client,
    url: &Url,
    output_dir: &Path,
) -> Result<(), LauncherError> {
    let temp_dir = std::env::temp_dir().join("backstitch_update");
    println!("Temp dir: {temp_dir:?}");
    ensure_empty_directory(&temp_dir).await?;

    let response = client
        .get(url.to_string())
        .send()
        .await?
        .error_for_status()?;
    let bytes = response.bytes().await?;

    println!("Downloading asset...");
    let temp_filepath = temp_dir.join("download.zip");
    let mut temp_file = fs::File::create(&temp_filepath).await?;
    temp_file.write_all(&bytes).await?;

    println!("Extracting to {output_dir:?}...");
    ensure_empty_directory(output_dir).await?;
    unzip_file(&temp_filepath, output_dir)?;

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
pub async fn make_folder_contents_executable(path: &Path) -> Result<(), GetError> {
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
