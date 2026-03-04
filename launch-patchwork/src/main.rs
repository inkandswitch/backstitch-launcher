use std::io;
use std::process::Command;

fn main() -> io::Result<()> {
    // TODO: make this cross-platform
    let godot = std::env::current_dir()?.join("./godot_editor/godot.windows.editor.x86_64.exe");

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
