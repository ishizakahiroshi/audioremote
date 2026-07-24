# audioremote

Switch your **Windows 11 host's default audio output device** from any browser on the LAN.
No more walking back to the host to change the output from Nest Hub Max to wired earphones
during a meeting — do it from a Hyper-V guest, WSL2, your phone, or another PC.

> Status: **v0.1 in development** (C1 scaffold). Not yet released.
> See `docs/local/plan_audioremote-v0.1.md` for the full plan and `docs/local/mockup_audioremote-v0.1-ux_2026-07-24.html` for the UX mockup.

---

## How it works

```
Browser / phone / other PC
        ↓ HTTP(S)
audioremote server (Rust, runs as your logon user on the Windows 11 host)
        ↓ windows crate (COM)
Windows Core Audio (IMMDeviceEnumerator / IPolicyConfig)
        ↓
Physical devices (Nest Hub Max / wired earphones / headphones / …)
```

- The server runs in **your physical console session** (not as a SYSTEM service, and not inside an RDP session — audio endpoints belong to the interactive user's physical session; from an RDP session you only see the virtual "Remote Audio" endpoint).
- Default bind is **`127.0.0.1:17650`** (safe by default). Open to LAN via `config.toml` when you actually need remote control.
- Console / Multimedia / Communications default endpoints are always **switched together** (otherwise meeting apps still route to the old device via the Communications default).
- Switching is done by **device ID**, not display name (display names change on reconnect).

## Supported platforms

| Side | Support (v0.1) |
|---|---|
| Server (host) | **Windows 11 only.** Windows 10 is not officially supported. macOS / Linux are out of scope. |
| Client (browser) | Any modern browser on any OS. The built-in Web UI is served by the host binary. |

## Install (planned — not yet published)

Two entry points are planned for v0.1:

- **npm** (primary): `npm i -g audioremote` or one-off `npx audioremote`
  - Ships the platform-specific Rust binary via `optionalDependencies` (esbuild / Biome style).
  - Package-manager installs skip Mark of the Web, so SmartScreen warnings are avoided.
- **GitHub Releases** (secondary): direct `audioremote.exe` + `SHA256SUMS.txt` for Node-free users.

Neither channel is live yet. Names are reserved on all three: npm `audioremote`, crates.io `audioremote@0.0.0` (reservation only — real releases start at `0.1.0`), and this GitHub repo.

## Build from source (developer)

```powershell
# On Windows 11 with Rust stable
cargo build            # dev build
cargo run              # runs the C1 scaffold (prints a banner)
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
```

`cargo build --release` produces `target/release/audioremote.exe`.

## Roadmap

- **v0.1** — Host server + HTTP API + minimal built-in Web UI (this milestone).
- **v0.2** — Guest-resident companion app on Windows: tray-icon popup + global hotkey for the daily meeting toggle (e.g. Nest Hub Max ⇄ wired earphones). Same HTTP API, different client.
- **v0.3+** — Additional entry points (PWA / app window) as needed.

All clients are thin HTTP clients over the same API. The core architecture (host-resident server + HTTP API) does not change between versions.

## Configuration (v0.1)

Config lives at `%APPDATA%\audioremote\config.toml` (created automatically on first run). See the UX mockup for the concrete layout; the shape is roughly:

```toml
[server]
bind = "127.0.0.1"   # "0.0.0.0" to expose to LAN (token required)
port = 17650

[auth]
token = "ar_live_..."   # auto-generated on first run
require_token = true

[audio]
unify_roles = true      # switch Console / Multimedia / Communications together
device_sort = "state"   # "state" | "name" | "recent"
```

Device usage history (for `device_sort = "recent"`) is stored separately in `%APPDATA%\audioremote\history.toml` so editing config by hand does not clobber it.

## Security posture

- Bearer token authentication required by default on all API endpoints.
- CORS is not wide-open.
- When bound to LAN, the guest UI shows a **"LAN exposed"** badge as a reminder.
- No unsigned exe direct-download flow is recommended for end users; use the npm channel to avoid SmartScreen prompts.
- No HTTPS out of the box in v0.1. Reverse-proxy or manual TLS is left to the operator.

## Non-goals (v0.1)

- Tray icon / global hotkeys (that's v0.2's guest companion).
- Per-role (Console / Multimedia / Communications) individual switching UI.
- macOS / Linux server implementations.
- Windows 10 official support.
- Automatic HTTPS provisioning, mkcert integration, self-signed helpers.
- Code signing / winget submission.

## Project layout

```
audioremote/
├── src/                        Rust sources (binary crate)
├── Cargo.toml                  package definition
├── CLAUDE.md / AGENTS.md       AI entry points
├── LICENSE                     MIT
├── scripts/                    secrets-scan + hook installer
├── .githooks/                  layer 2 pre-commit
├── .github/workflows/          CI (validate.yml) + secrets-scan
└── docs/
    └── local/                  plan / recap / bugfix / mockup (tracked, no secrets)
```

## License

MIT — see [LICENSE](LICENSE). Copyright (c) 2026 Hiroshi Ishizaka (ishizakahiroshi).
