# OpenCrosshair

A crosshair overlay for Windows games. Free, open source, and small: one 7 MB exe written in Rust.

![OpenCrosshair settings window](docs/crosshair.png)

It draws your crosshair in a transparent, click-through window that sits on top of the game. It
never touches the game itself: no DLL injection, no hooks, no reading game memory. It's the same
approach most crosshair overlays use. Still, check your game's rules before using any
overlay in competitive play.

## Features

- **Line crosshairs** with length, thickness, gap, outline, T-style, centre dot and circle.
- **Pixel canvas** up to 64×64 for anything the sliders can't do, with mirrored painting and undo.
- **Presets**, and a different preset for each game.
- **Games only mode**: the crosshair shows while one of your games is focused and hides everywhere else.
- **Rebindable global hotkeys**: F8 toggles the crosshair, F9 opens the settings.
- **Start with Windows**, hidden until you press the hotkey.
- **Automatic updates** from GitHub releases. It waits until you've left your game before restarting.
- Centres on the game window you're focused on, so windowed games and second monitors just work.

![Pixel canvas, with the live preview beside it on a wide window](docs/canvas.png)

| Games | Presets | Settings |
| --- | --- | --- |
| ![Games page](docs/games.png) | ![Presets page](docs/presets.png) | ![Settings page](docs/settings.png) |

## Install

Download `OpenCrosshair-Setup.exe` from the [latest release](https://github.com/Tu2525/OpenCrosshair/releases/latest)
and run it. It installs for your user account only, so it doesn't need admin rights, and you can remove it
from Apps & features like anything else.

If you'd rather not install anything, download `OpenCrosshair.exe` instead and run it from wherever you like.

The exe isn't code-signed yet, so Windows SmartScreen may warn you the first time. Click
**More info**, then **Run anyway**.

Games need to run in **borderless** or **windowed** mode. Exclusive fullscreen takes over the display
and nothing can draw on top of it without hooking into the game, which this deliberately doesn't do.

## Using it

- **F8** shows or hides the crosshair, **F9** shows or hides the settings window. Both can be changed under Settings.
- Launching OpenCrosshair while it's already running just brings up the settings.
- Closing the settings window quits the app. To keep the crosshair running, hide the window with F9 instead.
- Settings are saved as you change them, to `%APPDATA%\OpenCrosshair\settings.json`.

## How it works

The overlay is a layered window (`WS_EX_LAYERED | WS_EX_TRANSPARENT`) drawn with `UpdateLayeredWindow`.
It's redrawn only when the crosshair actually changes, so while you play it uses no measurable CPU.

Focus changes come from an out-of-context `SetWinEventHook`, so there's no polling and nothing is loaded
into other processes. When the focused app is one of your games, the crosshair is centred on that window's
client area.

The settings window uses [egui](https://github.com/emilk/egui). The app icon is drawn from code in
`build.rs` at compile time, so there's no icon file to keep in sync.

Updates go through WinHTTP, which is part of Windows, so no TLS library ships with the app. A running exe
can't be overwritten, but it can be renamed: the updater downloads the new build, checks it arrived
complete, swaps the two files and restarts.

`OpenCrosshair-Setup.exe` is the same binary as `OpenCrosshair.exe`. When its file name contains "setup",
it installs itself instead of starting up.

| File | What's in it |
| --- | --- |
| `src/render.rs` | Turns crosshair settings into pixels |
| `src/overlay.rs` | The overlay window, hotkeys and focus tracking |
| `src/ui.rs` | Settings window |
| `src/config.rs` | Settings and presets, saved as JSON |
| `src/update.rs` | Self-updater |
| `src/install.rs` | Installer, uninstaller, start with Windows, single instance |

## Building

You need Rust (stable) and the Windows SDK, which comes with the Visual Studio Build Tools.

```
cargo build --release
```

The exe ends up in `target/release/OpenCrosshair.exe`. Copy it to a name containing `setup` if you want the installer.

## Releasing

Bump `version` in `Cargo.toml`, commit, then tag and push:

```
git tag v0.2.0
git push origin v0.2.0
```

GitHub Actions builds the release and attaches both exes. Installed copies check every six hours and update themselves.

## License

MIT. See [LICENSE](LICENSE).
