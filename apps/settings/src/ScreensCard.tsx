/**
 * Dock tab, "Pantallas": which displays get a dock and how big it is on each.
 * Shown only when there is more than one display.
 */

import { useEffect, useState } from "react";
import { type Config, type DisplayInfo, type ScreenWidgets, ICON_SIZE, listDisplays } from "./config";
import { Card, Row, Segmented, Slider, SubRow, Toggle } from "./controls";
import { DEFAULT_SCREEN } from "./WidgetsSection";

const DesktopIcon = () => (
  <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <rect x="3" y="4" width="18" height="12" rx="2" />
    <path d="M8 20h8M12 16v4" />
  </svg>
);

export function ScreensCard({ config, update }: { config: Config; update: (patch: Partial<Config>) => void }) {
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);
  useEffect(() => {
    listDisplays().then(setDisplays).catch(() => setDisplays([]));
  }, []);
  const widgets = config.widgets;
  // A settings file from before widgets existed has none yet.
  if (!widgets || displays.length < 2) return null;

  const saveScreen = (device: string, patch: Partial<ScreenWidgets>) =>
    update({
      widgets: {
        ...widgets,
        screens: {
          ...(widgets.screens ?? {}),
          [device]: { ...DEFAULT_SCREEN, ...(widgets.screens?.[device] ?? {}), ...patch },
        },
      },
    });

  return (
    <Card title="Pantallas">
      {displays.map((d) => {
        if (d.primary) {
          return (
            <Row key={d.device} icon={<DesktopIcon />} label={d.label} hint="Pantalla principal: siempre tiene dock y barra superior.">
              <span className="row-value">{config.iconSize} px</span>
            </Row>
          );
        }
        const enabled = widgets.screens?.[d.device]?.enabled ?? true;
        const own = widgets.screens?.[d.device]?.iconSize ?? null;
        return (
          <div key={d.device}>
            <Row
              icon={<DesktopIcon />}
              label={d.label}
              hint="Dock y barra superior propios, con sus widgets y solo las apps de esta pantalla."
            >
              <Toggle label={`Dock en ${d.label}`} checked={enabled} onChange={(on) => saveScreen(d.device, { enabled: on })} />
            </Row>
            <SubRow
              label="Tamaño propio"
              hint={own === null ? `Usa el tamaño del dock principal (${config.iconSize} px).` : "Solo el dock de esta pantalla."}
              open={enabled}
            >
              <Segmented<"same" | "own">
                label={`Tamaño propio en ${d.label}`}
                value={own === null ? "same" : "own"}
                options={[
                  { value: "same", label: "Igual que la principal" },
                  { value: "own", label: "Propio" },
                ]}
                onChange={(mode) => saveScreen(d.device, { iconSize: mode === "own" ? config.iconSize : null })}
              />
            </SubRow>
            <SubRow label="Tamaño del dock" open={enabled && own !== null}>
              <Slider
                label={`Tamaño del dock en ${d.label}`}
                value={own ?? config.iconSize}
                min={ICON_SIZE.min}
                max={ICON_SIZE.max}
                onChange={(iconSize) => saveScreen(d.device, { iconSize })}
              />
            </SubRow>
          </div>
        );
      })}
    </Card>
  );
}
