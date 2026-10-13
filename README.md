# Clipman

> **A fast, lightweight clipboard manager for Linux (optimized for GNOME & Ubuntu)**

**Clipman** is an ultra-fast, minimalist clipboard manager designed for the Linux desktop. Powered by **Rust** and **GPUI** (the GPU-accelerated UI framework developed for the Zed editor), Clipman runs a lightweight background daemon consuming just **~11 MB of RAM** while providing an instant popup selector in under 80 milliseconds.

---

## Features

- **Blazing Fast & Resource-Efficient**: Dual-process architecture ensures zero GPU/UI memory footprint while idle. Uses event-driven XFixes notifications without CPU polling.
- **Text, Images & Files**: Seamlessly captures plain text, code, copied images (`image/png` with fast disk caching & previews), and files (supporting both GNOME Nautilus and `text/uri-list` standards).
- **Instant Search & Filter**: Real-time fuzzy-like search with dedicated filter tabs: **All**, **Pinned**, **Text**, **Images**, and **Files**.
- **Pinning Support**: Pin important items (`Ctrl+P`) so they never get pruned from history.
- **Privacy First**: Automatically skips sensitive entries containing password manager hints (`x-kde-passwordManagerHint`), with an instant capture pause toggle.
- **Native Look & Feel**: Centered floating window that automatically adapts to your system Dark and Light themes.
- **System Tray Integration**: Compact StatusNotifierItem / AppIndicator tray icon for quick access and status monitoring.

---

## Installation

### One-Line Install (Recommended)

Run the automated installer script via `curl`:

```bash
curl -fsSL https://raw.githubusercontent.com/dsnbyte/clipman/main/install.sh | bash
```

The installer will:
1. Download the latest prebuilt `clipman` binary into `~/.local/bin/`.
2. Register the desktop launcher (`~/.local/share/applications/clipman.desktop`).
3. Enable autostart on login (`~/.config/autostart/clipman.desktop`).
4. Set up the default global keybinding (**`Ctrl + Alt + V`**) in GNOME.

### Manual Download

Download the latest prebuilt tarball from the [GitHub Releases](https://github.com/dsnbyte/clipman/releases) page, extract the binary, and run:

```bash
mv clipman ~/.local/bin/
clipman install
```

---

## Usage

### Global Shortcut

Press **`Ctrl + Alt + V`** anywhere to summon the clipboard selector.

### Selector Controls

| Key | Action |
| --- | --- |
| `↑` / `↓` or `Ctrl + K` / `Ctrl + J` | Navigate items |
| `Enter` | Copy selected item to clipboard and close |
| `Ctrl + P` | Pin / unpin item |
| `Delete` | Remove item from history |
| `Tab` / `Shift + Tab` | Switch filter categories |
| `Ctrl + ,` | Open Settings |
| `Esc` | Close selector |

### CLI Commands

```bash
clipman toggle      # Show or hide the clipboard window (ideal for custom shortcuts)
clipman show        # Open the clipboard window
clipman settings    # Open preferences dialog
clipman pause       # Pause / resume clipboard capturing
clipman daemon      # Run background service manually
clipman quit        # Stop the background service
clipman install     # Register desktop launcher, autostart, and default shortcut
clipman uninstall   # Remove desktop launcher, autostart, and shortcut
```

---

## Desktop Compatibility

Clipman is built for the **Linux desktop ecosystem** and works on any modern distribution:

- **Display Server**:
  - **X11**: Supported natively.
  - **Wayland**: Supported via XWayland (default on Ubuntu, Fedora GNOME, and most desktop setups).
- **GNOME / Ubuntu**:
  - Out-of-the-box zero configuration. Shortcut (`Ctrl + Alt + V`), autostart, and dark mode detection are configured automatically via `gsettings`.
- **KDE Plasma, XFCE, and Window Managers (i3, Sway, Hyprland)**:
  - All core capabilities (daemon, history capture, tray icon, and GPUI selector) work seamlessly.
  - Since automatic keybinding registration relies on GNOME settings, simply bind your preferred key combination in your desktop's shortcut settings to execute:
    ```bash
    clipman toggle
    ```

<details>
<summary><b>Example shortcut configs for window managers</b></summary>

**i3 / Sway (`~/.config/i3/config` or `~/.config/sway/config`):**
```text
bindsym $mod+v exec --no-startup-id clipman toggle
```

**Hyprland (`~/.config/hypr/hyprland.conf`):**
```text
bind = $mainMod, V, exec, clipman toggle
```

</details>

---

## Configuration

Configuration is stored in `~/.config/clipman/config.toml` (created automatically on first launch or via the in-app Settings screen):


```toml
max_items = 200     # Maximum number of unpinned items kept in history
max_text_kb = 1024  # Maximum text size to record (in KiB)
max_image_mb = 16   # Maximum image size to record (in MiB)
```

---

## Building from Source

### Prerequisites

Ensure you have a working [Rust toolchain](https://rustup.rs/) (2024 edition) and the required system development headers installed:

**Ubuntu / Debian:**
```bash
sudo apt update && sudo apt install -y build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev
```

**Fedora:**
```bash
sudo dnf install -y gcc pkgconf-pkg-config libxkbcommon-devel libxkbcommon-x11-devel fontconfig-devel
```

**Arch Linux:**
```bash
sudo pacman -S --needed base-devel libxkbcommon fontconfig
```

### Build & Install

```bash
git clone https://github.com/dsnbyte/clipman.git
cd clipman
cargo build --release

# Install launcher, autostart, and shortcuts
./target/release/clipman install
```

The optimized binary will be available at `target/release/clipman`.

---

## License

This project is licensed under the [MIT License](LICENSE).
