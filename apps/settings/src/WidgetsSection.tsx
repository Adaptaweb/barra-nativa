/**
 * Barra widgets, in two places of Settings: the menu bar section edits the
 * menu bar's widgets (`part="bar"`), the Widgets section the dock's and the
 * islands' (`part="dock"`). Configuraciones holds what widgets need (the
 * weather city, the calendar link, Home Assistant). Secrets go to the Windows
 * Credential Manager and never come back to this page; only whether one is saved.
 */

import { type ReactNode, useEffect, useState } from "react";
import {
  type Config,
  type DisplayInfo,
  type HomeItem,
  type ScreenWidgets,
  type WidgetSettings,
  googleConnect,
  googleDisconnect,
  googleStatus,
  importHome,
  listDisplays,
  openUrl,
  secretSaved,
  setSecret,
} from "./config";
import { Card, Row, Segmented } from "./controls";
import { Tile } from "./tiles";
import { AddWidget, type MapBox, type Zone, ZONES, ZoneList, ZoneMap, isBarZone } from "./ZoneEditor";

export const DEFAULT_SCREEN: ScreenWidgets = {
  enabled: true,
  barLeft: [],
  barRight: ["reloj"],
  dockLeft: [],
  dock: [],
  islandLeft: [],
  islandRight: [],
  dockSizes: {},
};

/** The boxes of the zone map; the dashed ones are what the dock and the bar draw themselves. */
const DOCK_MAP: MapBox[] = [
  { zone: "islandLeft" },
  { zone: "dockLeft" },
  { note: "tus apps" },
  { zone: "dock" },
  { zone: "islandRight" },
];
const BAR_MAP: MapBox[] = [{ zone: "barLeft" }, { note: "hora e iconos" }, { zone: "barRight" }];

interface Props {
  config: Config;
  update: (patch: Partial<Config>) => void;
  /** Which widgets this page edits: the menu bar's or the dock's. */
  part: "bar" | "dock";
}

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

  const map = part === "bar" ? BAR_MAP : DOCK_MAP;
  const zones = ZONES.filter(({ zone }) => isBarZone(zone) === (part === "bar")).map((z) => z.zone);
  const [selected, setSelected] = useState<Zone>(
    () => zones.find((z) => (layout[z] ?? []).length > 0) ?? (part === "bar" ? "barLeft" : "islandRight"),
  );
  const zone = zones.includes(selected) ? selected : zones[0];
  const keys = layout[zone] ?? [];
  const counts = Object.fromEntries(zones.map((z) => [z, (layout[z] ?? []).length]));

  const reorder = (from: number, to: number) => {
    const list = [...keys];
    if (from === to || from < 0 || to < 0 || from >= list.length || to >= list.length) return;
    const [item] = list.splice(from, 1);
    list.splice(to, 0, item);
    saveLayout({ [zone]: list });
  };
  const remove = (index: number) => saveLayout({ [zone]: keys.filter((_, i) => i !== index) });
  const add = (key: string) => {
    if (!key || keys.includes(key)) return;
    saveLayout({ [zone]: [...keys, key] });
  };

  return (
    <>
      {displays.length > 1 ? (
        <Card title={part === "bar" ? "Pantalla" : undefined}>
          <ScreenPicker displays={displays} value={editing ? editing.device : ""} onChange={setScreen} />
        </Card>
      ) : null}
      <Card>
        <ZoneMap boxes={map} counts={counts} selected={zone} onSelect={setSelected} />
        {part === "dock" ? (
          <Row
            label="Alto de las islas"
            hint={editing
              ? `Solo en ${editing.label}. Cuántas veces el icono mide de alto la burbuja de cada isla.`
              : "Cuántas veces el icono mide de alto la burbuja de cada isla."}
          >
            <Segmented<"10" | "12" | "15">
              label="Alto de las islas"
              value={String((editing ? layout.islandScale : null) ?? widgets.islandScale ?? 12) as "10" | "12" | "15"}
              options={[
                { value: "10", label: "1,0×" },
                { value: "12", label: "1,2×" },
                { value: "15", label: "1,5×" },
              ]}
              onChange={(scale) =>
                editing
                  ? saveScreen(editing.device, { islandScale: Number(scale) })
                  : save({ islandScale: Number(scale) })}
            />
          </Row>
        ) : null}
      </Card>
      <ZoneList
        zone={zone}
        keys={keys}
        sizes={layout.dockSizes}
        showSize={part === "dock"}
        onReorder={reorder}
        onRemove={remove}
        onSize={(key, size) => saveLayout({ dockSizes: { ...layout.dockSizes, [key]: size } })}
      >
        <AddWidget key={zone} zone={zone} placed={keys} onAdd={add} />
      </ZoneList>

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

type ServiceId = "clima" | "calendario" | "casa" | "ia";

/** Accounts and services the widgets need: weather city, calendar, Home Assistant, AI limits. */
export function ServicesSection({ config, update }: Omit<Props, "part">) {
  const [open, setOpen] = useState<ServiceId | null>(null);
  const [status, setStatus] = useState({ google: false, ical: false, token: false });
  // Read again when a row opens or closes, so the pills follow what was just saved.
  useEffect(() => {
    Promise.all([
      googleStatus().catch(() => false),
      secretSaved("calendario.ics").catch(() => false),
      secretSaved("casa.token").catch(() => false),
    ]).then(([google, ical, token]) => setStatus({ google, ical, token }));
  }, [open]);
  // A settings file from before widgets existed has none yet.
  if (!config.widgets) return null;
  const widgets = config.widgets;
  const save = (patch: Partial<WidgetSettings>) => update({ widgets: { ...widgets, ...patch } });
  const toggle = (id: ServiceId) => setOpen(open === id ? null : id);

  const buttons = widgets.homeItems.length;
  const homeSummary = widgets.homeUrl || buttons > 0
    ? `${buttons} ${buttons === 1 ? "botón" : "botones"}${widgets.cameraName ? ` · cámara ${widgets.cameraName}` : ""}`
    : "Sin configurar";

  return (
    <Card>
      <ServiceRow
        tile="clima"
        title="Clima"
        subtitle={widgets.weatherCity || "Sin ciudad"}
        open={open === "clima"}
        onToggle={() => toggle("clima")}
      >
        <Row label="Ciudad" hint="El nombre se busca en Open-Meteo; no hace falta cuenta.">
          <TextField
            value={widgets.weatherCity}
            placeholder="Santiago"
            onCommit={(weatherCity) => save({ weatherCity })}
          />
        </Row>
      </ServiceRow>
      <ServiceRow
        tile="calendario"
        title="Calendario"
        subtitle={status.google ? "Google" : status.ical ? "iCal" : "Sin conectar"}
        pill={status.google || status.ical ? "Conectado" : undefined}
        open={open === "calendario"}
        onToggle={() => toggle("calendario")}
      >
        <CalendarCard />
      </ServiceRow>
      <ServiceRow
        tile="casa"
        title="Casa · Home Assistant"
        subtitle={homeSummary}
        pill={widgets.homeUrl && status.token ? "Conectado" : undefined}
        open={open === "casa"}
        onToggle={() => toggle("casa")}
      >
        <HomeCard widgets={widgets} save={save} />
      </ServiceRow>
      <ServiceRow
        tile="claude"
        title="Límites de IA"
        subtitle="Claude, ChatGPT, Gemini"
        open={open === "ia"}
        onToggle={() => toggle("ia")}
      >
        <div className="row">
          <div className="row-text">
            <span className="row-hint">
              Claude usa la sesión de Claude Code de este PC (abre «claude» en una terminal si caducó).
              ChatGPT usa Codex CLI («npm install -g @openai/codex» y «codex login»). Gemini usa Gemini CLI
              o Antigravity abierto. Nada se envía a Barra; cada widget explica los pasos en su panel.
            </span>
          </div>
        </div>
      </ServiceRow>
    </Card>
  );
}

/** One service of Configuraciones: a header that unfolds its settings below (one open at a time). */
function ServiceRow({ tile, title, subtitle, pill, open, onToggle, children }: {
  tile: string;
  title: string;
  subtitle: string;
  pill?: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <div className="service">
      <button type="button" className="service-head" aria-expanded={open} onClick={onToggle}>
        <Tile id={tile} size={30} />
        <span className="row-text">
          <span className="row-label">{title}</span>
          <span className="row-hint">{subtitle}</span>
        </span>
        {pill ? <span className="pill">{pill}</span> : null}
        <svg className="service-chevron" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
          <path d="m6 3 5 5-5 5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>
      {open ? <div className="service-body">{children}</div> : null}
    </div>
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
    <>
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
    </>
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
    <>
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
      <Row label="Cámara · go2rtc URL" hint="Si lo dejas vacío, en este PC se usa la cámara de panel-proyectos.">
        <TextField
          value={widgets.cameraUrl}
          placeholder="http://192.168.100.250:1984"
          onCommit={(cameraUrl) => save({ cameraUrl: cameraUrl.replace(/\/+$/, "") })}
        />
      </Row>
      <Row label="Usuario" hint="Usuario de la cámara en go2rtc.">
        <TextField
          value={widgets.cameraUser}
          placeholder="panel"
          onCommit={(cameraUser) => save({ cameraUser })}
        />
      </Row>
      <SecretRow
        label="Clave"
        hint="Contraseña de go2rtc. Se guarda solo en este PC."
        secretKey="camara.clave"
        placeholder="Contraseña"
      />
      <Row label="Fuente" hint="Nombre del stream en go2rtc.">
        <TextField
          value={widgets.cameraSource}
          placeholder="porton_sd"
          onCommit={(cameraSource) => save({ cameraSource })}
        />
      </Row>
      <Row label="Nombre" hint="Se muestra sobre la imagen.">
        <TextField
          value={widgets.cameraName}
          placeholder="Portón"
          onCommit={(cameraName) => save({ cameraName })}
        />
      </Row>
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
    </>
  );
}
