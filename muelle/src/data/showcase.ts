import type { ImageMetadata } from "astro";
import downloads from "@/assets/icons/downloads.webp";
import fileExplorer from "@/assets/icons/file-explorer.webp";
import microsoftPhotos from "@/assets/icons/microsoft-photos.webp";
import microsoftStore from "@/assets/icons/microsoft-store.webp";
import microsoftTeams from "@/assets/icons/microsoft-teams.webp";
import recycleBin from "@/assets/icons/recycle-bin.webp";
import settings from "@/assets/icons/settings.webp";
import windowsMediaPlayer from "@/assets/icons/windows-media-player.webp";
import windowsTerminal from "@/assets/icons/windows-terminal.webp";
import { figures } from "@/data/stats";
import { site } from "@/data/site";

/* ---------------------------------------------------------------- Anatomy */

/** A tile of the annotated dock: an app icon, or one of the two drawn tiles. */
export type AnatomyTile =
  | { kind: "app"; icon: ImageMetadata; label: string; open?: boolean }
  | { kind: "start" }
  | { kind: "window" };

/** One labelled group of the dock; labels alternate above and below the glass. */
export interface AnatomyGroup {
  label: string;
  detail: string;
  side: "top" | "bottom";
  tiles: AnatomyTile[];
  /** A thin divider is drawn before this group, as in the real dock. */
  divider?: boolean;
}

export const anatomyIntro = {
  eyebrow: "The Dock",
  title: "Everything in its place.",
  lead: "Your apps, files and open windows in one glass shelf. Place it along the bottom or either side, and keep only the shortcuts you need.",
} as const;

export const anatomy: AnatomyGroup[] = [
  {
    label: "Start",
    detail: "The Windows Start menu. Optional",
    side: "top",
    tiles: [{ kind: "start" }],
  },
  {
    label: "Pinned apps",
    detail: "Drag to reorder. A dot marks the open ones",
    side: "bottom",
    tiles: [
      { kind: "app", icon: fileExplorer, label: "File Explorer", open: true },
      { kind: "app", icon: microsoftStore, label: "Microsoft Store" },
      { kind: "app", icon: windowsTerminal, label: "Terminal", open: true },
      { kind: "app", icon: settings, label: "Settings" },
    ],
  },
  {
    label: "Recent apps",
    detail: "The last three you used",
    side: "top",
    tiles: [
      { kind: "app", icon: microsoftPhotos, label: "Photos", open: true },
      { kind: "app", icon: windowsMediaPlayer, label: "Media Player" },
      { kind: "app", icon: microsoftTeams, label: "Teams" },
    ],
  },
  {
    label: "Downloads",
    detail: "Drop files in. Right-click to hide",
    side: "bottom",
    divider: true,
    tiles: [{ kind: "app", icon: downloads, label: "Downloads" }],
  },
  {
    label: "Minimized",
    detail: "Optional live thumbnails of minimized windows",
    side: "top",
    tiles: [{ kind: "window" }],
  },
  {
    label: "Recycle Bin",
    detail: "Drop files to recycle. Optional",
    side: "bottom",
    tiles: [{ kind: "app", icon: recycleBin, label: "Recycle Bin" }],
  },
];

/* -------------------------------------------------------------- Customize */

export const customizeIntro = {
  eyebrow: "Settings",
  title: "Make it yours.",
  lead: "Adjust icon size, spacing and animation speed. Choose which shortcuts stay in your Dock, and try the changes in the preview.",
} as const;

/** The preview dock in the Customize scene. */
export const previewDockItems: {
  icon: ImageMetadata;
  fixed?: "pinExplorer" | "showDownloads" | "showTrash";
}[] = [
  { icon: fileExplorer, fixed: "pinExplorer" },
  { icon: microsoftStore },
  { icon: windowsTerminal },
  { icon: microsoftPhotos },
  { icon: settings },
  { icon: downloads, fixed: "showDownloads" },
  { icon: recycleBin, fixed: "showTrash" },
];

/** Shared decorative docks keep their existing icon list. */
export const previewIcons: ImageMetadata[] = previewDockItems.map(({ icon }) => icon);

/** Same defaults as the app (`crates/muelle-config`). */
export const settingsDefaults = {
  iconSize: 48,
  iconSizeMin: 32,
  iconSizeMax: 96,
  magnification: true,
  magnifiedSize: 77,
  magnifiedSizeMax: 160,
  autoHide: false,
  dockDensity: "normal",
  dockAutoHideSpeed: "normal",
  showStart: true,
  showDownloads: true,
  showTrash: true,
  pinExplorer: true,
  menuBar: true,
  menuBarAutoHide: false,
  aiUsage: true,
  accessoryBattery: false,
  genie: true,
  showMinimized: false,
  autostart: true,
  mascot: true,
  autoUpdate: true,
} as const;

/** The Settings app's own strings (`apps/settings/src/i18n`), in both languages. */
export const settingsText = {
  en: {
    title: "Muelle Settings",
    dock: "Dock",
    windows: "Windows",
    menubar: "Menu bar",
    system: "System",
    size: "Size",
    dockDensity: "Spacing",
    compact: "Compact",
    normal: "Normal",
    spacious: "Spacious",
    dockAutoHideSpeed: "Animation speed",
    smooth: "Smooth",
    fast: "Fast",
    fixedItems: "Dock shortcuts",
    fixedItemsHint:
      "You can also right-click Start, Downloads or Recycle Bin in Muelle and choose Remove from Dock.",
    showStart: "Show Start",
    showDownloads: "Show Downloads",
    showTrash: "Show Recycle Bin",
    pinExplorer: "Keep File Explorer in the Dock",
    pinExplorerHint: "When off, it still appears while its windows are open.",
    small: "Small",
    large: "Large",
    magnification: "Magnification",
    magnificationHint: "Icons grow as the pointer passes over them.",
    magnifiedSize: "Magnified size",
    autoHide: "Automatically hide and show the Dock",
    autoHideHint: "It reappears when you touch the bottom edge of the screen.",
    menuBar: "Show the menu bar",
    menuBarHint: "Time, battery, and access to the Windows quick settings and notifications.",
    menuBarAutoHide: "Automatically hide and show the menu bar",
    menuBarAutoHideHint: "It reappears when you touch the top edge of the screen.",
    aiUsage: "Show AI provider usage",
    aiUsageHint:
      "While a supported AI client is open, an island in the menu bar shows its available plan limits.",
    accessoryBattery: "Show accessory battery",
    accessoryBatteryHint:
      "Battery levels for supported mice and headsets. While off, no accessories are monitored or queried.",
    accessoryPreviewHint:
      "Turn this on to preview a connected mouse and headset. The values are examples; device support varies.",
    genie: "Genie effect",
    genieHint:
      "While it is on, windows do not use the Windows animations for maximizing or closing.",
    showMinimized: "Show minimized windows",
    showMinimizedHint:
      "As thumbnails on the right side of the Dock, instead of inside their app's icon.",
    autostart: "Open Muelle at login",
    mascot: "Crab on special moments",
    mascotHint:
      "A pixel crab comes out when an AI plan resets or runs out, when you plug in the charger, or when energy saver starts. Hover over it.",
    autoUpdate: "Install updates automatically",
    autoUpdateHint:
      "Once a day, a couple of minutes after you turn on the computer (or every 24 hours if it stays on), Muelle installs new versions by itself.",
    language: "Language",
    languageAuto: "Automatic",
    updates: "Updates",
    updatesCurrent: `Muelle ${site.version} is up to date.`,
    updatesChecking: "Checking for updates…",
    updatesCheck: "Check",
  },
  es: {
    title: "Ajustes de Muelle",
    dock: "Dock",
    windows: "Ventanas",
    menubar: "Barra superior",
    system: "Sistema",
    size: "Tamaño",
    dockDensity: "Espaciado",
    compact: "Compacto",
    normal: "Normal",
    spacious: "Amplio",
    dockAutoHideSpeed: "Velocidad de la animación",
    smooth: "Suave",
    fast: "Rápida",
    fixedItems: "Accesos del Dock",
    fixedItemsHint:
      "También puedes hacer clic derecho en Inicio, Descargas o Papelera en Muelle y elegir Quitar del Dock.",
    showStart: "Mostrar Inicio",
    showDownloads: "Mostrar Descargas",
    showTrash: "Mostrar Papelera",
    pinExplorer: "Mantener Explorador en el Dock",
    pinExplorerHint: "Desactivado, sigue apareciendo mientras tenga ventanas abiertas.",
    small: "Pequeño",
    large: "Grande",
    magnification: "Ampliación",
    magnificationHint: "Los iconos crecen al pasar el puntero sobre ellos.",
    magnifiedSize: "Tamaño ampliado",
    autoHide: "Ocultar y mostrar el Dock automáticamente",
    autoHideHint: "Reaparece al tocar el borde inferior de la pantalla.",
    menuBar: "Mostrar la barra superior",
    menuBarHint: "Hora, batería y acceso a los ajustes rápidos y las notificaciones de Windows.",
    menuBarAutoHide: "Ocultar y mostrar la barra superior automáticamente",
    menuBarAutoHideHint: "Reaparece al tocar el borde superior de la pantalla.",
    aiUsage: "Mostrar uso de proveedores de IA",
    aiUsageHint:
      "Mientras un cliente de IA compatible está abierto, una isla en la barra superior muestra sus límites disponibles.",
    accessoryBattery: "Mostrar batería de accesorios",
    accessoryBatteryHint:
      "Batería de ratones y auriculares compatibles. Desactivado, no detecta ni consulta accesorios.",
    accessoryPreviewHint:
      "Actívalo para ver un ratón y unos auriculares conectados. Los valores son ejemplos; la compatibilidad depende del dispositivo.",
    genie: "Efecto genio",
    genieHint:
      "Mientras está activo, las ventanas no usan las animaciones de Windows al maximizar ni al cerrar.",
    showMinimized: "Mostrar ventanas minimizadas",
    showMinimizedHint:
      "Como miniaturas a la derecha del Dock, en lugar de dentro del icono de su app.",
    autostart: "Abrir Muelle al iniciar sesión",
    mascot: "Cangrejo en momentos especiales",
    mascotHint:
      "Un cangrejo pixelado aparece cuando un plan de IA se reinicia o se agota, al enchufar el cargador o al activarse el ahorro de energía. Pasa el puntero por encima.",
    autoUpdate: "Instalar actualizaciones automáticamente",
    autoUpdateHint:
      "Una vez al día, un par de minutos después de encender el ordenador (o cada 24 horas si sigue encendido), Muelle instala las versiones nuevas por sí solo.",
    language: "Idioma",
    languageAuto: "Automático",
    updates: "Actualizaciones",
    updatesCurrent: `Muelle ${site.version} está al día.`,
    updatesChecking: "Buscando actualizaciones…",
    updatesCheck: "Buscar",
  },
} as const;

export type SettingsKey = keyof (typeof settingsText)["en"];

/* ----------------------------------------------------------------- Safety */

export const safetyIntro = {
  eyebrow: "Reversible",
  title: "Your taskbar is never gone. Just tucked away.",
  lead: "Muelle hides the Windows taskbar while it runs and gives it back the moment it stops. Flip the switch and see.",
} as const;

/** The taskbar shown when Muelle is off. */
export const taskbarIcons: ImageMetadata[] = [
  fileExplorer,
  microsoftStore,
  windowsTerminal,
  microsoftPhotos,
  settings,
];

export const safetyFacts = [
  {
    title: "Quit any time",
    body: "Right-click the dock and choose Quit Muelle. The taskbar is back at once.",
  },
  {
    title: "Even if it stops",
    body: "A tiny guardian process watches the dock and restores the taskbar if it ever stops.",
  },
  {
    title: "Uninstall cleanly",
    body: "Windows Settings → Apps → Muelle. Windows is left exactly as it was.",
  },
] as const;

/* --------------------------------------------------------------- Download */

export const download = {
  name: site.name,
  tagline: site.tagline,
  meta: [
    `Version ${site.version}`,
    "Windows 10 & 11 · 64-bit",
    `${figures.installer.value} ${figures.installer.unit}`,
    `Free · ${site.license}`,
  ],
  cta: "Download for Windows",
  releases: "All versions on GitHub",
  steps: [
    { title: "Open the installer", body: "Per user. No admin rights, no wizard." },
    {
      title: "If SmartScreen asks",
      body: "Muelle is new, so Windows may not know it yet: More info → Run anyway.",
    },
    { title: "That's it", body: "The dock appears and starts with Windows from now on." },
  ],
  source: {
    summary: "Build it from source",
    note: "Needs Windows 10 (1903+) or 11, Rust (MSVC) and the Visual Studio C++ build tools.",
    commands: [`git clone ${site.repository}`, "cd Muelle", "cargo run --release -p muelle"],
  },
} as const;
