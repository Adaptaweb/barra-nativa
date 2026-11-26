/**
 * Small coloured tiles with a white glyph, one per widget or service, in the
 * style of the sidebar icons. Used by the widget lists, the add gallery and
 * the Configuraciones rows.
 */

import type { ReactNode } from "react";

const GRAY = "#8e8e93";

const TILES: Record<string, { bg: string; glyph: ReactNode }> = {
  reloj: { bg: "#ff3b30", glyph: <><circle cx="12" cy="12" r="6.5" /><path d="M12 8.5V12l2.5 1.5" /></> },
  clima: { bg: "#0a84ff", glyph: <><circle cx="9" cy="9" r="2.6" /><path d="M9 3.6v1M3.6 9h1M5.2 5.2l.7.7M12.8 5.2l-.7.7" /><path d="M8 18.5h8.5a3 3 0 0 0 0-6 4 4 0 0 0-7.6 1A2.6 2.6 0 0 0 8 18.5z" /></> },
  sistema: { bg: GRAY, glyph: <><rect x="7" y="7" width="10" height="10" rx="1.5" /><path d="M10 4.5v2.5M14 4.5v2.5M10 17v2.5M14 17v2.5M4.5 10H7M4.5 14H7M17 10h2.5M17 14h2.5" /></> },
  bateria: { bg: "#34c759", glyph: <><rect x="4" y="8" width="14" height="8" rx="2" /><path d="M20 11v2M8 10.5l-1.5 2.5h3L8 15.5" /></> },
  memoria: { bg: "#5e5ce6", glyph: <><rect x="4.5" y="8" width="15" height="8" rx="1.5" /><path d="M8 11v2M11 11v2M14 11v2M17 11v2M7 16v2.5M12 16v2.5M17 16v2.5" /></> },
  casa: { bg: "#ff9500", glyph: <path d="M5 11.5 12 5l7 6.5V19H5zM10 19v-4.5h4V19" /> },
  claude: { bg: "#d97757", glyph: <path d="m12 4.5 1.7 5.8 5.8 1.7-5.8 1.7L12 19.5l-1.7-5.8L4.5 12l5.8-1.7z" /> },
  chatgpt: { bg: "#10a37f", glyph: <><path d="M12 4.5 18.5 8.2v7.6L12 19.5l-6.5-3.7V8.2z" /><circle cx="12" cy="12" r="2.2" /></> },
  gemini: { bg: "linear-gradient(135deg, #4285f4, #9b72f3)", glyph: <path d="M12 4.5c.6 4 3 6.9 7.5 7.5-4.5.6-6.9 3.5-7.5 7.5-.6-4-3-6.9-7.5-7.5 4.5-.6 6.9-3.5 7.5-7.5z" /> },
  control: { bg: GRAY, glyph: <><path d="M5 8.5h14M5 15.5h14" /><circle cx="9" cy="8.5" r="2" fill="#fff" /><circle cx="15" cy="15.5" r="2" fill="#fff" /></> },
  notificaciones: { bg: "#ff3b30", glyph: <path d="M7 16.5V11a5 5 0 0 1 10 0v5.5l1.5 1.5h-13zM10.5 19.5a1.6 1.6 0 0 0 3 0" /> },
  musica: { bg: "#ff2d55", glyph: <><path d="M10 16.5V6.5l7-1.5v10" /><circle cx="8" cy="16.5" r="2" /><circle cx="15" cy="15" r="2" /></> },
  audio: { bg: GRAY, glyph: <><rect x="9.5" y="4.5" width="5" height="9" rx="2.5" /><path d="M6.5 11.5a5.5 5.5 0 0 0 11 0M12 17v2.5" /></> },
  bandeja: { bg: GRAY, glyph: <><path d="m8 14 4-4 4 4" /><path d="M5 18.5h14" /></> },
  // Services of Configuraciones that are not widgets.
  calendario: { bg: "#ff3b30", glyph: <><rect x="5" y="6" width="14" height="13" rx="2" /><path d="M5 10.5h14M9 4.5v3M15 4.5v3" /></> },
};

const FALLBACK = { bg: GRAY, glyph: <circle cx="12" cy="12" r="4" /> };

export function Tile({ id, size = 26 }: { id: string; size?: number }) {
  const { bg, glyph } = TILES[id] ?? FALLBACK;
  return (
    <span className="wtile" style={{ background: bg, width: size, height: size }} aria-hidden="true">
      <svg viewBox="0 0 24 24" width={size * 0.72} height={size * 0.72} fill="none" stroke="#fff" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
        {glyph}
      </svg>
    </span>
  );
}
