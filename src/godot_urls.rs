use std::path::PathBuf;

use reqwest::Url;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InfoError {
    #[error("malformed version {0}")]
    MalformedVersion(String),
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
    let slug = godot_slug(dotnet);
    let platform = godot_platform();
    let url = Url::parse(&format!(
        "https://downloads.godotengine.org/?version={short_version}&flavor={flavor}&slug={slug_prefix}{slug}.zip&platform={platform}"
    )).map_err(|e| InfoError::Unknown(e.to_string()))?;
    let path = godot_path(dotnet, version);

    Ok(GodotDownloadInfo {
        url,
        exe_path: path,
    })
}

fn godot_slug(dotnet: bool) -> String {
    cfg_select! {
        all(target_os = "windows", target_arch = "x86_64") => if dotnet { "mono_win64" } else { "win64" },
        all(target_os = "windows", target_arch = "aarch64") => if dotnet { "mono_windows_arm64" } else { "windows_arm64" },
        all(target_os = "linux", target_arch = "x86_64") => if dotnet { "mono_linux_x86_64" } else { "linux.x86_64" },
        all(target_os = "linux", target_arch = "aarch64") => if dotnet { "mono_linux_arm64" } else { "linux.arm64" },
        target_os = "macos" => if dotnet { "mono_macos.universal" } else { "macos.universal" },
        _ => compile_error!("unsupported platform!"),
    }.to_owned()
}

fn godot_platform() -> String {
    cfg_select! {
        all(target_os = "windows", target_arch = "x86_64") => "windows.64",
        all(target_os = "windows", target_arch = "aarch64") => "windows.arm64",
        all(target_os = "linux", target_arch = "x86_64") => "linux.64",
        all(target_os = "linux", target_arch = "aarch64") => "linux.arm64",
        target_os = "macos" => "macos.universal",
        _ => compile_error!("unsupported platform!"),
    }
    .to_owned()
}

fn godot_path(dotnet: bool, version: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        return PathBuf::from("Godot_mono.app/Contents/MacOS/Godot");
    }

    // this is a THIRD platform slug variant
    let platform = cfg_select! {
        all(target_os = "windows", target_arch = "x86_64") => "win64.exe",
        all(target_os = "windows", target_arch = "aarch64") => "windows_arm64.exe",
        all(target_os = "linux", target_arch = "x86_64") => "linux.x86_64",
        all(target_os = "linux", target_arch = "aarch64") => "linux.arm64",
        _ => compile_error!("unsupported platform!"),
    }
    .to_owned();

    let mono = if dotnet { "_mono" } else { "" };

    PathBuf::from(format!("Godot_{version}{mono}_{platform}"))
}
