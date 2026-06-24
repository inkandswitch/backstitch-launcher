# Backstitch Launcher

A launcher for [Backstitch](https://backstitch.dev/). When dropped in a project, this launcher will ensure you have the correct version of Backstitch and Godot, and run your project.

Join our [Discord](https://discord.gg/SkW9vem5Ez) to connect with the Backstitch community!

## How to Use

1. Download the [latest release](https://github.com/inkandswitch/backstitch-launcher/releases).
2. Unzip the folder, and place its contents in the root directory.
3. If you use Git, merge the `.gitignore.template` with your own `.gitignore`.
4. Run the launcher for your platform to open your project with the latest version of Backstitch:
    - **macOS**: run `backstitch-launcher-macos.app`
    - **Windows**: run `backstitch-launcher-windows.exe`
    - **Linux**: run `backstitch-launcher-linux`


## Configuration

The Backstitch Launcher has a variety of useful command-line arguments. To view a list of arguments, type `backstitch-launcher-windows.exe --help` (for Windows, or an equivalent for your platform).


Alternatively, you may include a section for configuration in your `backstitch.cfg` like so:

```toml
[backstitch_launcher]
dotnet="<auto|true|false>" # Whether to use the .NET build of Godot. If set to auto (default), it automatically checks for .NET build files and picks a .NET version if so.
allow_prerelease=<true|false> # Whether to allow downloading pre-release versions of Backstitch.
godot_path="<GODOT_PATH>" # Skip the Godot download and open the exe from a path instead. 
godot_url="<GODOT_URL>" # Specify a custom Godot URL to download from. Cross-platform support doesn't work, so it had better match your mono and platform versions!
```

Command-line specified arguments will take precedence over the `[backstitch_launcher]` section.