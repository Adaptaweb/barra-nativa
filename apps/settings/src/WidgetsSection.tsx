/**
 * Barra widgets: where each one goes (left or right of the menu bar, or the
 * dock), in which order, and what each needs (the weather city, the calendar
 * link, Home Assistant). Secrets go to the Windows Credential Manager and
 * never come back to this page; only whether one is saved.
 */

import { useEffect, useState } from "react";
import {
  type Config,
  type HomeItem,
  type WidgetSettings,
  importHome,
  secretSaved,
  setSecret,
} from "./config";
import { Card, Row } from "./controls";

type Zone = "barLeft" | "barRight" | "dock";

const WIDGETS: { key: string; name: string; detail: string }[] = [
  { key: "reloj", name: "Reloj y calendario", detail: "Fecha, hora y tu próximo evento. Reemplaza el reloj de la barra." },
  { key: "clima", name: "Clima", detail: "Temperatura y cielo de tu ciudad." },
  { key: "sistema", name: "Sistema", detail: "Temperatura de la CPU, uso de CPU y memoria." },
  { key: "casa", name: "Casa", detail: "Tus botones de Home Assistant." },
  { key: "claude", name: "Claude", detail: "Límites de 5 horas y semanal de tu plan." },
  { key: "chatgpt", name: "ChatGPT", detail: "Límites de tu plan, a través de Codex." },
  { key: "gemini", name: "Gemini", detail: "Cuota de Gemini CLI o Antigravity." },
];

const ZONES: { zone: Zone; title: string; hint: string }[] = [
  { zone: "barLeft", title: "Barra superior · izquierda", hint: "Después del nombre de la app activa." },
  { zone: "barRight", title: "Barra superior · derecha", hint: "Antes de los iconos de estado." },
  { zone: "dock", title: "Dock", hint: "Entre tus apps y las carpetas." },
];

const nameOf = (key: string) => WIDGETS.find((w) => w.key === key)?.name ?? key;

interface Props {
  config: Config;
  update: (patch: Partial<Config>) => void;
}

export function WidgetsSection({ config, update }: Props) {
  const widgets = config.widgets;
  const save = (patch: Partial<WidgetSettings>) => update({ widgets: { ...widgets, ...patch } });

  const move = (zone: Zone, index: number, by: number) => {
    const list = [...widgets[zone]];
    const target = index + by;
    if (target < 0 || target >= list.length) return;
    [list[index], list[target]] = [list[target], list[index]];
    save({ [zone]: list });
  };
  const remove = (zone: Zone, index: number) =>
    save({ [zone]: widgets[zone].filter((_, i) => i !== index) });
  const add = (zone: Zone, key: string) => {
    if (!key || widgets[zone].includes(key)) return;
    save({ [zone]: [...widgets[zone], key] });
  };

  return (
    <>
      {ZONES.map(({ zone, title, hint }) => (
        <Card key={zone} title={title}>
          {widgets[zone].length === 0 ? (
            <div className="row"><div className="row-text"><span className="row-hint">{hint} Sin widgets.</span></div></div>
          ) : (
            widgets[zone].map((key, index) => (
              <div className="row" key={key}>
                <div className="row-text">
                  <span className="row-label">{nameOf(key)}</span>
                  {index === 0 ? <span className="row-hint">{hint}</span> : null}
                </div>
                <div className="row-control push-group">
                  {zone === "dock" ? (
                    <span className="select-wrap">
                      <select
                        className="select-control size-control"
                        aria-label={`Tamaño de ${nameOf(key)}`}
                        value={widgets.dockSizes?.[key] ?? "mediano"}
                        onChange={(e) => save({ dockSizes: { ...widgets.dockSizes, [key]: e.target.value } })}
                      >
                        <option value="compacto">Compacto</option>
                        <option value="mediano">Mediano</option>
                        <option value="ancho">Ancho</option>
                      </select>
                    </span>
                  ) : null}
                  <button type="button" className="push" aria-label="Subir" disabled={index === 0} onClick={() => move(zone, index, -1)}>↑</button>
                  <button type="button" className="push" aria-label="Bajar" disabled={index === widgets[zone].length - 1} onClick={() => move(zone, index, 1)}>↓</button>
                  <button type="button" className="push" aria-label={`Quitar ${nameOf(key)}`} onClick={() => remove(zone, index)}>Quitar</button>
                </div>
              </div>
            ))
          )}
          <AddRow zone={zone} placed={widgets[zone]} onAdd={(key) => add(zone, key)} />
        </Card>
      ))}

      <Card title="Clima">
        <Row label="Ciudad" hint="El nombre se busca en Open-Meteo; no hace falta cuenta.">
          <TextField
            value={widgets.weatherCity}
            placeholder="Santiago"
            onCommit={(weatherCity) => save({ weatherCity })}
          />
        </Row>
      </Card>

      <CalendarCard />
      <HomeCard widgets={widgets} save={save} />

      <Card title="Límites de IA">
        <div className="row">
          <div className="row-text">
            <span className="row-hint">
              Claude usa la sesión de Claude Code de este PC (abre «claude» en una terminal si caducó).
              ChatGPT usa Codex CLI («npm install -g @openai/codex» y «codex login»). Gemini usa Gemini CLI
              o Antigravity abierto. Nada se envía a Barra; cada widget explica los pasos en su panel.
            </span>
          </div>
        </div>
      </Card>
    </>
  );
}

function AddRow({ zone, placed, onAdd }: { zone: Zone; placed: string[]; onAdd: (key: string) => void }) {
  const free = WIDGETS.filter((w) => !placed.includes(w.key));
  const [choice, setChoice] = useState("");
  if (free.length === 0) return null;
  const selected = free.find((w) => w.key === choice);
  return (
    <div className="row">
      <div className="row-text">
        <span className="row-label">Añadir widget</span>
        {selected ? <span className="row-hint">{selected.detail}</span> : null}
      </div>
      <div className="row-control push-group">
        <span className="select-wrap">
          <select
            className="select-control"
            aria-label={`Widget para ${zone}`}
            value={choice}
            onChange={(e) => setChoice(e.target.value)}
          >
            <option value="">Elegir…</option>
            {free.map((w) => (
              <option key={w.key} value={w.key}>{w.name}</option>
            ))}
          </select>
        </span>
        <button
          type="button"
          className="push push-primary"
          disabled={!choice}
          onClick={() => {
            onAdd(choice);
            setChoice("");
          }}
        >
          Añadir
        </button>
      </div>
    </div>
  );
}

function TextField({
  value,
  placeholder,
  secret,
  onCommit,
}: {
  value: string;
  placeholder?: string;
  secret?: boolean;
  onCommit: (value: string) => void;
}) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <input
      className="text-field"
      type={secret ? "password" : "text"}
      value={text}
      placeholder={placeholder}
      spellCheck={false}
      autoComplete="off"
      onChange={(e) => setText(e.target.value)}
      onBlur={() => text !== value && onCommit(text.trim())}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
      }}
    />
  );
}

/** A secret field: shows whether one is saved, takes a new one, or removes it. */
function SecretRow({ label, hint, secretKey, placeholder, multiline }: {
  label: string;
  hint: string;
  secretKey: string;
  placeholder: string;
  multiline?: boolean;
}) {
  const [saved, setSaved] = useState<boolean | null>(null);
  const [text, setText] = useState("");
  const [message, setMessage] = useState("");
  useEffect(() => {
    secretSaved(secretKey).then(setSaved).catch(() => setSaved(false));
  }, [secretKey]);
  const store = (value: string) => {
    setSecret(secretKey, value)
      .then(() => {
        setSaved(value.trim() !== "");
        setText("");
        setMessage(value.trim() ? "Guardado en el Administrador de credenciales de Windows." : "Quitado.");
      })
      .catch((error) => setMessage(String(error)));
  };
  return (
    <div className="row row-stacked">
      <div className="row-text">
        <span className="row-label">
          {label} {saved === null ? "" : saved ? "· guardado" : "· sin configurar"}
        </span>
        <span className="row-hint">{message || hint}</span>
      </div>
      <div className="row-control secret-control">
        {multiline ? (
          <textarea
            className="text-field"
            rows={2}
            value={text}
            placeholder={saved ? "Pega uno nuevo para reemplazarlo" : placeholder}
            spellCheck={false}
            onChange={(e) => setText(e.target.value)}
          />
        ) : (
          <input
            className="text-field"
            type="password"
            value={text}
            placeholder={saved ? "Pega uno nuevo para reemplazarlo" : placeholder}
            autoComplete="off"
            onChange={(e) => setText(e.target.value)}
          />
        )}
        <div className="push-group">
          <button type="button" className="push push-primary" disabled={!text.trim()} onClick={() => store(text)}>
            Guardar
          </button>
          {saved ? (
            <button type="button" className="push" onClick={() => store("")}>Quitar</button>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function CalendarCard() {
  return (
    <Card title="Calendario">
      <div className="row">
        <div className="row-text">
          <span className="row-hint">
            En Google Calendar: Configuración › tu calendario › «Integrar el calendario» › copia la
            «Dirección secreta en formato iCal». En Outlook: «Publicar calendario» › enlace ICS. Puedes poner
            varios, uno por línea.
          </span>
        </div>
      </div>
      <SecretRow
        label="Enlaces iCal"
        hint="Son privados: quien los tenga ve tus eventos. Se guardan solo en este PC."
        secretKey="calendario.ics"
        placeholder="https://calendar.google.com/calendar/ical/…/basic.ics"
        multiline
      />
    </Card>
  );
}

function HomeCard({ widgets, save }: { widgets: WidgetSettings; save: (patch: Partial<WidgetSettings>) => void }) {
  const [message, setMessage] = useState("");
  const importNow = () => {
    importHome()
      .then((found) => {
        save({ homeUrl: found.url, homeItems: found.items });
        setMessage(`Importados ${found.items.length} botones desde ${found.source}.`);
      })
      .catch((error) => setMessage(String(error)));
  };
  const removeItem = (item: HomeItem) =>
    save({ homeItems: widgets.homeItems.filter((i) => i.id !== item.id) });
  return (
    <Card title="Casa · Home Assistant">
      <Row label="Dirección" hint="Por ejemplo http://192.168.1.10:8123">
        <TextField
          value={widgets.homeUrl}
          placeholder="http://homeassistant.local:8123"
          onCommit={(homeUrl) => save({ homeUrl: homeUrl.replace(/\/+$/, "") })}
        />
      </Row>
      <SecretRow
        label="Token de acceso"
        hint="Home Assistant › tu perfil › Seguridad › Token de acceso de larga duración."
        secretKey="casa.token"
        placeholder="eyJhbGciOi…"
      />
      <Row label="Botones" hint={message || "Copia la dirección y los botones de tu Barra anterior o del Panel de proyectos."}>
        <button type="button" className="push" onClick={importNow}>Importar</button>
      </Row>
      {widgets.homeItems.map((item) => (
        <div className="row" key={item.id}>
          <div className="row-text">
            <span className="row-label">{item.name}</span>
            <span className="row-hint">{item.entities.join(", ")}</span>
          </div>
          <div className="row-control">
            <button type="button" className="push" onClick={() => removeItem(item)}>Quitar</button>
          </div>
        </div>
      ))}
    </Card>
  );
}
