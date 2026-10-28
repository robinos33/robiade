/** Entrée de démo : `/demo/?view=agents&dialog=resume`, pour `screens.sh`. */
import { emit, install } from './mock';

install();
const { app } = await import('../src/lib/state.svelte');
await import('../src/main');

const params = new URLSearchParams(location.search);
await new Promise((r) => setTimeout(r, 300));
app.view = (params.get('view') ?? 'inbox') as typeof app.view;
if (params.get('project')) app.selectProject(`/Users/demo/code/${params.get('project')}`);
if (params.get('dialog') === 'resume') app.resumeOpen = true;
if (params.get('view') === 'agents') app.openAgent('a1');

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
emit('agent:output', { id: 'a1', data: btoa(bin) });
