# AutoPipe Frontend (Tauri + Svelte)

This is the new web-based UI for the AutoPipe desktop app. It replaces the
egui-based GUI in `crates/desktop/src/app.rs` while keeping all the Rust
backend logic (MCP server, SSH, config, etc.) intact.

## Architecture

```
frontend/
├── src/                # Svelte UI (4 tabs: Setup / SSH / GitHub / Status)
├── src-tauri/          # Tauri config + bridge to existing Rust crates
└── package.json
```

The Rust backend lives in `../crates/desktop/` and is consumed by Tauri
through new `commands.rs` wrappers (TODO: see Phase 2 below).

## Setup (one-time)

### Prerequisites

- Node.js 20+ (`node --version`)
- Rust toolchain (`rustc --version`)
- OS-specific webview deps:
  - **macOS**: nothing extra (WKWebView built in)
  - **Windows**: WebView2 (preinstalled on Windows 11; auto-installed on Win10)
  - **Linux**: `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev`

### Install Tauri CLI

```bash
cd frontend
npm install
```

## Development

```bash
cd frontend
npm run tauri dev
```

This will:
1. Start Vite dev server on http://localhost:1420 (Svelte hot reload)
2. Compile the Rust backend
3. Open the Tauri window pointing at the dev server

UI changes hot-reload instantly. Rust changes trigger a rebuild.

## Production build

```bash
cd frontend
npm run tauri build
```

Outputs:
- macOS: `src-tauri/target/release/bundle/dmg/AutoPipe.dmg`
- Windows: `src-tauri/target/release/bundle/nsis/AutoPipe-Setup.exe`
- Linux: `src-tauri/target/release/bundle/deb/autopipe.deb`

## Migration roadmap

- [x] **Phase 1**: Frontend skeleton — Svelte UI scaffolded
- [x] **Phase 2**: `commands.rs` exposing 11 Rust functions to JS
- [x] **Phase 3**: Tray icon (left-click brings window back, right-click for menu)
- [x] **Phase 4** (partial): autopipe-tauri binary with close-to-hide
- [x] **Phase 7** (initial): single-page layout with Required/Optional badges,
  collapsible advanced section, sticky save bar, gradient action button
- [ ] **Phase 5**: Remove `crates/desktop/src/app.rs` and the `gui` Cargo feature
- [ ] **Phase 6**: Update GitHub Actions release workflow to use `cargo tauri build`
- [ ] **Phase 7** (more): Tailwind, dark mode, refined typography

## Building (current state)

The new Svelte UI is its own Cargo binary at `frontend/src-tauri/` (named
`autopipe-tauri`). It depends on the existing `autopipe_desktop` library
crate so all the MCP server / SSH / config logic is shared with the legacy
binary.

### Dev mode (with hot reload)

```bash
cd frontend
npm install              # one-time, installs Svelte + Tauri CLI
npm run tauri dev
```

This starts Vite on :1420, builds the `autopipe-tauri` Rust binary, and
opens the Tauri window. Frontend changes hot-reload; Rust changes trigger
a rebuild.

### Production build

```bash
cd frontend
npm install              # one-time
npm run tauri build
```

Outputs go to `target/release/bundle/` at the workspace root (Tauri uses
the workspace target dir).

### Legacy egui build (unchanged)

```bash
cd autopipe-app
cargo build --release -p autopipe --features gui
```

Builds the original `autopipe` binary (no Tauri involvement).

## What's NOT yet wired up

- Tray icon — currently the close button hides the window but there is no
  tray icon to bring it back. Until Phase 3 lands, restart the binary or
  use `wmctrl`/Spotlight to refocus.
- Plugins tab — present in the egui UI but not yet ported. The frontend
  shows 4 tabs (Setup / SSH / GitHub / Status); plugins will be added.
- Some smaller UI affordances (test SSH connection button, validation
  badges next to tab names, etc.) are not yet wired.

## Notes for Phase 2 implementers

The Tauri bridge needs:

1. A new `crates/desktop/src/commands.rs` that wraps existing functions:
   ```rust
   #[tauri::command]
   async fn register_mcp() -> Result<Vec<String>, String> {
       let cfg = AppConfig::load();
       let url = cli_mcp_url(&cfg);
       let token = config::load_or_create_mcp_token().map_err(|e| e.to_string())?;
       let results = claude_config::register_all(&url, &token);
       Ok(results.iter().filter(|(_, r)| r.is_ok()).map(|(c, _)| c.name().to_string()).collect())
   }
   ```

2. `Cargo.toml` updates: add `tauri = { version = "2", features = [...] }` as
   an optional dependency behind a `tauri-ui` feature.

3. `main.rs` branching: when `--tauri-ui` (or default mode) is requested,
   start `tauri::Builder` instead of eframe.
