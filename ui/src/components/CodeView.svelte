<script lang="ts">
  import { defaultKeymap } from '@codemirror/commands';
  import { Compartment, EditorState } from '@codemirror/state';
  import { drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers } from '@codemirror/view';
  import { robiadeTheme } from '../lib/codemirror';
  import { languageOf } from '../lib/languages';

  /**
   * Texte en lecture seule, coloré d'après le nom du fichier. CodeMirror ne dessine que les lignes
   * visibles : un fichier de 1 Mo s'affiche sans ralentir. Le curseur reste, pour naviguer au
   * clavier et sélectionner ; rien ne s'écrit.
   */
  let { text, name }: { text: string; name: string } = $props();

  let host: HTMLDivElement | undefined = $state();

  $effect(() => {
    if (!host) return;
    const language = new Compartment();
    const view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: text,
        extensions: [
          EditorState.readOnly.of(true),
          lineNumbers(),
          highlightActiveLine(),
          highlightActiveLineGutter(),
          drawSelection(),
          keymap.of(defaultKeymap),
          robiadeTheme,
          language.of([]),
        ],
      }),
    });
    // Le texte s'affiche tout de suite ; la coloration suit, une fois son langage chargé.
    let alive = true;
    languageOf(name)?.().then(
      (extension) => alive && view.dispatch({ effects: language.reconfigure(extension) }),
      () => {
        /* langage indisponible : le texte reste lisible sans couleur */
      },
    );
    return () => {
      alive = false;
      view.destroy();
    };
  });
</script>

<div class="code" bind:this={host}></div>

<style>
  .code { flex: 1; min-width: 0; min-height: 0; overflow: hidden; }
</style>
