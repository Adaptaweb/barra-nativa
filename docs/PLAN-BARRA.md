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
2. [x] Build and run (`cargo run -p muelle`; `cargo run -p muelle -- --restore` restores the taskbar).
   Settings › Dock › "Reemplazar la barra de tareas de Windows" (`hideWindowsTaskbar`, on by
   default) hides it or leaves it next to the dock; it applies live. The user keeps it off
   while developing. The Start menu and the Windows key are untouched.
3. [x] Widget framework in `apps/dock/src/widgets/`: sources on their own threads publish
   readings and post `WIDGETS_CHANGED` to the menu bar, which repaints only the widgets
   (and the dock tiles) whose reading changed. Free-form drawing through `gfx::Pen`
   (`gfx/pen.rs`). Placement in `config.json` → `widgets`:
   `{"barLeft":[], "barRight":["clima","sistema"], "dock":["sistema","clima"], "weatherCity":"Santiago"}`.
   Clicks: sistema opens Task Manager, clima opens the Weather app. Widgets are a dock
   `Role::Widget` between the apps and the folders.
   Measured idle cost in a debug build with both widgets in bar and dock: ~0.02 s CPU per 10 s.
4. [ ] Port data sources from the Tauri Barra (`barra/src-tauri/src/fuentes/*.rs`, already Rust):
   done: system (CPU/RAM via Win32, temperature via LHM/fan helper), weather (Open-Meteo),
   `reloj` (clock + next iCal event; replaces the bar clock when placed), `casa` (Home
   Assistant states; settings from `widgets.homeUrl/homeItems`, else the Tauri Barra's),
   `claude` / `chatgpt` / `gemini` (Muelle's usage reader run as `--usage-worker all` in a
   child process every 5 min). Secrets are read from the Credential Manager entries the
   Tauri Barra saved (`Barra/calendario.ics`, `Barra/casa.token`).
   The projects widget is set aside for now (user's decision, 2026-11-24).
   Panels: done (`widgets/panel.rs` window + `panel_content.rs`). A click on a widget in the
   bar or the dock opens a frosted panel built from blocks; Home Assistant buttons toggle;
   calendar lists the next events or explains how to (re)connect; AI shows each window with
   its reset. Debug builds: `BARRA_PANEL=casa` opens a panel at start for screenshots.
   Note: a process started from the PowerShell tool sees a different %APPDATA% (sandbox);
   launch Muelle from bash to use the real config.
   Claude shows "Abre Claude Code" while the CLI's OAuth token in `~/.claude` is expired.
5. [ ] Draw each widget natively (designs from the Tauri Barra and the user's references).
6. [x] Settings app: "Widgets" section (`apps/settings/src/WidgetsSection.tsx`, commands in
   `src-tauri/src/barra.rs`): add / remove / reorder per zone, weather city, calendar links
   and Home Assistant token (Credential Manager), Home Assistant import. Build the front end
   with `npx -y pnpm@10 install` and `npx -y pnpm@10 build` (pnpm is not installed globally).
   `VITE_SECTION=widgets` opens that section first. Pending: drag to reorder.
   Dock widgets are wide transparent cells like the Tauri Barra's (`widgets/cell.rs`, layout
   `ItemKind::Wide(hundredths of an icon)`, no magnification), sized compacto / mediano /
   ancho from `widgets.dockSizes` (Settings › Widgets › Dock). Each cell is as wide as its
   content (measured on render, cached, in tenths of an icon); wide cells grow a quarter as
   much as icons under the cursor. Zones: `widgets.dockLeft` (before the apps) and
   `widgets.dock` (at the far end); the apps stay centred on the screen when it has room.
   `bandeja` is a chevron at the far right end of the dock, always last; its panel (bubble)
   holds all the tray icons (click, right click for the app's menu). Panels stay open while
   used and close on a click outside them (polled every 50 ms; app menus count as inside). Source:
   `muelle_shell::tray`, an own `Shell_TrayWnd` that copies every Shell_NotifyIcon call and
   forwards it to Explorer (ported from the Tauri Barra); `taskbar::taskbars` skips it.
   The dock does not magnify while the pointer is over a widget cell. Debug builds:
   `BARRA_MONITOR=x,y` puts the dock on the display at that point (screenshots while the
   primary screen is busy with a full-screen app).
7. [ ] One dock and top bar per monitor, with per-screen widgets and on/off.
8. [ ] Rebrand (name, icon, AppUserModelID, installer) before distributing to other PCs.
