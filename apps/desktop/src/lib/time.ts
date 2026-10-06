// Display helpers. Parsing always goes through the core (`api.parseTime`): one grammar everywhere.

export function fmt(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds)) return "--:--:--.---";
  const ms = Math.round(Math.max(0, seconds) * 1000);
  const h = Math.floor(ms / 3_600_000);
  const m = Math.floor((ms % 3_600_000) / 60_000);
  const s = Math.floor((ms % 60_000) / 1000);
  const f = ms % 1000;
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(h)}:${p(m)}:${p(s)}.${p(f, 3)}`;
}

/** Seconds from a manifest time value that is already known to be numeric. */
export function seconds(v: number | string | undefined): number | null {
  return typeof v === "number" ? v : null;
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
