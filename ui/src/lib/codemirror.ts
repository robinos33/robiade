import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { EditorView } from '@codemirror/view';
import { tags } from '@lezer/highlight';

/** Couleurs du thème de l'application, claires comme sombres. */
const highlight = HighlightStyle.define([
  { tag: tags.keyword, color: 'var(--accent)', fontWeight: '600' },
  { tag: [tags.string, tags.special(tags.string)], color: 'var(--ok)' },
  { tag: [tags.number, tags.bool, tags.null], color: 'var(--info)' },
  { tag: [tags.comment, tags.lineComment, tags.blockComment], color: 'var(--ink-3)', fontStyle: 'italic' },
  { tag: [tags.typeName, tags.standard(tags.name)], color: 'var(--attention)' },
  { tag: tags.operator, color: 'var(--ink-2)' },
]);

/** Coloration et habillage communs aux éditeurs CodeMirror de Robiade. */
export const robiadeTheme = [
  syntaxHighlighting(highlight),
  EditorView.theme({
    '&': { height: '100%', fontSize: '12.5px', backgroundColor: 'var(--bg-sunken)', color: 'var(--ink)' },
    '.cm-scroller': { fontFamily: 'var(--mono)', lineHeight: '1.55' },
    '.cm-content': { caretColor: 'var(--ink)' },
    '.cm-gutters': { backgroundColor: 'var(--bg-sunken)', color: 'var(--ink-3)', border: 'none' },
    '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'var(--bg-hover)' },
    // Ligne citée par un agent (`src/foo.rs:42`), ouverte depuis son terminal.
    '.cm-citedLine': { backgroundColor: 'var(--accent-soft)', boxShadow: 'inset 2px 0 var(--accent)' },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': { backgroundColor: 'var(--accent-soft)' },
    '&.cm-focused': { outline: 'none' },
  }),
];
