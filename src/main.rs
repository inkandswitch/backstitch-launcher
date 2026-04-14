use std::env;
use std::error::Error;
use std::io::{self, IsTerminal};
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
            println!("Attempting to launch old version...")
        }
    }

    let godot = update::get_godot_path();
    println!("Launching Godot from {:?}...", godot);

    let code = match Command::new(godot)
        .arg("-e")
        .arg("--path")
        .arg(".")
        .status()
    {
        Err(e) => {
            println!("Failed to launch Godot: {e}");
            return Err(Box::new(e));
        }
        Ok(status) => status,
    };

    if code.success() {
        println!("Godot editor launched successfully.");
    } else {
        let err = io::Error::new(
            io::ErrorKind::Other,
            format!("Godot editor exited with: {}", code),
        );
        println!("{err}");
        return Err(Box::new(err));
    }

    Ok(())
}


fn relaunch_in_terminal() -> Result<(), ()> {
    let exe = env::current_exe().expect("Failed to get current executable");

    // Try common terminal emulators
    let terminals = [
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xterm", &["-e"]),
        ("alacritty", &["-e"]),
    ];

    for (term, args) in terminals {
        let result = Command::new(term)
            .args(args)
            .arg(&exe)
            .spawn();

        if result.is_ok() {
            return Ok(());
        }
    }

    eprintln!("Failed to find a terminal emulator.");
    return Err(())
}

#[tokio::main]
async fn main() -> ExitCode {
    // Hacky fix to ensure we always launch a terminal for Godot. 
    // Queries a bunch of common terminal emulators...
    // If someone doesn't have any of these available... hopefully they know how to run it from the terminal.
    if cfg!(target_os = "linux") && !std::io::stdout().is_terminal() {
        // do we actually want to give up here, or try launching in the background?
        // giving up for now
        match relaunch_in_terminal() {
            Ok(_) => return ExitCode::SUCCESS,
            Err(_) => return ExitCode::FAILURE,
        }
    }

    #[cfg(target_os = "macos")]
    {
        // change cwd from the .app bundle to the project root
        let mut cwd = env::current_dir().expect("Failed to get current directory");
        println!("CWD: {:?}", cwd);
        if cwd.to_str().unwrap() == "/" {
            // change it to the executable's directory
            let exe = env::current_exe().expect("Failed to get current executable");
            let exe_dir = exe.parent().expect("Failed to get parent directory");
            cwd = exe_dir.to_path_buf();
        }
        // App translocation; we can't find the current directory, so we'll create a directory in the home directory
        if cwd.starts_with("/private"){
            cwd = untranslocator::resolve_translocated_path(&cwd).expect("Failed to resolve translocated path");
        } 
        if cwd.ends_with("Contents/MacOS") {
            let project_root = cwd.parent().expect("Failed to get parent directory").parent().expect("Failed to get parent2 directory").parent().expect("Failed to get parent3 directory");
            env::set_current_dir(project_root).expect("Failed to set current directory");
            println!("Changed CWD from {:?} to {:?}", cwd, project_root);
        } else{
            println!("Already in the project root");
        }
    }

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
