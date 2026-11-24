# Barra nativa — plan

Fork of [Muelle](https://github.com/crafter-station/Muelle) (MIT) on branch `barra`.
Remote `upstream` points at Muelle so its fixes can be merged later
(`git fetch upstream && git merge upstream/main`).

Goal: the features of the Tauri Barra (`Proyectos\barra`) on a native Rust
renderer (Direct2D + DirectWrite + Windows.UI.Composition, no WebView), so it
uses far less RAM and CPU.

## Where things live in Muelle

| Path | What |
|---|---|
| `apps/dock/src/app*.rs` | Dock window, input, drag, previews, Genie |
| `apps/dock/src/menubar.rs` | Top bar (5.4k lines, one struct): app name, status icons, clock, "island" with music and AI usage |
| `apps/dock/src/usage.rs` | AI plan limits: Claude (OAuth usage API), Codex, Gemini, Antigravity, OpenCode, Z.ai |
| `apps/dock/src/gfx.rs`, `scene.rs` | Direct2D/DirectWrite drawing helpers and composition scene |
| `crates/muelle-core` | Platform-independent layout and animation (magnify, spring, genie) |
| `crates/muelle-shell` | Windows shell integration (appbar, taskbar hiding, tray, status) |
| `crates/muelle-config` | Settings struct and persistence |
| `apps/settings` | Tauri + React settings app (only open while configuring) |

## Gaps against the Tauri Barra

- **Widgets**: Muelle has no widget system. Top bar items are hard-coded in `menubar.rs`.
- **Multi-monitor**: dock and top bar live on the primary screen only.
- Weather, Home Assistant, projects, calendar, CPU temperature, control center sliders.

## Steps

1. [x] Fork into `Proyectos\barra-nativa`, branch `barra`.
2. [ ] Build and run (`cargo run -p muelle`; `cargo run -p muelle -- --restore` restores the taskbar).
   During development the Windows taskbar must stay visible: add a dev switch that skips hiding it.
3. [ ] Widget framework: a `Widget` trait (measure, draw with `Gfx`, hit test, click, panel)
   in a new `apps/dock/src/widgets/` module, with two hosts: the top bar (left / right zones)
   and the dock (right side, after the separator). Config lists which widgets go in which zone.
4. [ ] Port data sources from the Tauri Barra (`barra/src-tauri/src/fuentes/*.rs`, already Rust):
   system (CPU/RAM/temperature), weather, calendar (iCal), Home Assistant, projects.
5. [ ] Draw each widget natively (designs from the Tauri Barra and the user's references).
6. [ ] Settings app: widget editor (zones, order, sizes) in `apps/settings`.
7. [ ] One dock and top bar per monitor, with per-screen widgets and on/off.
8. [ ] Rebrand (name, icon, AppUserModelID, installer) before distributing to other PCs.
