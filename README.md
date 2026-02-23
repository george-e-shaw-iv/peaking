# Peaking

A self-hosted game clip recorder for Windows. Peaking continuously buffers your screen and audio in memory, and saves the last N seconds to an MP4 file whenever you press a hotkey — no cloud, no subscription, no account.

---

## How it works

Peaking has two independent components:

**Daemon** (`peaking-daemon`) — a Rust background process that:
- Watches for configured game executables using the Windows process list
- Captures the primary monitor via the **Windows Graphics Capture API** and system audio via **WASAPI** when a watched game is running
- Encodes frames in real time using **NVIDIA NVENC** (H.264) and AAC audio via FFmpeg, keeping only a rolling ring buffer of 1-second segments in RAM
- Flushes the buffer to an MP4 file at `<clip dir>\<game>\<timestamp>.mp4` on a configurable hotkey press (default: F8)
- Hot-reloads configuration without restarting
- Registers itself to run at Windows login

**GUI** (`peaking-gui`) — an Electron + React app that:
- Reads and writes the shared TOML config file
- Shows live daemon status (idle / recording / flushing)
- Lets you start, stop, and restart the daemon
- Manages the application list and per-game overrides
- Browses and plays saved clips in-app

Both components communicate solely through two files under `%APPDATA%\Peaking\`:

| File | Writer | Reader |
|------|--------|--------|
| `config.toml` | GUI | Daemon |
| `status.toml` | Daemon | GUI |

Either component can run independently — the daemon works headlessly without the GUI open.

---

## Requirements

- Windows 10/11
- NVIDIA GPU with NVENC support
- [FFmpeg 6.x Windows shared build](https://github.com/BtbN/FFmpeg-Builds/releases) (GPL, with NVENC)

---

## Building

### Daemon

Requires the Rust toolchain and MSVC build tools installed on Windows.

```powershell
# Point ffmpeg-sys-next at your FFmpeg SDK (set permanently or per-session)
$env:FFMPEG_DIR = "C:\ffmpeg"

cd daemon
cargo build --release
```

The FFmpeg `.dll` files from `%FFMPEG_DIR%\bin` must be available on `PATH` (or copied next to the exe) at runtime.

### GUI

Requires Node.js. Run from WSL or Windows.

```bash
cd gui
npm install
```

| Command | Description |
|---------|-------------|
| `npm run dev` | Dev server (Linux/WSL, for UI development) |
| `npm run dev:win` | Build and launch Windows exe directly from WSL |
| `npm run build:win` | Build unpacked Windows exe to `C:\Windows\Temp\peaking-dev\` |
| `npm run dist:win` | Build distributable Windows installer |
| `npm test` | Run the renderer test suite |

---

## Configuration

Config is stored at `%APPDATA%\Peaking\config.toml` and written by the GUI. The daemon hot-reloads it on change.

```toml
[global]
buffer_length_secs = 15   # 5–120 seconds
hotkey = "F8"
clip_output_dir = "%USERPROFILE%\\Videos\\Peaking"

[[applications]]
display_name    = "Rocket League"
executable_name = "RocketLeague.exe"
executable_path = "C:\\...\\RocketLeague.exe"
# buffer_length_secs = 30  # optional per-game override
# hotkey = "F9"            # optional per-game override
```

---

## Usage

1. Open the GUI and go to **Settings**
2. Add the games you want to record and configure your clip directory
3. Click **Save Settings**
4. Start the daemon via the **Status** page (or let it start automatically at login after the first run)
5. Launch a configured game — the daemon begins buffering automatically
6. Press **F8** (or your configured hotkey) to save the last N seconds as a clip
7. View, play, and manage clips in the **Clips** tab

Clips are saved as `<clip_output_dir>\<game name>\YYYY-MM-DD_HH-MM-SS.mp4`.
