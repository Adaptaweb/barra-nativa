/**
 * Barra widgets, in two places of Settings: the menu bar section edits the
 * menu bar's widgets (`part="bar"`), the Widgets section the dock's and the
 * islands' (`part="dock"`) and what widgets need (the weather city, the
 * calendar link, Home Assistant). Secrets go to the Windows Credential
 * Manager and never come back to this page; only whether one is saved.
 */

import { useEffect, useState } from "react";
import {
  type Config,
  type DisplayInfo,
  type HomeItem,
  type ScreenWidgets,
  type WidgetSettings,
  ICON_SIZE,
  googleConnect,
  googleDisconnect,
  googleStatus,
  importHome,
  listDisplays,
  openUrl,
  secretSaved,
  setSecret,
} from "./config";
import { Card, Row, Slider, Toggle } from "./controls";
import { type PreviewSize, WidgetPreview } from "./WidgetPreview";

type Zone = "barLeft" | "barRight" | "islandLeft" | "dockLeft" | "dock" | "islandRight";

const WIDGETS: { key: string; name: string; detail: string }[] = [
  { key: "reloj", name: "Reloj y calendario", detail: "Fecha, hora y tu próximo evento. Reemplaza el reloj de la barra." },
  { key: "clima", name: "Clima", detail: "Temperatura y cielo de tu ciudad." },
  { key: "sistema", name: "Sistema", detail: "Temperatura de la CPU, uso de CPU y memoria." },
  { key: "casa", name: "Casa", detail: "Tus botones de Home Assistant." },
  { key: "claude", name: "Claude", detail: "Límites de 5 horas y semanal de tu plan." },
  { key: "chatgpt", name: "ChatGPT", detail: "Límites de tu plan, a través de Codex." },
  { key: "gemini", name: "Gemini", detail: "Cuota de Gemini CLI o Antigravity." },
  { key: "control", name: "Centro de control", detail: "Red, volumen y batería; abre la configuración rápida de Windows." },
  { key: "notificaciones", name: "Notificaciones", detail: "Un botón de campana; abre el centro de notificaciones de Windows." },
  { key: "bandeja", name: "Segundo plano", detail: "Los iconos ocultos de la bandeja de Windows (solo en el dock)." },
];

const ZONES: { zone: Zone; title: string; hint: string }[] = [
  { zone: "barLeft", title: "Barra superior · izquierda", hint: "Después del nombre de la app activa." },
  { zone: "barRight", title: "Barra superior · derecha", hint: "Antes de los iconos de estado." },
  { zone: "islandLeft", title: "Isla izquierda", hint: "Una burbuja aparte abajo a la izquierda; el dock queda solo con tus apps." },
  { zone: "dockLeft", title: "Dentro del dock · izquierda", hint: "Antes de tus apps; las apps quedan centradas." },
  { zone: "dock", title: "Dentro del dock · derecha", hint: "Al final del dock, después de las carpetas." },
  { zone: "islandRight", title: "Isla derecha", hint: "Una burbuja aparte abajo a la derecha." },
];

const DEFAULT_SCREEN: ScreenWidgets = {
  enabled: true,
  barLeft: [],
  barRight: ["reloj"],
  dockLeft: [],
  dock: [],
  islandLeft: [],
  islandRight: [],
  dockSizes: {},
};

const nameOf = (key: string) => WIDGETS.find((w) => w.key === key)?.name ?? key;

interface Props {
  config: Config;
  update: (patch: Partial<Config>) => void;
  /** Which widgets this page edits: the menu bar's or the dock's. */
  part: "bar" | "dock";
}

const isBarZone = (zone: Zone) => zone === "barLeft" || zone === "barRight";

export function WidgetsSection(props: Props) {
  // A settings file from before widgets existed has none yet.
  if (!props.config.widgets) return null;
  return <WidgetsPage {...props} />;
}

function WidgetsPage({ config, update, part }: Props) {
  const widgets = config.widgets;
  const save = (patch: Partial<WidgetSettings>) => update({ widgets: { ...widgets, ...patch } });
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);
  const [screen, setScreen] = useState<string>("");
  useEffect(() => {
    listDisplays().then(setDisplays).catch(() => setDisplays([]));
  }, []);
  const editing = displays.find((d) => d.device === screen && !d.primary);
  // The primary display uses the top-level fields; the others their own entry.
  const layout: ScreenWidgets = editing
    ? { ...DEFAULT_SCREEN, ...(widgets.screens?.[editing.device] ?? {}) }
    : {
        enabled: true,
        barLeft: widgets.barLeft,
        barRight: widgets.barRight,
        dockLeft: widgets.dockLeft,
        dock: widgets.dock,
        islandLeft: widgets.islandLeft,
        islandRight: widgets.islandRight,
        dockSizes: widgets.dockSizes,
      };
  const saveScreen = (device: string, patch: Partial<ScreenWidgets>) =>
    save({
      screens: {
        ...(widgets.screens ?? {}),
        [device]: { ...DEFAULT_SCREEN, ...(widgets.screens?.[device] ?? {}), ...patch },
      },
    });
  const saveLayout = (patch: Partial<ScreenWidgets>) =>
    editing ? saveScreen(editing.device, patch) : save(patch as Partial<WidgetSettings>);

  const move = (zone: Zone, index: number, by: number) => {
    const list = [...(layout[zone] ?? [])];
    const target = index + by;
    if (target < 0 || target >= list.length) return;
    [list[index], list[target]] = [list[target], list[index]];
    saveLayout({ [zone]: list });
  };
  const remove = (zone: Zone, index: number) =>
    saveLayout({ [zone]: (layout[zone] ?? []).filter((_, i) => i !== index) });
  const add = (zone: Zone, key: string) => {
    if (!key || (layout[zone] ?? []).includes(key)) return;
    saveLayout({ [zone]: [...(layout[zone] ?? []), key] });
  };

  return (
    <>
      {displays.length > 1 && part === "bar" ? (
        <Card title="Pantalla">
          <ScreenPicker displays={displays} value={editing ? editing.device : ""} onChange={setScreen} />
        </Card>
      ) : null}
      {displays.length > 1 && part === "dock" ? (
        <Card title="Pantallas">
          {displays.map((d) => (
            <Row
              key={d.device}
              label={d.label}
              hint={d.primary ? "Siempre tiene dock y barra superior." : "Dock y barra superior propios, con sus widgets y solo las apps de esta pantalla."}
            >
              <Toggle
                label={`Dock en ${d.label}`}
                checked={d.primary || (widgets.screens?.[d.device]?.enabled ?? true)}
                disabled={d.primary}
                onChange={(enabled) => saveScreen(d.device, { enabled })}
              />
            </Row>
          ))}
          {displays.filter((d) => !d.primary).map((d) => {
            const own = widgets.screens?.[d.device]?.iconSize ?? null;
            return (
              <div key={`size-${d.device}`}>
                <Row
                  label={`Tamaño propio en ${d.label}`}
                  hint={own === null ? `Usa el tamaño del dock principal (${config.iconSize} px).` : "Solo el dock de esta pantalla."}
                >
                  <Toggle
                    label={`Tamaño propio en ${d.label}`}
                    checked={own !== null}
                    onChange={(on) => saveScreen(d.device, { iconSize: on ? config.iconSize : null })}
                  />
                </Row>
                {own !== null ? (
                  <Row label="Tamaño del dock">
                    <Slider
                      label={`Tamaño del dock en ${d.label}`}
                      value={own}
                      min={ICON_SIZE.min}
                      max={ICON_SIZE.max}
                      onChange={(iconSize) => saveScreen(d.device, { iconSize })}
                    />
                  </Row>
                ) : null}
              </div>
            );
          })}
          <ScreenPicker displays={displays} value={editing ? editing.device : ""} onChange={setScreen} />
        </Card>
      ) : null}
      {ZONES.filter(({ zone }) => isBarZone(zone) === (part === "bar")).map(({ zone, title, hint }) => (
        <Card key={zone} title={title}>
          {(layout[zone] ?? []).length === 0 ? (
            <div className="row"><div className="row-text"><span className="row-hint">{hint} Sin widgets.</span></div></div>
          ) : (
            (layout[zone] ?? []).map((key, index) => (
              <div className="row" key={key}>
                <div className="row-text">
                  <span className="row-label">{nameOf(key)}</span>
                  {index === 0 ? <span className="row-hint">{hint}</span> : null}
                </div>
                <div className="row-control push-group">
                  {zone !== "barLeft" && zone !== "barRight" ? (
                    <span className="select-wrap">
                      <select
                        className="select-control size-control"
                        aria-label={`Tamaño de ${nameOf(key)}`}
                        value={layout.dockSizes?.[key] ?? "mediano"}
                        onChange={(e) => saveLayout({ dockSizes: { ...layout.dockSizes, [key]: e.target.value } })}
                      >
                        <option value="compacto">Compacto</option>
                        <option value="mediano">Mediano</option>
                        <option value="ancho">Ancho</option>
                      </select>
                    </span>
                  ) : null}
                  <button type="button" className="push" aria-label="Subir" disabled={index === 0} onClick={() => move(zone, index, -1)}>↑</button>
                  <button type="button" className="push" aria-label="Bajar" disabled={index === (layout[zone] ?? []).length - 1} onClick={() => move(zone, index, 1)}>↓</button>
                  <button type="button" className="push" aria-label={`Quitar ${nameOf(key)}`} onClick={() => remove(zone, index)}>Quitar</button>
                </div>
              </div>
            ))
          )}
          <AddRow zone={zone} placed={layout[zone] ?? []} onAdd={(key) => add(zone, key)} />
        </Card>
      ))}

      {part === "dock" ? (
        <div className="row">
          <div className="row-text">
            <span className="row-hint">
              Las cuentas y servicios de cada widget (clima, calendario, casa, IA) están en Configuraciones.
            </span>
          </div>
        </div>
      ) : null}
    </>
  );
}

/** Accounts and services the widgets need: weather city, calendar, Home Assistant, AI limits. */
export function ServicesSection({ config, update }: Omit<Props, "part">) {
  // A settings file from before widgets existed has none yet.
  if (!config.widgets) return null;
  const widgets = config.widgets;
  const save = (patch: Partial<WidgetSettings>) => update({ widgets: { ...widgets, ...patch } });
  return (
    <>
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

function ScreenPicker({ displays, value, onChange }: { displays: DisplayInfo[]; value: string; onChange: (device: string) => void }) {
  return (
    <Row label="Editar widgets de" hint="Cada pantalla tiene sus propios widgets.">
      <span className="select-wrap">
        <select className="select-control" aria-label="Pantalla a editar" value={value} onChange={(e) => onChange(e.target.value)}>
          {displays.map((d) => (
            <option key={d.device} value={d.primary ? "" : d.device}>{d.label}</option>
          ))}
        </select>
      </span>
    </Row>
  );
}

function AddRow({ zone, placed, onAdd }: { zone: Zone; placed: string[]; onAdd: (key: string) => void }) {
  const bar = isBarZone(zone);
  // The tray chevron lives only in the dock.
  const free = WIDGETS.filter((w) => !placed.includes(w.key) && !(bar && w.key === "bandeja"));
  const [choice, setChoice] = useState("");
  const [size, setSize] = useState<PreviewSize>("mediano");
  if (free.length === 0) return null;
  const selected = free.find((w) => w.key === choice);
  return (
    <>
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
    {selected ? (
      <div className="row row-stacked widget-preview-row">
        <div className="row-text">
          <span className="row-label">Vista previa</span>
          <span className="row-hint">Así se ve {selected.name} {bar ? "en la barra superior" : "en el dock"}, con datos de ejemplo.</span>
        </div>
        <div className="row-control widget-preview-control">
          <WidgetPreview widget={selected.key} place={bar ? "bar" : "dock"} size={size} />
          {bar ? null : (
            <span className="select-wrap">
              <select className="select-control size-control" aria-label="Tamaño de la vista previa" value={size} onChange={(e) => setSize(e.target.value as PreviewSize)}>
                <option value="compacto">Compacto</option>
                <option value="mediano">Mediano</option>
                <option value="ancho">Ancho</option>
              </select>
            </span>
          )}
        </div>
      </div>
    ) : null}
    </>
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

/** Google Calendar connection: OAuth client JSON, plus connect / disconnect with status. */
function GoogleRow() {
  const [connected, setConnected] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  useEffect(() => {
    googleStatus().then(setConnected).catch(() => setConnected(false));
  }, []);
  const connect = () => {
    setBusy(true);
    setMessage("Esperando al navegador…");
    googleConnect()
      .then(() => {
        setConnected(true);
        setMessage("Conectado. El calendario se actualizará en un minuto.");
      })
      .catch((error) => setMessage(String(error)))
      .finally(() => setBusy(false));
  };
  const disconnect = () => {
    googleDisconnect()
      .then(() => {
        setConnected(false);
        setMessage("");
      })
      .catch((error) => setMessage(String(error)));
  };
  return (
    <>
      <div className="row">
        <div className="row-text">
          <span className="row-hint">
            Conecta tu cuenta de Google para ver el título, la hora y el lugar de cada reunión. Necesitas un
            cliente OAuth de tipo «Aplicación de escritorio» con la API de Google Calendar activada; pega aquí
            el contenido de su archivo JSON.
          </span>
        </div>
      </div>
      <div className="row">
        <div className="row-text">
          <details className="steps">
            <summary>Cómo conseguir el JSON (una vez por PC o cuenta)</summary>
            <ol>
              <li>Entra en Google Cloud Console con tu cuenta de Google y crea un proyecto (por ejemplo «Barra»).</li>
              <li>En «APIs y servicios › Biblioteca» busca «Google Calendar API» y pulsa «Habilitar».</li>
              <li>En «Pantalla de consentimiento de OAuth» elige tipo «Externo», pon de nombre «Barra», tu correo de asistencia y guarda.</li>
              <li>En «Público › Usuarios de prueba» añade tu propio correo.</li>
              <li>En «Credenciales › Crear credenciales › ID de cliente de OAuth» elige tipo «Aplicación de escritorio» y créalo.</li>
              <li>Pulsa «Descargar JSON», abre el archivo con el Bloc de notas, copia todo el texto y pégalo en «Cliente OAuth (JSON)» de arriba; luego «Guardar».</li>
              <li>Pulsa «Conectar con Google» y acepta en el navegador. Si avisa de que la app no está verificada, elige «Avanzado › Ir a Barra»: es tu propia app.</li>
            </ol>
            <div className="push-group">
              <button type="button" className="push" onClick={() => void openUrl("https://console.cloud.google.com/projectcreate").catch(() => {})}>
                Abrir Google Cloud Console
              </button>
              <button type="button" className="push" onClick={() => void openUrl("https://console.cloud.google.com/apis/library/calendar-json.googleapis.com").catch(() => {})}>
                Activar Google Calendar API
              </button>
            </div>
          </details>
        </div>
      </div>
      <SecretRow
        label="Cliente OAuth (JSON)"
        hint="Se guarda solo en este PC."
        secretKey="google.cliente"
        placeholder='{"installed":{"client_id":"…","client_secret":"…"}}'
        multiline
      />
      <div className="row">
        <div className="row-text">
          <span className="row-label">
            Google · {connected === null ? "…" : connected ? "conectado" : "sin conectar"}
          </span>
          {message ? <span className="row-hint">{message}</span> : null}
        </div>
        <div className="row-control">
          <div className="push-group">
            {connected ? (
              <button type="button" className="push" onClick={disconnect}>Desconectar</button>
            ) : (
              <button type="button" className="push push-primary" disabled={busy || connected === null} onClick={connect}>
                {busy ? "Esperando al navegador…" : "Conectar con Google"}
              </button>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

function CalendarCard() {
  return (
    <Card title="Calendario">
      <GoogleRow />
      <div className="row">
        <div className="row-text">
          <span className="row-label">O usa enlaces iCal (solo si no conectas Google):</span>
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
