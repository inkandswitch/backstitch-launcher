use reqwest::Client;
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
fn relaunch_in_terminal_linux() -> Result<(), ()> {
    let exe = std::env::current_exe().expect("Failed to get current executable");

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
        let result = Command::new(term).args(args).arg(&exe).spawn();

        if result.is_ok() {
            return Ok(());
        }
    }

    eprintln!("Failed to find a terminal emulator.");
    return Err(());
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

    #[cfg(target_os = "linux")]
    {
        // Hacky fix to ensure we always launch a terminal for Godot.
        // Queries a bunch of common terminal emulators...
        // If someone doesn't have any of these available... hopefully they know how to run it from the terminal.
        if !std::io::stdout().is_terminal() {
            // do we actually want to give up here, or try launching in the background?
            // giving up for now
            match relaunch_in_terminal_linux() {
                Ok(_) => return ExitCode::SUCCESS,
                Err(_) => return ExitCode::FAILURE,
            }
        }

        let cwd = std::env::current_dir().expect("Failed to get current directory");
        let exe = std::env::current_exe().expect("Failed to get current executable");
        let project_root = exe
            .parent()
            .expect("Failed to get parent directory of current executable");
        if cwd != project_root {
            std::env::set_current_dir(project_root).expect("Failed to set current directory");
            println!("Changed CWD from {:?} to {:?}", cwd, project_root);
        }
    }

    #[cfg(target_os = "macos")]
    {
        // change cwd from the .app bundle to the project root
        let mut cwd = std::env::current_dir().expect("Failed to get current directory");
        println!("CWD: {:?}", cwd);
        if cwd.to_str().unwrap() == "/" {
            // change it to the executable's directory
            let exe = std::env::current_exe().expect("Failed to get current executable");
            let exe_dir = exe.parent().expect("Failed to get parent directory");
            cwd = exe_dir.to_path_buf();
        }
        // App translocation; we can't find the current directory, so we'll create a directory in the home directory
        if cwd.starts_with("/private") {
            cwd = untranslocator::resolve_translocated_path(&cwd)
                .expect("Failed to resolve translocated path");
        }
        if cwd.ends_with("Contents/MacOS") {
            let project_root = cwd
                .parent()
                .expect("Failed to get parent directory")
                .parent()
                .expect("Failed to get parent2 directory")
                .parent()
                .expect("Failed to get parent3 directory");
            std::env::set_current_dir(project_root).expect("Failed to set current directory");
            println!("Changed CWD from {:?} to {:?}", cwd, project_root);
        } else {
            println!("Already in the project root");
        }
    }

    let res = download_and_launch(&config).await;
    // pause in case of error, so we can read it
    if let Err(e) = res {
        println!("{e}");
        fail();
        return ExitCode::FAILURE;
    }
    return ExitCode::SUCCESS;
}
