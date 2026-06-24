use std::path::PathBuf;

use reqwest::Url;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InfoError {
    #[error("malformed version {0}")]
    MalformedVersion(String),
    #[error("unsupported platform {0}")]
    UnsupportedPlatform(String),
    #[error("{0}")]
    Unknown(String),
}

pub struct GodotDownloadInfo {
    pub url: Url,
    pub exe_path: PathBuf,
}

pub fn get_godot_info(version: &str, dotnet: bool) -> Result<GodotDownloadInfo, InfoError> {
    let [short_version, flavor]: [&str; 2] =
        version
            .split("-")
            .collect::<Vec<&str>>()
            .try_into()
            .map_err(|_| InfoError::MalformedVersion(version.to_string()))?;

    let slug_prefix = if dotnet { "mono_" } else { "" };
    let slug = godot_slug(dotnet)?;
    let platform = godot_platform()?;
    let url = Url::parse(&format!(
        "https://downloads.godotengine.org/?version={short_version}&flavor={flavor}&slug={slug_prefix}{slug}.zip&platform={platform}"
    )).map_err(|e| InfoError::Unknown(e.to_string()))?;
    let path = godot_path(dotnet, version)?;

    Ok(GodotDownloadInfo {
        url,
        exe_path: path,
    })
}

fn godot_slug(dotnet: bool) -> Result<String, InfoError> {
    cfg_select! {
        all(target_os = "windows", target_arch = "x86_64") => Ok(if dotnet { "mono_win64" } else { "win64" }),
        all(target_os = "windows", target_arch = "aarch64") => Ok(if dotnet { "mono_windows_arm64" } else { "windows_arm64" }),
        all(target_os = "linux", target_arch = "x86_64") => Ok(if dotnet { "mono_linux_x86_64" } else { "linux.x86_64" }),
        all(target_os = "linux", target_arch = "aarch64") => Ok(if dotnet { "mono_linux_arm64" } else { "linux.arm64" }),
        target_os = "macos" => Ok(if dotnet { "mono_macos.universal" } else { "macos.universal" }),
        _ => Err(InfoError::UnsupportedPlatform(cfg!(target_os))),
    }.map(|s| s.to_owned())
}

fn godot_platform() -> Result<String, InfoError> {
    cfg_select! {
        all(target_os = "windows", target_arch = "x86_64") => Ok("windows.64"),
        all(target_os = "windows", target_arch = "aarch64") => Ok("windows.arm64"),
        all(target_os = "linux", target_arch = "x86_64") => Ok("linux.64"),
        all(target_os = "linux", target_arch = "aarch64") => Ok("linux.arm64"),
        target_os = "macos" => Ok("macos.universal"),
        _ => Err(InfoError::UnsupportedPlatform(format!("{}, {}", cfg!(target_os), cfg!(target_arch)))),
    }
    .map(|s| s.to_owned())
}

fn godot_path(dotnet: bool, version: &str) -> Result<PathBuf, InfoError> {
    if cfg!(target_os = "macos") {
        return Ok(PathBuf::from("Godot_mono.app/Contents/MacOS/Godot"));
    }

    // this is a THIRD platform slug variant
    let platform = cfg_select! {
        all(target_os = "windows", target_arch = "x86_64") => Ok("win64.exe"),
        all(target_os = "windows", target_arch = "aarch64") => Ok("windows_arm64.exe"),
        all(target_os = "linux", target_arch = "x86_64") => Ok("linux.x86_64"),
        all(target_os = "linux", target_arch = "aarch64") => Ok("linux.arm64"),
        _ => Err(InfoError::UnsupportedPlatform(format!("{}, {}", cfg!(target_os), cfg!(target_arch)))),
    }
    .map(|s| s.to_owned())?;

    let mono = if dotnet { "_mono" } else { "" };

    Ok(PathBuf::from(format!("Godot_{version}{mono}_{platform}")))
}
