/**
 * The pieces of the widget editor: the catalogue, the zone map, the ordered
 * list of one zone (drag to reorder) and the gallery to add a widget.
 */

import { type ReactNode, useEffect, useRef, useState } from "react";
import { Card, Segmented, Select } from "./controls";
import { Tile } from "./tiles";
import { type PreviewSize, WidgetPreview } from "./WidgetPreview";

export type Zone = "barLeft" | "barRight" | "islandLeft" | "dockLeft" | "dock" | "islandRight";

export const WIDGETS: { key: string; name: string; detail: string }[] = [
  { key: "reloj", name: "Reloj y calendario", detail: "Fecha, hora y tu próximo evento. Reemplaza el reloj de la barra." },
  { key: "clima", name: "Clima", detail: "Temperatura y cielo de tu ciudad." },
  { key: "sistema", name: "Sistema", detail: "Temperatura de la CPU, uso de CPU y memoria." },
  { key: "casa", name: "Casa", detail: "Tus botones de Home Assistant." },
  { key: "claude", name: "Claude", detail: "Límites de 5 horas y semanal de tu plan." },
  { key: "chatgpt", name: "ChatGPT", detail: "Límites de tu plan, a través de Codex." },
  { key: "gemini", name: "Gemini", detail: "Cuota de Gemini CLI o Antigravity." },
  { key: "control", name: "Centro de control", detail: "Red, volumen y batería; abre la configuración rápida de Windows." },
  { key: "notificaciones", name: "Notificaciones", detail: "Un botón de campana; abre el centro de notificaciones de Windows." },
  { key: "musica", name: "Sonando ahora", detail: "La canción que suena, con su carátula, el progreso y los botones de anterior, pausa y siguiente." },
  { key: "audio", name: "Micrófono y salida", detail: "Dos botones: silencia el micrófono con un clic y elige la salida y la entrada de audio predeterminadas." },
  { key: "bandeja", name: "Segundo plano", detail: "Los iconos ocultos de la bandeja de Windows (solo en el dock)." },
];

export const ZONES: { zone: Zone; title: string; hint: string }[] = [
  { zone: "barLeft", title: "Barra superior · izquierda", hint: "Después del nombre de la app activa." },
  { zone: "barRight", title: "Barra superior · derecha", hint: "Antes de los iconos de estado." },
  { zone: "islandLeft", title: "Isla izquierda", hint: "Una burbuja aparte abajo a la izquierda; el dock queda solo con tus apps." },
  { zone: "dockLeft", title: "Dentro del dock · izquierda", hint: "Antes de tus apps; las apps quedan centradas." },
  { zone: "dock", title: "Dentro del dock · derecha", hint: "Al final del dock, después de las carpetas." },
  { zone: "islandRight", title: "Isla derecha", hint: "Una burbuja aparte abajo a la derecha." },
];

/** Short names, as the zone map and the add button show them. */
export const ZONE_LABEL: Record<Zone, string> = {
  barLeft: "Izquierda",
  barRight: "Derecha",
  islandLeft: "Isla izq.",
  dockLeft: "Dock izq.",
  dock: "Dock der.",
  islandRight: "Isla der.",
};

export const nameOf = (key: string) => WIDGETS.find((w) => w.key === key)?.name ?? key;

export const isBarZone = (zone: Zone) => zone === "barLeft" || zone === "barRight";

/** A box of the map: a clickable zone, or a dashed placeholder for what is not editable. */
export type MapBox = { zone: Zone } | { note: string };

export function ZoneMap({ boxes, counts, selected, onSelect }: {
  boxes: MapBox[];
  counts: Partial<Record<Zone, number>>;
  selected: Zone;
  onSelect: (zone: Zone) => void;
}) {
  return (
    <div className="zone-map" role="group" aria-label="Zonas de widgets">
      {boxes.map((box) =>
        "zone" in box ? (
          <button
            key={box.zone}
            type="button"
            className="zone-box"
            aria-pressed={selected === box.zone}
            onClick={() => onSelect(box.zone)}
          >
            {ZONE_LABEL[box.zone]} · {counts[box.zone] ?? 0}
          </button>
        ) : (
          <div key={box.note} className="zone-box zone-box-fixed">{box.note}</div>
        ),
      )}
    </div>
  );
}

function sizeOptions(key: string) {
  const base = [
    { value: "compacto", label: "Compacto" },
    { value: "mediano", label: "Mediano" },
    { value: "ancho", label: "Ancho" },
  ];
  return key === "clima" ? [...base, { value: "estado", label: "Tarjeta" }] : base;
}

/** The saved size as a preview size; "estado" or empty show as "mediano". */
const previewSize = (size?: string): PreviewSize =>
  size === "compacto" || size === "mediano" || size === "ancho" ? size : "mediano";

/** A preview in a fixed-size box; it shrinks (never grows) to fit both width and height, without cropping. */
function FitPreview({ children }: { children: ReactNode }) {
  const boxRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const box = boxRef.current;
    const inner = innerRef.current;
    if (!box || !inner) return;
    const fit = () => {
      const cs = getComputedStyle(box);
      const w = box.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
      const h = box.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
      const width = inner.scrollWidth;
      const height = inner.scrollHeight;
      const scale = width > 0 && height > 0 ? Math.min(1, w / width, h / height) : 1;
      inner.style.transform = scale < 1 ? `scale(${scale})` : "";
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(box);
    observer.observe(inner);
    return () => observer.disconnect();
  }, []);
  return (
    <div ref={boxRef} className="zone-item-preview" aria-hidden="true">
      <div ref={innerRef} className="zone-item-fit">{children}</div>
    </div>
  );
}

/** The widgets of one zone, in order. Drag a row, or press Alt+Up / Alt+Down on it, to reorder. */
export function ZoneList({ zone, keys, sizes, showSize, onReorder, onRemove, onSize, children }: {
  zone: Zone;
  keys: string[];
  sizes?: Record<string, string>;
  showSize: boolean;
  onReorder: (from: number, to: number) => void;
  onRemove: (index: number) => void;
  onSize: (key: string, size: string) => void;
  /** The add row, below the list. */
  children?: ReactNode;
}) {
  const info = ZONES.find((z) => z.zone === zone)!;
  const [drag, setDrag] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const reset = () => {
    setDrag(null);
    setOver(null);
  };
  // Moving a node can drop its focus, so give it back after the repaint.
  const keepFocus = (key: string) =>
    setTimeout(() => listRef.current?.querySelector<HTMLElement>(`[data-key="${key}"]`)?.focus(), 0);
  return (
    <Card title={info.title}>
      <div className="row">
        <div className="row-text">
          <span className="row-hint">
            {info.hint} {keys.length === 0 ? "Sin widgets." : "Arrastra para ordenar (o Alt+↑ / Alt+↓ con la fila enfocada)."}
          </span>
        </div>
      </div>
      <div ref={listRef}>
        {keys.map((key, index) => (
          <div
            key={key}
            data-key={key}
            className={`row zone-item${showSize ? "" : " no-size"}${drag === index ? " dragging" : ""}${over === index && drag !== null && drag !== index ? " drop-target" : ""}`}
            tabIndex={0}
            draggable
            aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
            onDragStart={(e) => {
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData("text/plain", key);
              setDrag(index);
            }}
            onDragOver={(e) => {
              if (drag === null) return;
              e.preventDefault();
              setOver(index);
            }}
            onDrop={(e) => {
              e.preventDefault();
              if (drag !== null && drag !== index) onReorder(drag, index);
              reset();
            }}
            onDragEnd={reset}
            onKeyDown={(e) => {
              if (e.target !== e.currentTarget || !e.altKey) return;
              const by = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
              if (by === 0 || index + by < 0 || index + by >= keys.length) return;
              e.preventDefault();
              onReorder(index, index + by);
              keepFocus(key);
            }}
          >
            <span className="zone-grip" aria-hidden="true">⠿</span>
            <FitPreview>
              <WidgetPreview bare widget={key} place={isBarZone(zone) ? "bar" : "dock"} size={previewSize(sizes?.[key])} />
            </FitPreview>
            <span className="zone-item-name">{nameOf(key)}</span>
            {showSize ? (
              <Select
                label={`Tamaño de ${nameOf(key)}`}
                value={sizes?.[key] ?? "mediano"}
                options={sizeOptions(key)}
                onChange={(size) => onSize(key, size)}
              />
            ) : null}
            <button type="button" className="zone-remove" aria-label={`Quitar ${nameOf(key)}`} onClick={() => onRemove(index)}>×</button>
          </div>
        ))}
      </div>
      {children}
    </Card>
  );
}

/** "Añadir widget…": opens a gallery of the widgets not placed yet, with a preview of the chosen one. */
export function AddWidget({ zone, placed, onAdd }: { zone: Zone; placed: string[]; onAdd: (key: string) => void }) {
  const bar = isBarZone(zone);
  // The tray chevron lives only in the dock.
  const free = WIDGETS.filter((w) => !placed.includes(w.key) && !(bar && w.key === "bandeja"));
  const [open, setOpen] = useState(false);
  const [choice, setChoice] = useState("");
  const [size, setSize] = useState<PreviewSize>("mediano");
  if (free.length === 0) return null;
  const selected = free.find((w) => w.key === choice);
  return (
    <>
      <button type="button" className="add-row" aria-expanded={open} onClick={() => setOpen(!open)}>
        ＋ Añadir widget…
      </button>
      {open ? (
        <div className="add-panel">
          <div className="gallery" role="group" aria-label="Widgets disponibles">
            {free.map((w) => (
              <button
                key={w.key}
                type="button"
                className="gallery-tile"
                aria-pressed={choice === w.key}
                onClick={() => setChoice(w.key)}
              >
                <Tile id={w.key} size={30} />
                <span className="gallery-name">{w.name}</span>
                <span className="gallery-detail">{w.detail}</span>
              </button>
            ))}
          </div>
          {selected ? (
            <div className="widget-preview-row add-preview">
              <span className="row-hint">
                Así se ve {selected.name} {bar ? "en la barra superior" : "en el dock"}, con datos de ejemplo.
              </span>
              <div className="widget-preview-control">
                <WidgetPreview widget={selected.key} place={bar ? "bar" : "dock"} size={size} />
                {bar ? null : (
                  <Segmented<PreviewSize>
                    label="Tamaño de la vista previa"
                    value={size}
                    options={[
                      { value: "compacto", label: "Compacto" },
                      { value: "mediano", label: "Mediano" },
                      { value: "ancho", label: "Ancho" },
                    ]}
                    onChange={setSize}
                  />
                )}
              </div>
              <button
                type="button"
                className="push push-primary"
                onClick={() => {
                  onAdd(selected.key);
                  setChoice("");
                  setOpen(false);
                }}
              >
                Añadir a {ZONE_LABEL[zone]}
              </button>
            </div>
          ) : null}
        </div>
      ) : null}
    </>
  );
}
