import type { DbEngine, Granularity } from './types';

const rtf = new Intl.RelativeTimeFormat('fr', { numeric: 'auto', style: 'short' });
const compact = new Intl.NumberFormat('fr', { notation: 'compact', maximumFractionDigits: 1 });
const integer = new Intl.NumberFormat('fr');
const dayFmt = new Intl.DateTimeFormat('fr', { weekday: 'short', day: 'numeric', month: 'short' });
const dateTimeFmt = new Intl.DateTimeFormat('fr', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
const timeFmt = new Intl.DateTimeFormat('fr', { hour: '2-digit', minute: '2-digit' });

export function ago(at: number, now = Date.now()): string {
  const s = Math.round((at - now) / 1000);
  const abs = Math.abs(s);
  if (abs < 45) return 'à l’instant';
  if (abs < 3600) return rtf.format(Math.round(s / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.round(s / 3600), 'hour');
  if (abs < 86400 * 30) return rtf.format(Math.round(s / 86400), 'day');
  return dateTimeFmt.format(at);
}

export function duration(ms: number): string {
  const m = Math.floor(ms / 60000);
  if (m < 1) return `${Math.max(0, Math.round(ms / 1000))} s`;
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return `${h} h ${String(m % 60).padStart(2, '0')}`;
}

export const tokens = (n: number) => compact.format(n);

const usd = new Intl.NumberFormat('fr', { style: 'currency', currency: 'USD', maximumFractionDigits: 2 });
const usdRound = new Intl.NumberFormat('fr', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 });
/** Coût estimé : jamais une facture, toujours précédé de « ≈ ». */
export const dollars = (n: number) => `≈ ${(n >= 100 ? usdRound : usd).format(n)}`;
export const count = (n: number) => integer.format(n);
export const dateTime = (at: number) => dateTimeFmt.format(at);
export const time = (at: number) => timeFmt.format(at);

const ENGINE_LABELS: Record<DbEngine, string> = {
  postgres: 'PostgreSQL',
  mariadb: 'MariaDB',
  mongo: 'MongoDB',
  elasticsearch: 'Elasticsearch',
};
export const engineLabel = (engine: DbEngine) => ENGINE_LABELS[engine];
/** Le serveur tient une session en lecture seule et le confirme ; sinon, c'est la console. */
export const sessionReadOnly = (engine: DbEngine) => engine === 'postgres' || engine === 'mariadb';

/** Jour local `AAAA-MM-JJ`, celui d'un `<input type="date">` — pas le jour UTC de `toISOString`. */
export function localDay(at: number): string {
  const d = new Date(at);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** `day` : jour local compté depuis l'epoch, tel que calculé côté Rust. */
export function dayLabel(day: number): string {
  const offset = new Date().getTimezoneOffset() * 60000;
  return dayFmt.format(day * 86400000 + offset + 12 * 3600000);
}

const weekFmt = new Intl.DateTimeFormat('fr', { day: 'numeric', month: 'short' });
const monthFmt = new Intl.DateTimeFormat('fr', { month: 'long', year: 'numeric' });

/** Période commençant le jour local `day` : « mar. 3 nov. », « sem. du 2 nov. », « novembre 2026 », « 2026 ». */
export function periodLabel(day: number, granularity: Granularity): string {
  const at = day * 86400000 + new Date().getTimezoneOffset() * 60000 + 12 * 3600000;
  switch (granularity) {
    case 'day':
      return dayLabel(day);
    case 'week':
      return `sem. du ${weekFmt.format(at)}`;
    case 'month':
      return monthFmt.format(at);
    case 'year':
      return String(new Date(at).getFullYear());
  }
}

export function mrNumber(url: string): string | null {
  return url.match(/merge_requests\/(\d+)/)?.[1] ?? null;
}

/** Projet et numéro d'une URL de MR ; le Rust revalide tout ce qui en sort. */
export function mrRefOf(url: string): { project: string; iid: number } | null {
  const m = url.match(/^https:\/\/[^/]+\/(.+?)\/-\/merge_requests\/(\d+)/);
  return m?.[1] && m[2] ? { project: m[1], iid: Number(m[2]) } : null;
}

const sizeFmt = new Intl.NumberFormat('fr', { maximumFractionDigits: 1 });

/** Taille d'un fichier en octets, Ko ou Mo (base 1024, comme le Rust qui la borne). */
export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} o`;
  if (bytes < 1024 * 1024) return `${sizeFmt.format(bytes / 1024)} Ko`;
  return `${sizeFmt.format(bytes / (1024 * 1024))} Mo`;
}

export function shortPath(path: string, home: string): string {
  return home && path.startsWith(home) ? `~${path.slice(home.length)}` : path;
}
