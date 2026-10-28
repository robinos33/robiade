import type { DbEngine } from './types';

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

/** `day` : jour local compté depuis l'epoch, tel que calculé côté Rust. */
export function dayLabel(day: number): string {
  const offset = new Date().getTimezoneOffset() * 60000;
  return dayFmt.format(day * 86400000 + offset + 12 * 3600000);
}

export function mrNumber(url: string): string | null {
  return url.match(/merge_requests\/(\d+)/)?.[1] ?? null;
}

export function shortPath(path: string, home: string): string {
  return home && path.startsWith(home) ? `~${path.slice(home.length)}` : path;
}
