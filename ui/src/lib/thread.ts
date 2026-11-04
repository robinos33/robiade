import { Marked } from 'marked';
import { markdownStyle } from './markdown';
import type { ColorScheme } from './theme.svelte';
import { dateTime } from './format';
import type { Discussion } from './types';

const escape = (text: string) =>
  text.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] ?? c);

/**
 * Markdown de GitLab. Une image jointe (`/uploads/…`) est servie derrière authentification, que
 * l'iframe n'a pas : elle s'affiche comme son adresse sur GitLab, sans être chargée.
 */
function gitlabMarkdown(instance: string, project: string) {
  return new Marked({
    gfm: true,
    async: false,
    renderer: {
      image({ href, text }) {
        const url = href.startsWith('/uploads/') ? `${instance}/${project}${href}` : href;
        return `<span class="asset">Image jointe${text ? ` « ${escape(text)} »` : ''} : <span class="url">${escape(url)}</span></span>`;
      },
    },
  });
}

/** Mise en page du fil, ajoutée à celle du Markdown : le panneau est étroit, les liens inertes. */
const THREAD_STYLE = `<style>
  body { max-width: none; margin: 0; padding: 4px 18px 32px; }
  a { pointer-events: none; color: inherit; text-decoration: underline; text-decoration-color: rgb(127 127 127 / 0.6); }
  .asset { font-size: 12.5px; opacity: 0.75; }
  .asset .url { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; word-break: break-all; }
  .empty { opacity: 0.55; font-style: italic; }
  h2.exchanges { font-size: 11px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; opacity: 0.6; margin: 28px 0 10px; }
  .thread { border: 1px solid rgb(127 127 127 / 0.28); border-radius: 8px; margin: 0 0 12px; overflow: hidden; }
  .thread.resolved { opacity: 0.7; }
  .thread > .state { font-size: 11.5px; padding: 4px 12px; border-bottom: 1px solid rgb(127 127 127 / 0.2); }
  .thread.open > .state { color: #d98a00; }
  .thread.resolved > .state { color: #13895f; }
  .note { padding: 8px 12px; }
  .note + .note { border-top: 1px solid rgb(127 127 127 / 0.16); }
  .meta { font-size: 12px; opacity: 0.7; margin-bottom: 2px; }
  .meta strong { opacity: 1; }
  .body > :first-child { margin-top: 0; } .body > :last-child { margin-bottom: 0; }
</style>`;

/**
 * Page HTML d'une issue ou d'une MR : description puis fils d'échanges. Tout y est non fiable et
 * s'affiche dans une iframe `sandbox=""` : rien ne s'y exécute. L'échappement des auteurs sert au
 * rendu, pas à la sécurité.
 */
export function threadPage(instance: string, project: string, description: string, discussions: Discussion[], scheme: ColorScheme): string {
  const md = gitlabMarkdown(instance, project);
  const render = (text: string) => md.parse(text) as string;
  const intro = description.trim() ? render(description) : '<p class="empty">Pas de description.</p>';
  const threads = discussions.map((d) => {
    const state =
      d.resolved === null ? '' : `<div class="state">${d.resolved ? '✓ Résolu' : '● Non résolu'}</div>`;
    const notes = d.notes
      .map(
        (n) =>
          `<div class="note"><div class="meta"><strong>${escape(n.author)}</strong> · ${escape(dateTime(n.createdAt))}</div>` +
          `<div class="body">${render(n.body)}</div></div>`,
      )
      .join('');
    const kind = d.resolved === null ? '' : d.resolved ? ' resolved' : ' open';
    return `<article class="thread${kind}">${state}${notes}</article>`;
  });
  const exchanges = threads.length
    ? `<h2 class="exchanges">Échanges · ${threads.length}</h2>${threads.join('')}`
    : '<h2 class="exchanges">Échanges</h2><p class="empty">Aucun échange.</p>';
  return markdownStyle(scheme) + THREAD_STYLE + intro + exchanges;
}
