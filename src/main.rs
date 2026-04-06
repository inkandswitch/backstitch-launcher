use std::error::Error;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

pub mod update;

async fn download_and_launch() -> Result<(), Box<dyn Error>> {
    let current_version = update::get_current_version().await;
    let res = update::try_update(current_version.clone()).await;

    if let Err(e) = res {
        if current_version.is_none() {
            println!("Error during Backstitch download: {e}");
            println!("Stopping.");
            return Err(e);
        } else {
            println!("Error during Backstitch update: {e}");
            println!("Launching old version...")
        }
    }

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

    let godot: PathBuf = std::env::current_dir()?.join("godot_editor").join(exe_name);

    println!("Launching Godot from {:?}...", godot);

    let status = Command::new(godot)
        .arg("-e")
        .arg("--path")
        .arg(".")
        .status()?;

    if status.success() {
        println!("Godot editor launched successfully.");
    } else {
        println!("Godot editor exited with: {}", status);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let res = download_and_launch().await;
    // pause in case of error, so we can read it
    if let Err(_) = res {
        println!("Press Enter to continue...");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        return ExitCode::FAILURE;
    }
    return ExitCode::SUCCESS;
}
