import { useEffect, useState } from "react";
import { Card } from "./controls";
import {
  type Config,
  type CustomWallpaper,
  addCustomWallpaper,
  applyCustomPicture,
  listCustomWallpapers,
  removeCustomWallpaper,
} from "./config";

interface Props {
  config: Config;
  update: (patch: Partial<Config>) => void;
  onApplied: (preview: string) => void;
}

/** The user's own pictures and MP4 videos. A picture becomes the Windows
 *  wallpaper; a video plays behind the desktop icons on every display. */
export function CustomWallpapers({ config, update, onApplied }: Props) {
  const [items, setItems] = useState<CustomWallpaper[]>([]);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const [picture, setPicture] = useState<string | null>(null);
  // The tile the user picked; "Guardar fondo" applies it.
  const [chosen, setChosen] = useState<string | null>(null);

  useEffect(() => {
    listCustomWallpapers().then(setItems).catch(() => setItems([]));
  }, []);

  const apply = async (item: CustomWallpaper) => {
    setMessage(null);
    if (item.video) {
      update({ videoWallpaper: item.path });
      setMessage({ text: "Video aplicado. Se reproduce detrás de los iconos en todas las pantallas.", error: false });
      return;
    }
    setBusy(true);
    try {
      await applyCustomPicture(item.name);
      if (config.videoWallpaper) update({ videoWallpaper: null });
      setPicture(item.name);
      if (item.preview) onApplied(item.preview);
      setMessage({ text: "Fondo aplicado.", error: false });
    } catch {
      setMessage({ text: "No se pudo cambiar el fondo.", error: true });
    } finally {
      setBusy(false);
    }
  };

  const add = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const added = await addCustomWallpaper();
      if (added) {
        setItems((list) => [added, ...list]);
        setChosen(added.name);
        setMessage({ text: "Agregado. Presiona \"Guardar fondo\" para usarlo.", error: false });
      }
    } catch (error) {
      setMessage({ text: typeof error === "string" ? error : "No se pudo agregar el fondo.", error: true });
    } finally {
      setBusy(false);
    }
  };

  const remove = async (item: CustomWallpaper) => {
    try {
      if (config.videoWallpaper === item.path) update({ videoWallpaper: null });
      if (chosen === item.name) setChosen(null);
      await removeCustomWallpaper(item.name);
      setItems((list) => list.filter((i) => i.name !== item.name));
    } catch {
      setMessage({ text: "No se pudo quitar el fondo.", error: true });
    }
  };

  const active = (item: CustomWallpaper) =>
    item.video ? config.videoWallpaper === item.path : !config.videoWallpaper && picture === item.name;
  const selected = items.find((item) => item.name === chosen) ?? null;
  const save = async () => {
    if (!selected) return;
    await apply(selected);
    setChosen(null);
  };

  return (
    <Card title="Tus fondos">
      <p className="wallpaper-hint">
        Agrega tus imágenes o videos MP4, elige uno y presiona "Guardar fondo". Un video se reproduce en movimiento detrás de los iconos, en todas las pantallas, sin sonido.
      </p>
      <div className="custom-actions">
        <button type="button" className="push" disabled={busy} onClick={add}>
          Agregar imagen o video…
        </button>
        <button
          type="button"
          className="push push-primary"
          disabled={busy || !selected || active(selected)}
          onClick={save}
        >
          Guardar fondo
        </button>
        {config.videoWallpaper ? (
          <button type="button" className="push" onClick={() => update({ videoWallpaper: null })}>
            Quitar video de fondo
          </button>
        ) : null}
      </div>
      {items.length > 0 ? (
        <div className="custom-grid">
          {items.map((item) => (
            <div className="custom-item" key={item.name}>
              <button
                type="button"
                className="wallpaper-choice"
                title={item.name}
                aria-label={`Elegir ${item.name}`}
                aria-pressed={chosen === item.name || (chosen === null && active(item))}
                data-chosen={chosen === item.name || undefined}
                disabled={busy}
                onClick={() => {
                  setChosen(item.name);
                  setMessage(active(item) ? { text: "Este es tu fondo actual.", error: false } : null);
                }}
              >
                <span className="wallpaper-image">
                  {item.preview ? <img src={item.preview} alt="" /> : <span className="custom-placeholder">{item.video ? "Video" : "Imagen"}</span>}
                  {item.video ? <span className="custom-badge" aria-hidden="true">▶ Video</span> : null}
                  {active(item) ? (
                    <span className="wallpaper-check" aria-hidden="true">
                      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2"><path d="m4 8 2.5 2.5L12 5" /></svg>
                    </span>
                  ) : null}
                  <span className="wallpaper-name" aria-hidden="true">{item.name}</span>
                </span>
              </button>
              <button type="button" className="custom-remove" aria-label={`Quitar ${item.name}`} onClick={() => remove(item)}>
                ×
              </button>
            </div>
          ))}
        </div>
      ) : null}
      <p className="wallpaper-status" role={message?.error ? "alert" : "status"}>{message?.text ?? ""}</p>
    </Card>
  );
}
