# keysound

A lightweight, low-latency Rust CLI daemon for Linux (EndeavourOS, Hyprland/Wayland, X11) that plays a sound on every key press system-wide.

## Features

- **Near-zero latency**: The chosen audio file is fully loaded and decoded into memory at startup. Key presses trigger cached audio buffers with no disk access or decoding overhead.
- **Polyphonic playback**: Rapid keystrokes overlap naturally without clipping or cutting off previous sounds.
- **Lightweight release binary**: Sub-2 MB binary size with minimal idle memory consumption.
- **Multiple format support**: Supports `wav`, `mp3`, `ogg` (Vorbis), `flac`, `m4a` / `aac`, and `mp4` (audio track).
- **Graceful hardware handling**: Automatically detects all input devices supporting `KEY_A` with one listener thread per keyboard. Gracefully handles device disconnections and exits cleanly if all keyboards disappear.
- **Foreground validation**: Verifies audio decoding and `/dev/input` permissions before spawning the daemon into the background.

---

## Prerequisites & Permissions

`keysound` captures keypress events by reading `/dev/input/event*` via Linux `evdev`. Your user must be a member of the `input` group.

1. Add your user to the `input` group:
   ```bash
   sudo usermod -aG input $USER
   ```
2. Log out and log back in (or reboot) for group permissions to take effect.
3. Verify your group membership:
   ```bash
   groups | grep input
   ```

---

## Installation

### Build from source

```bash
cargo build --release
```

### Install system-wide

```bash
sudo install -m755 target/release/keysound /usr/local/bin/keysound
```

Alternatively, install to your user local bin:

```bash
install -m755 target/release/keysound ~/.local/bin/keysound
```

---

## Usage

### Run in the foreground (interactive / on-demand)

If you only want `keysound` active while a terminal window is open, run:

```bash
keysound run <file> [--volume 0.8] [--max-ms 400]
```
Press `Ctrl+C` in that terminal whenever you want it to stop.

### Run as a background daemon

```bash
# Start in background
keysound start <file> [--volume 0.8] [--max-ms 400]

# Check status
keysound status

# Stop background daemon
keysound stop

# Restart with a new sound or settings
keysound restart <file> [--volume 0.8] [--max-ms 400]
```

### Options

- `<file>`: Path to audio file (`wav`, `mp3`, `ogg`, `flac`, `m4a`, `mp4`).
- `--volume <value>`: Audio volume multiplier (e.g. `0.8`, `1.0`). Default: `1.0`.
- `--max-ms <ms>`: Truncate sound length in milliseconds to keep key sounds crisp. Default: `400`.

### Examples

```bash
# Start with a mechanical switch click sound
keysound start ~/.config/sounds/cherry-blue.wav

# Start with an MP3 at 60% volume and 250ms duration limit
keysound start ~/.config/sounds/typewriter.mp3 --volume 0.6 --max-ms 250

# Check if the daemon is running
keysound status

# Switch to a different sound file
keysound restart ~/.config/sounds/bubble.mp3 --volume 0.8

# Stop the daemon
keysound stop
```

---

## Hyprland Autostart

To automatically start `keysound` when logging into Hyprland, add an `exec-once` entry to your `~/.config/hypr/hyprland.conf`:

```ini
exec-once = keysound start /path/to/sound.mp3 --volume 0.8 --max-ms 300
```

---

## Daemon & Logs

- **PID file**: `$XDG_RUNTIME_DIR/keysound.pid` (fallback `/tmp/keysound-$UID.pid`). Stale PID files are cleaned up automatically.
- **Log file**: `$XDG_RUNTIME_DIR/keysound.log` (fallback `/tmp/keysound-$UID.log`). The daemon logs any runtime warnings, hardware disconnections, or errors here.
- If an MP4 container has no compatible audio track, `keysound` prints:
  ```text
  Tip: ffmpeg -i in.mp4 -vn out.mp3
  ```
