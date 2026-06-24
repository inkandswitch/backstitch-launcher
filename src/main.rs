use reqwest::Client;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use crate::config::CommandConfig;
use crate::utils::{LauncherError, fail};

pub mod backstitch_update;
pub mod config;
pub mod godot_update;
pub mod godot_urls;
pub mod utils;

async fn download_and_launch(config: &CommandConfig) -> Result<(), LauncherError> {
    let current_version = backstitch_update::get_current_version().await;

    let client = Client::builder()
        .user_agent("backstitch-launcher")
        .build()
        .map_err(|e| LauncherError::Unknown(e.to_string()))?;

    let new_version =
        match backstitch_update::try_update(&client, config, current_version.as_ref().ok()).await {
            Err(e) => match &current_version {
                Err(_) => {
                    println!("Error during Backstitch download: {e}");
                    println!("Stopping.");
                    return Err(e);
                }
                Ok(v) => {
                    println!("Error during Backstitch update: {e}");
                    println!("Attempting to launch old version...");
                    v.clone()
                }
            },
            Ok(v) => v,
        };

    let godot = godot_update::try_update(
        &client,
        config,
        current_version
            .ok()
            .as_ref()
            .map(|v| v.godot_version.as_str()),
        &new_version.godot_version,
    )
    .await?;

    println!("Launching Godot from {:?}...", godot);

    match Command::new(godot)
        .arg("-e")
        .arg("--path")
        .arg(".")
        .status()
    {
        Err(e) => {
            println!("Failed to launch Godot: {e}");
            Err(LauncherError::Launch(e.to_string()))
        }
        Ok(status) => {
            if status.success() {
                println!("Godot editor launched successfully.");
                Ok(())
            } else {
                Err(LauncherError::Exit(status))
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn relaunch_in_terminal(cwd: &PathBuf, exe: &PathBuf) -> Result<(), ()> {
    // Hacky fix to ensure we always launch a terminal for Godot.
    // Queries a bunch of common terminal emulators...
    // If someone doesn't have any of these available... hopefully they know how to run it from the terminal.

    // Try common terminal emulators
    let terminals = [
        ("xdg-terminal-exec", &["--"]),
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("ptyxis", &["--"]),
        ("konsole", &["-e"]),
        ("xterm", &["-e"]),
        ("alacritty", &["-e"]),
    ];

    for (term, args) in terminals {
        let result = Command::new(term)
            .args(args)
            .arg(exe)
            .current_dir(cwd)
            .spawn();

        if result.is_ok() {
            return Ok(());
        }
    }

    eprintln!("Failed to find a terminal emulator.");
    Err(())
}

#[cfg(target_os = "macos")]
fn relaunch_in_terminal(cwd: &PathBuf, exe: &PathBuf) -> Result<(), ()> {
    std::env::set_current_dir(cwd).expect("Failed to set current directory");
    let result = Command::new("open")
        .arg("-a")
        .arg("Terminal")
        .arg(exe)
        .current_dir(cwd)
        .spawn();
    if result.is_ok() {
        return Ok(());
    }

    eprintln!("Failed to launch terminal: {result:?}");
    Err(())
}

#[cfg(target_os = "windows")]
fn relaunch_in_terminal(cwd: &PathBuf, exe: &PathBuf) -> Result<(), ()> {
    // change cwd to the cwd param
    std::env::set_current_dir(cwd).expect("Failed to set current directory");
    let result = Command::new(exe).spawn();
    if result.is_ok() {
        return Ok(());
    }
    eprintln!("Failed to launch terminal: {result:?}");
    Err(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let config = match config::setup_config().await {
        Ok(config) => config,
        Err(e) => {
            println!("{}", e);
            fail();
            return ExitCode::FAILURE;
        }
    };
    #[allow(unused_mut)]
    let (mut cwd, mut exe) = {
        let cwd = std::env::current_dir().expect("Failed to get current directory");
        let exe = std::env::current_exe().expect("Failed to get current executable");
        (cwd, exe)
    };

    #[cfg(target_os = "linux")]
    {
        let project_root = exe
            .parent()
            .expect("Failed to get parent directory of current executable");
        if cwd != project_root {
            std::env::set_current_dir(project_root).expect("Failed to set current directory");
            println!("Changed CWD from {:?} to {:?}", cwd, project_root);
        }
        cwd = project_root.to_path_buf();
    };

    #[cfg(target_os = "macos")]
    {
        // change cwd from the .app bundle to the project root
        if exe.starts_with("/private") {
            exe = untranslocator::resolve_translocated_path(&exe)
                .expect("Failed to resolve translocated path");
        }
        let mut project_root = exe.parent().expect("Failed to get parent directory");
        if project_root.ends_with("Contents/MacOS") {
            project_root = project_root
                .parent()
                .expect("Failed to get parent directory")
                .parent()
                .expect("Failed to get parent2 directory")
                .parent()
                .expect("Failed to get parent3 directory");
        }
        if project_root != cwd {
            std::env::set_current_dir(&project_root).expect("Failed to set current directory");
            println!("Changed CWD from {:?} to {:?}", cwd, project_root);
        }
        cwd = project_root.to_path_buf();
    };

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::io::IsTerminal;
        if !std::io::stdout().is_terminal() {
            // do we actually want to give up here, or try launching in the background?
            // giving up for now
            match relaunch_in_terminal(&cwd, &exe) {
                Ok(_) => return ExitCode::SUCCESS,
                Err(_) => return ExitCode::FAILURE,
            }
        }
    }

    let res = download_and_launch(&config).await;
    // pause in case of error, so we can read it
    if let Err(e) = res {
        println!("Launcher error: {}", e);
        fail();
        return ExitCode::FAILURE;
    }
    return ExitCode::SUCCESS;
}
