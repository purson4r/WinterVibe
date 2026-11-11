# WinterVibe

WinterVibe is a customizable snowfall overlay for Windows 11. It is based on [Snowhush](https://github.com/mbezruchko/Snowhush), with a Windows-focused interface, Russian and English localization, snowfall presets, and additional blizzard controls.

## Features

- Five built-in snowfall profiles:
  - **Light snowfall**
  - **Snowfall**
  - **Weak blizzard**
  - **Strong blizzard**
  - **Blizzard** — dense snow with powerful, predominantly horizontal gusts
- **Custom settings** for snowfall intensity, density, wind, turbulence, flake movement, and snowdrift shape.
- Interactive snow physics: flakes respond to the cursor and moving windows.
- Natural, uneven snowdrifts can accumulate along the desktop and window edges.
- Controls for accumulation, melting, snow on windows, taskbar layering, and screenshot visibility.
- Russian is selected by default; English is available in the tray menu.
- Optional, voluntary EVM donations.

## Download and install

1. Download `WinterVibe-v0.1.0-beta-windows-x64.zip` from the [v0.1.0-beta release](https://github.com/purson4r/WinterVibe/releases/tag/v0.1.0-beta).
2. Extract the archive to a folder you can write to.
3. Run `WinterVibe.exe`. The app appears in the Windows notification area (system tray).

This beta is distributed as a portable ZIP; it does not require an installer or administrator privileges. To exit, open the tray menu and choose **Quit** / **Выход**.

## System requirements

- Windows 11, 64-bit
- A display driver supporting Windows Direct2D/Direct3D
- Internet access is only needed to check for releases; the snowfall simulation works offline

## Build from source

Install the stable Rust toolchain for the `x86_64-pc-windows-msvc` target and the Visual Studio C++ Build Tools. From the repository root, run:

```powershell
cargo check --manifest-path windows/Cargo.toml
cargo test --manifest-path windows/Cargo.toml
cargo test --manifest-path core/Cargo.toml
cargo build --manifest-path windows/Cargo.toml --release --bin WinterVibe
```

The executable is written to `windows/target/release/WinterVibe.exe`.

## Based on Snowhush

WinterVibe is derived from **Snowhush v0.6.1** by **Mykhailo Bezruchko**. The original project and its source history are available at [github.com/mbezruchko/Snowhush](https://github.com/mbezruchko/Snowhush). Snowhush's MIT license and copyright notice are retained in this repository. WinterVibe is an independent community derivative; the original author is not responsible for WinterVibe releases.

## License and third-party notices

WinterVibe is distributed under the [MIT License](LICENSE). See [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) for attribution and dependency licensing information.

## Support development

Donations are optional and are not required to use WinterVibe. If you choose to support development, the EVM address is:

`0xcC170d39cEE9786CdBe9E75a5586a3294191b91C`

Only send assets on a network compatible with your wallet and verify the address before sending.
