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
    <text x="20" y="33" textAnchor="middle" className="wp-ring-label">{label}</text>
  </svg>
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
          <span className="wp-emoji">☀️</span>
          <b className="wp-big">19°</b>
          {compact ? null : <span className="wp-stack"><b>Despejado</b><span className="wp-dim">↑24° ↓9°</span></span>}
          {wide ? <span className="wp-hours"><span>15<br />☀️ 22°</span><span>16<br />⛅ 21°</span><span>17<br />⛅ 19°</span></span> : null}
        </>
      );
    case "sistema":
      return (
        <>
          <Ring value="52°" color="#34c759" label="CPU" />
          {compact ? null : <span className="wp-stack"><Bar label="CPU" value={18} color="#0a84ff" /><Bar label="RAM" value={47} color="#34c759" /></span>}
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
        <span className="wp-glyphs">
          <span className="wp-glyph">{""}</span>
          <span className="wp-glyph">{""}</span>
          {compact ? null : <span className="wp-glyph">{""}</span>}
          {compact ? null : <b>80%</b>}
        </span>
      );
    case "bandeja":
      return <span className="wp-glyph">{""}</span>;
    case "notificaciones":
      return <span className="wp-glyph">{""}</span>;
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
    default:
      return <span className="wp-dim">{key}</span>;
  }
}

function barCell(key: string) {
  switch (key) {
    case "reloj": return <span>mié 25 nov  14:30 · <span className="wp-dim">Reunión en 25 min</span></span>;
    case "clima": return <span>☀️ 19°</span>;
    case "sistema": return <span><span className="wp-dot" style={{ background: "#34c759" }} /> 52° · CPU 18% · RAM 47%</span>;
    case "casa": return <span>🏠 2</span>;
    case "claude": return <span><b>C</b> 63% · 1 h 52 min</span>;
    case "chatgpt": return <span><b>G</b> 40% · 3 h</span>;
    case "gemini": return <span><b>✦</b> 12%</span>;
    case "control": return <span className="wp-glyphs"><span className="wp-glyph">{""}</span><span className="wp-glyph">{""}</span></span>;
    case "notificaciones": return <span className="wp-glyph">{""}</span>;
    case "musica": return <span><span className="wp-glyph">{""}</span> Midnight City</span>;
    default: return null;
  }
}

/** A sample of widget `key` where it would go. */
export function WidgetPreview({ widget, place, size = "mediano" }: { widget: string; place: PreviewPlace; size?: PreviewSize }) {
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
