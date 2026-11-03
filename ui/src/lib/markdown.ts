import { marked } from 'marked';
import type { ColorScheme } from './theme.svelte';

/** Feuille de style du Markdown rendu : l'iframe n'hérite ni de celle de Robiade, ni de son thème. */
export const markdownStyle = (scheme: ColorScheme) => `<style>
  :root { color-scheme: ${scheme}; }
  body { font: 14px/1.6 -apple-system, BlinkMacSystemFont, sans-serif; max-width: 78ch; margin: 24px auto; padding: 0 24px; }
  h1, h2, h3 { line-height: 1.25; margin: 1.4em 0 0.5em; }
  h1 { font-size: 1.6em; } h2 { font-size: 1.3em; } h3 { font-size: 1.1em; }
  code { font: 12.5px ui-monospace, SFMono-Regular, Menlo, monospace; background: rgb(127 127 127 / 0.15); padding: 1px 4px; border-radius: 4px; }
  pre { background: rgb(127 127 127 / 0.12); padding: 12px 14px; border-radius: 8px; overflow-x: auto; }
  pre code { background: none; padding: 0; }
  table { border-collapse: collapse; } th, td { border: 1px solid rgb(127 127 127 / 0.35); padding: 4px 10px; text-align: left; }
  blockquote { margin: 0; padding-left: 14px; border-left: 3px solid rgb(127 127 127 / 0.4); opacity: 0.85; }
  img { max-width: 100%; }
</style>`;

/** Page HTML d'un Markdown, à afficher dans une iframe `sandbox=""` : rien de ce qu'il contient ne s'exécute. */
export function markdownPage(text: string, scheme: ColorScheme): string {
  return markdownStyle(scheme) + marked.parse(text, { async: false, gfm: true });
}
