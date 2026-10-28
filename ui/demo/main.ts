/** Entrée de démo : `/demo/?view=agents&dialog=resume`, pour `screens.sh`. */
import { EditorView } from '@codemirror/view';
import { AGENT_IDS, emit, install } from './mock';

install();
const { app } = await import('../src/lib/state.svelte');
await import('../src/main');

const params = new URLSearchParams(location.search);
await new Promise((r) => setTimeout(r, 300));
app.view = (params.get('view') ?? 'inbox') as typeof app.view;
if (params.get('project')) app.selectProject(`/Users/demo/code/${params.get('project')}`);
if (params.get('dialog') === 'resume') app.resumeOpen = true;
if (params.get('view') === 'agents') app.openAgent(AGENT_IDS[0]!);

/** `?view=db&explore=Boutique prod&table=order&cell=5` : ouvre l'explorateur comme le ferait un clic. */
async function find(selector: string, text: string): Promise<HTMLElement | undefined> {
  for (let i = 0; i < 20; i++) {
    const found = [...document.querySelectorAll<HTMLElement>(selector)].find((e) => e.textContent?.includes(text));
    if (found) return found;
    await new Promise((r) => setTimeout(r, 100));
  }
}
async function click(selector: string, text: string) {
  (await find(selector, text))?.click();
}
// `?view=agents&asset=regles-promo.md` : ouvre un asset de l'agent affiché.
if (params.get('asset')) await click('.asset', params.get('asset')!);
if (params.get('explore')) {
  const row = await find('li.row', params.get('explore')!);
  row?.querySelector<HTMLElement>('.actions button')?.click();
  await click('.dialog button.danger', 'Ouvrir');
  if (params.get('table')) await click('.relation', params.get('table')!);
  if (params.get('cell')) await click('.rows .cell', `"promo"`);
  // `&console=<sql>` : ouvre la console, y écrit la requête et l'exécute.
  if (params.get('console')) {
    await click('.tabs button', 'Console');
    const content = await find('.cm-content', '');
    const view = content && EditorView.findFromDOM(content);
    if (view) {
      const sql = params.get('console')!;
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: sql }, selection: { anchor: sql.length } });
      await click('.bar button', 'Exécuter');
      // `&typed=1` : tape le nom de la connexion dans la confirmation de prod, puis exécute.
      if (params.get('typed')) {
        const field = await find('.confirm input', '');
        const name = document.querySelector('.confirm strong')?.textContent ?? '';
        if (field instanceof HTMLInputElement) {
          field.value = name;
          field.dispatchEvent(new Event('input', { bubbles: true }));
          await new Promise((r) => setTimeout(r, 50));
          await click('.dialog button.danger', 'Exécuter');
        }
      }
    }
  }
}

const ESC = '\x1b[';
const c = (code: string, text: string) => `${ESC}${code}m${text}${ESC}0m`;
const lines = [
  `${c('38;2;198;160;246', '>')} Un client ne doit pouvoir appliquer qu'un seul code promo par commande.`,
  `  Ajoute la règle côté domaine, avec les tests.`,
  '',
  `${c('38;2;166;218;149', '⏺')} Je regarde comment le panier applique les codes aujourd'hui.`,
  '',
  `${c('38;2;166;218;149', '⏺')} ${c('1', 'Read')}(src/Cart/Cart.php)`,
  `  ⎿  Read 184 lines`,
  `${c('38;2;166;218;149', '⏺')} ${c('1', 'Grep')}(pattern: "applyPromo", path: "src")`,
  `  ⎿  Found 3 files`,
  '',
  `${c('38;2;166;218;149', '⏺')} La règle vit dans ${c('38;2;138;173;244', 'Cart::applyPromo()')}, mais rien n'empêche un deuxième`,
  `  appel. Je l'extrais dans une ${c('38;2;138;173;244', 'PromoCodePolicy')} injectée, testable seule.`,
  '',
  `${c('38;2;166;218;149', '⏺')} ${c('1', 'Write')}(src/Cart/PromoCodePolicy.php)`,
  `  ⎿  Wrote 61 lines`,
  `${c('38;2;166;218;149', '⏺')} ${c('1', 'Edit')}(src/Cart/PromoCode.php)`,
  `  ⎿  ${c('38;2;166;218;149', '+42')} ${c('38;2;237;135;150', '-7')}`,
  `${c('38;2;166;218;149', '⏺')} ${c('1', 'Bash')}(vendor/bin/phpunit tests/Cart)`,
  `  ⎿  OK (24 tests, 61 assertions)`,
  '',
  `${c('38;2;238;212;159', '✻')} ${c('38;2;238;212;159', 'Vérification des cas limites…')} ${c('2', '(42s · ↓ 3.1k tokens)')}`,
  '',
  c('2', '─'.repeat(100)),
  `${c('1', '>')} `,
  c('2', '─'.repeat(100)),
  `  ${c('38;2;245;169;127', '⏵⏵ accept edits on')} ${c('2', '(shift+tab to cycle)')}`,
];
const bytes = new TextEncoder().encode(lines.join('\r\n'));
let bin = '';
bytes.forEach((b) => (bin += String.fromCharCode(b)));
emit('agent:output', { id: AGENT_IDS[0]!, data: btoa(bin) });
