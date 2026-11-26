/**
 * How a widget looks, drawn with sample readings: in the menu bar (one thin
 * line) or in the dock and islands (a cell on the glass, at its size). It
 * follows the dock's drawing in `apps/dock/src/widgets/cell.rs` and `more.rs`.
 */

export type PreviewPlace = "bar" | "dock";
export type PreviewSize = "compacto" | "mediano" | "ancho";

const Ring = ({ value, color, label }: { value: string; color: string; label: string }) => (
  <svg className="wp-ring" viewBox="0 0 40 40" aria-hidden="true">
    <circle cx="20" cy="20" r="15" fill="none" stroke="rgba(255,255,255,0.22)" strokeWidth="4" strokeDasharray="70.7 94.2" transform="rotate(135 20 20)" strokeLinecap="round" />
    <circle cx="20" cy="20" r="15" fill="none" stroke={color} strokeWidth="4" strokeDasharray="35 94.2" transform="rotate(135 20 20)" strokeLinecap="round" />
    <text x="20" y="21" textAnchor="middle" className="wp-ring-value">{value}</text>
    <text x="20" y="35" textAnchor="middle" className="wp-ring-label">{label}</text>
  </svg>
);

/** The weather's sun, drawn as the dock draws it: a yellow-to-orange disc and eight rays. */
const Sun = () => (
  <svg className="wp-sun" viewBox="0 0 40 40" aria-hidden="true">
    <defs><linearGradient id="wp-sun-disc" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="#ffe14d" /><stop offset="1" stopColor="#ff9f0a" /></linearGradient></defs>
    <g stroke="#ffcc00" strokeWidth="2.4" strokeLinecap="round">
      {[0, 45, 90, 135, 180, 225, 270, 315].map((a) => {
        const r = (a * Math.PI) / 180;
        return <line key={a} x1={20 + Math.cos(r) * 12.5} y1={20 + Math.sin(r) * 12.5} x2={20 + Math.cos(r) * 16.5} y2={20 + Math.sin(r) * 16.5} />;
      })}
    </g>
    <circle cx="20" cy="20" r="9" fill="url(#wp-sun-disc)" />
  </svg>
);

/** The battery's box filling with green water whose waves keep moving (as the dock draws it). */
const Water = ({ level, charging, label }: { level: number; charging: boolean; label: string }) => (
  <span className="wp-water" aria-hidden="true">
    <span className="wp-water-body" style={{ height: `${Math.max(8, level)}%` }}>
      <svg className="wp-wave wp-wave-back" viewBox="0 0 200 20" preserveAspectRatio="none"><path d="M0 10 Q 25 2 50 10 T 100 10 T 150 10 T 200 10 V 20 H 0 Z" /></svg>
      <svg className="wp-wave wp-wave-front" viewBox="0 0 200 20" preserveAspectRatio="none"><path d="M0 10 Q 25 18 50 10 T 100 10 T 150 10 T 200 10 V 20 H 0 Z" /></svg>
    </span>
    <span className="wp-water-label">{charging ? "⚡" : ""}{label}</span>
  </span>
);

const Bar = ({ label, value, color }: { label: string; value: number; color: string }) => (
  <span className="wp-meter">
    <span className="wp-dim">{label}</span>
    <span className="wp-track"><span style={{ width: `${value}%`, background: color }} /></span>
    <b>{value}%</b>
  </span>
);

function dockCell(key: string, size: PreviewSize) {
  const compact = size === "compacto";
  const wide = size === "ancho";
  switch (key) {
    case "reloj":
      return compact ? (
        <span className="wp-stack"><b className="wp-big">14:30</b><span className="wp-dim">mié 25</span></span>
      ) : (
        <>
          <span className="wp-stack"><span className="wp-red">MIÉRCOLES</span><b className="wp-big">25 nov</b></span>
          <span className="wp-sep" />
          <b className="wp-big">14:30</b>
          {wide ? (<><span className="wp-sep" /><span className="wp-stack"><b>Reunión de equipo</b><span className="wp-dim">en 25 min</span></span></>) : null}
        </>
      );
    case "clima":
      return (
        <>
          <Sun />
          <b className={compact ? "wp-big wp-huge" : "wp-big"}>19°</b>
          {compact ? null : <span className="wp-stack"><b>Despejado</b><span className="wp-dim">↑24° ↓9°</span></span>}
          {wide ? <span className="wp-hours"><span>15<br />☀️ 22°</span><span>16<br />⛅ 21°</span><span>17<br />⛅ 19°</span></span> : null}
        </>
      );
    case "sistema":
      return (
        <>
          <Ring value="52°" color="#34c759" label="TEMP" />
          {compact ? null : <><Ring value="18%" color="#34c759" label="CPU" /><Ring value="47%" color="#34c759" label="RAM" /></>}
        </>
      );
    case "bateria":
      return (
        <>
          <Water level={86} charging label={compact ? "86" : ""} />
          {compact ? null : <span className="wp-stack"><b className="wp-big">86 %</b><span className="wp-dim">Cargando</span></span>}
          {wide ? <><span className="wp-sep" /><span className="wp-stack"><b>Ahorro de batería: no</b><span className="wp-dim">Enchufado</span></span></> : null}
        </>
      );
    case "memoria":
      return (
        <>
          <Ring value="54%" color="#34c759" label="RAM" />
          {compact ? null : <span className="wp-stack"><b className="wp-big">8,2 GB</b><span className="wp-dim">de 16 GB · 7,8 libres</span></span>}
          {wide ? <span className="wp-stack"><Bar label="Uso" value={54} color="#34c759" /><span className="wp-dim">7,8 GB libres</span></span> : null}
        </>
      );
    case "casa":
      return (
        <>
          <span className="wp-emoji">🏠</span>
          {compact ? <b className="wp-big wp-yellow">2</b> : <span className="wp-stack"><b>Casa</b><span className="wp-dim">2 encendidos</span></span>}
        </>
      );
    case "claude":
    case "chatgpt":
    case "gemini": {
      const name = key === "claude" ? "Claude" : key === "chatgpt" ? "ChatGPT" : "Gemini";
      return (
        <>
          <span className="wp-logo">{name[0]}</span>
          {compact ? <b className="wp-big">63%</b> : (
            <>
              <span className="wp-stack"><span className="wp-dim">Sesión</span><b>63% · 1 h 52 min</b></span>
              <span className="wp-stack"><span className="wp-dim">Semana</span><b>44%</b></span>
            </>
          )}
        </>
      );
    }
    case "control":
      return (
        <span className="wp-cc">
          <span className="wp-cc-disc wp-switch-on"><span className="wp-glyph">{""}</span></span>
          <span className="wp-cc-disc wp-switch-on"><span className="wp-glyph">{""}</span></span>
          <span className="wp-cc-disc wp-switch-on"><span className="wp-glyph">{""}</span></span>
          <span className="wp-cc-disc"><span className="wp-glyph">{""}</span></span>
        </span>
      );
    case "bandeja":
      return <span className="wp-tile"><span className="wp-glyph">{""}</span></span>;
    case "notificaciones":
      return <span className="wp-tile"><span className="wp-tile-disc"><span className="wp-glyph">{""}</span></span></span>;
    case "musica":
      return (
        <span className="wp-music">
          <span className="wp-cover" />
          {compact ? null : <span className="wp-stack"><b>Midnight City</b><span className="wp-music-dim">M83</span></span>}
          <span className="wp-glyphs">
            {compact ? null : <span className="wp-glyph">{""}</span>}
            <span className="wp-glyph">{""}</span>
            {compact ? null : <span className="wp-glyph">{""}</span>}
          </span>
          {wide ? <span className="wp-music-bar"><span /></span> : null}
        </span>
      );
    case "audio":
      return (
        <span className="wp-audio">
          <span className="wp-glyph">{""}</span>
          {compact ? null : <span className="wp-glyph">{""}</span>}
        </span>
      );
    default:
      return <span className="wp-dim">{key}</span>;
  }
}

function barCell(key: string) {
  switch (key) {
    case "reloj": return <span>mié 25 nov  14:30 · <span className="wp-dim">Reunión en 25 min</span></span>;
    case "clima": return <span>☀️ 19°</span>;
    case "sistema": return <span><span className="wp-dot" style={{ background: "#34c759" }} /> 52° · CPU 18% · RAM 47%</span>;
    case "bateria": return <span>🔋 86 %</span>;
    case "memoria": return <span><span className="wp-dot" style={{ background: "#34c759" }} /> 54% · 8,2 GB</span>;
    case "casa": return <span>🏠 2</span>;
    case "claude": return <span><b>C</b> 63% · 1 h 52 min</span>;
    case "chatgpt": return <span><b>G</b> 40% · 3 h</span>;
    case "gemini": return <span><b>✦</b> 12%</span>;
    case "control": return <span className="wp-glyphs"><span className="wp-glyph">{""}</span><span className="wp-glyph">{""}</span></span>;
    case "notificaciones": return <span className="wp-glyph">{""}</span>;
    case "musica": return <span><span className="wp-glyph">{""}</span> Midnight City</span>;
    case "audio": return <span className="wp-glyph">{""}</span>;
    default: return null;
  }
}

/** A sample of widget `key` where it would go. `bare` draws only the widget, without the fake bar or dock around it. */
export function WidgetPreview({ widget, place, size = "mediano", bare = false }: { widget: string; place: PreviewPlace; size?: PreviewSize; bare?: boolean }) {
  if (bare) {
    return (
      <span className="wp-cell">
        {place === "bar" ? (barCell(widget) ?? <span className="wp-dim">Solo en el dock</span>) : dockCell(widget, size)}
      </span>
    );
  }
  if (place === "bar") {
    const cell = barCell(widget);
    return (
      <div className="wp wp-bar" aria-label="Vista previa en la barra superior">
        {cell ?? <span className="wp-dim">Solo en el dock</span>}
      </div>
    );
  }
  return (
    <div className="wp wp-dock" aria-label="Vista previa en el dock">
      <span className="wp-app" />
      <span className="wp-cell">{dockCell(widget, size)}</span>
      <span className="wp-app" />
    </div>
  );
}
