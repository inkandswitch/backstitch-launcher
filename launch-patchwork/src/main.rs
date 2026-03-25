use std::io;
use std::process::Command;

fn await_confirmation() -> () {
    println!("Press Enter to continue...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}

fn main() -> io::Result<()> {
    // TODO: make this cross-platform
    let godot = std::env::current_dir()?.join("./godot_editor/godot.windows.editor.x86_64.exe");

    println!("Launching Godot from {:?}...", godot);

    match Command::new(godot)
        .arg("-e")
        .arg("--path")
        .arg(".")
        .status() {
            Ok(status) if status.success() => {
                println!("Godot editor launched successfully.");
            }
            Ok(status) => {
                println!("Godot editor exited with: {}", status);
                await_confirmation();
            }
            Err(e) => {
                println!("Failed to launch Godot: {}", e);
                await_confirmation();
            }
        }

    Ok(())
}
