import { invoke } from "@tauri-apps/api/core";

export type Language = "auto" | "es" | "en" | "fr" | "zh" | "hi" | "ar" | "bn" | "pt";
export type IslandStyle = "notch" | "bubble";
export type GenieSpeed = "normal" | "fast" | "faster";
export type DockPosition = "left" | "bottom" | "right";
export type DockDensity = "compact" | "normal" | "spacious";
export type DockAutoHideSpeed = "smooth" | "normal" | "fast";
/** View of the existing two settings; saved atomically as a mutually exclusive pair. */
export type DockHidingMode = "visible" | "automatic" | "intelligent";
/** `auto` follows Windows' light or dark mode. */
export type Appearance = "auto" | "light" | "dark" | "frosted" | "clear" | "aurora";

/** Mirrors `muelle_config::Config`. */
export interface Config {
  version: number;
  pinned: string[];
  recent: string[];
  autoHide: boolean;
  smartHide: boolean;
  iconSize: number;
  magnification: boolean;
  magnifiedSize: number;
  dockPosition: DockPosition;
  dockDensity: DockDensity;
  dockAutoHideSpeed: DockAutoHideSpeed;
  showStart: boolean;
  showTrash: boolean;
  showDownloads: boolean;
  pinExplorer: boolean;
  genie: boolean;
  genieSpeed: GenieSpeed;
  showMinimized: boolean;
  showOtherWorkspaces: boolean;
  jumpLists: boolean;
  menuBar: boolean;
  menuBarAutoHide: boolean;
  aiUsage: boolean;
  menuBarMusic: boolean;
  menuBarIslandStyle: IslandStyle;
  accessoryBattery: boolean;
  showIndicators: boolean;
  showRecents: boolean;
  bounceOnOpen: boolean;
  notificationBadges: boolean;
  mascot: boolean;
  checkUpdates: boolean;
  autoUpdate: boolean;
  language: Language;
  appearance: Appearance;
  /** The folder shown at the right of the dock, whose contents open as a menu. */
  shortcutsFolder: string | null;
  /** Barra widgets. */
  widgets: WidgetSettings;
  /** Replace the Windows taskbar (hide it) while the dock runs. */
  hideWindowsTaskbar: boolean;
}

/** Mirrors `muelle_config::HomeItem`. */
export interface HomeItem {
  id: string;
  name: string;
  kind: string;
  entities: string[];
}

/** Mirrors `muelle_config::Widgets`. */
export interface WidgetSettings {
  barLeft: string[];
  barRight: string[];
  dock: string[];
  dockLeft: string[];
  islandLeft: string[];
  islandRight: string[];
  weatherCity: string;
  homeUrl: string;
  homeItems: HomeItem[];
  /** Size of each widget in the dock: compacto, mediano or ancho. */
  dockSizes: Record<string, string>;
  /** Other displays by device name: their own dock, menu bar and widgets. */
  screens: Record<string, ScreenWidgets>;
}

/** Mirrors `muelle_config::ScreenWidgets`. */
export interface ScreenWidgets {
  enabled: boolean;
  barLeft: string[];
  barRight: string[];
  dockLeft: string[];
  dock: string[];
  islandLeft: string[];
  islandRight: string[];
  dockSizes: Record<string, string>;
}

export interface DisplayInfo {
  device: string;
  label: string;
  primary: boolean;
}

export const listDisplays = () => invoke<DisplayInfo[]>("list_displays");

export const ICON_SIZE = { min: 32, max: 96 } as const;
export const MAX_MAGNIFIED_SIZE = 160;

export const loadConfig = () => invoke<Config>("get_config");
export const defaultConfig = () => invoke<Config>("default_config");
export const saveConfig = (patch: Partial<Config>) => invoke<Config>("set_config", { patch });
export const getAutostart = () => invoke<boolean>("get_autostart");
/** Current Windows values for display only; never merge these into Config. */
export interface WindowsPreferences {
  animations: boolean;
}

let windowsPreferencesRequest: Promise<WindowsPreferences> | undefined;
/** Coalesce overlapping focus/StrictMode reads, but reread after returning from Windows Settings. */
export const loadWindowsPreferences = () =>
  windowsPreferencesRequest ??= invoke<WindowsPreferences>("windows_preferences")
    .finally(() => { windowsPreferencesRequest = undefined; });
export const openAnimationSettings = () => invoke<void>("open_animation_settings");
export const setAutostart = (enabled: boolean) => invoke<void>("set_autostart", { enabled });
/** The folder the user picks, or null if they cancel. */
export const pickFolder = (current: string | null) =>
  invoke<string | null>("pick_folder", { current });
export interface UpdateInfo {
  version: string;
  /** Release page with the notes, if the manifest has one. */
  notes: string | null;
}

/** Resolves to the newer version available, or null when up to date. */
export const checkUpdate = () => invoke<UpdateInfo | null>("check_update");
export const openReleaseNotes = (url: string) => invoke<void>("open_release_notes", { url });
/** Fixed project URL, opened only when the user asks. */
export const openRepository = () => invoke<void>("open_repository");
let wallpaperRequest: Promise<string | null> | undefined;
/** One bounded native read per Settings process, including React StrictMode. */
export const loadWallpaperPreview = () =>
  wallpaperRequest ??= invoke<string | null>("wallpaper_preview").catch(() => null);
let wallpaperSelectionRequest: Promise<number | null> | undefined;
/** Share overlapping reads from StrictMode or quick section navigation. */
export const loadWallpaperSelection = () => {
  if (wallpaperSelectionRequest) return wallpaperSelectionRequest;
  const request = invoke<number | null>("wallpaper_selection").finally(() => {
    if (wallpaperSelectionRequest === request) wallpaperSelectionRequest = undefined;
  });
  wallpaperSelectionRequest = request;
  return request;
};
export const applyWallpaper = async (id: number, preview: string) => {
  await invoke<void>("apply_wallpaper", { id });
  // Original images stay in native storage; Settings uses only the small preview.
  wallpaperRequest = Promise.resolve(preview);
  wallpaperSelectionRequest = undefined;
};
export const installUpdate = () => invoke<void>("install_update");

/** Whether a Barra secret is saved; its value never comes back. */
export const secretSaved = (key: string) => invoke<boolean>("secret_saved", { key });
/** Saves a Barra secret in the Credential Manager, or removes it when empty. */
export const setSecret = (key: string, value: string) => invoke<void>("set_secret", { key, value });
export const importHome = () =>
  invoke<{ url: string; items: HomeItem[]; source: string }>("import_home");
