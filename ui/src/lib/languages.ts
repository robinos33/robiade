import { sql as sqlLanguage } from '@codemirror/lang-sql';
import { StreamLanguage, type StreamParser } from '@codemirror/language';
import type { Extension } from '@codemirror/state';

/**
 * Coloration d'un fichier d'après son nom. Chaque langage est un import dynamique : il n'est
 * chargé qu'à l'ouverture du premier fichier qui le demande, pas au démarrage de Robiade.
 */
type Loader = () => Promise<Extension>;

const legacy = (parser: Promise<StreamParser<unknown>>): Promise<Extension> =>
  parser.then((p) => StreamLanguage.define(p));

const php: Loader = () => import('@codemirror/lang-php').then((m) => m.php());
const js = (options: { jsx?: boolean; typescript?: boolean }): Loader => () =>
  import('@codemirror/lang-javascript').then((m) => m.javascript(options));
const html: Loader = () => import('@codemirror/lang-html').then((m) => m.html());
const css: Loader = () => import('@codemirror/lang-css').then((m) => m.css());
const json: Loader = () => import('@codemirror/lang-json').then((m) => m.json());
const yaml: Loader = () => import('@codemirror/lang-yaml').then((m) => m.yaml());
const xml: Loader = () => import('@codemirror/lang-xml').then((m) => m.xml());
const markdown: Loader = () => import('@codemirror/lang-markdown').then((m) => m.markdown());
const rust: Loader = () => import('@codemirror/lang-rust').then((m) => m.rust());
/** Déjà chargé par la console DB : rien à importer à la demande. */
const sql: Loader = async () => sqlLanguage();
const shell: Loader = () => legacy(import('@codemirror/legacy-modes/mode/shell').then((m) => m.shell));
const toml: Loader = () => legacy(import('@codemirror/legacy-modes/mode/toml').then((m) => m.toml));
const docker: Loader = () => legacy(import('@codemirror/legacy-modes/mode/dockerfile').then((m) => m.dockerFile));
const properties: Loader = () => legacy(import('@codemirror/legacy-modes/mode/properties').then((m) => m.properties));
/** Twig n'a pas de mode à lui ; Jinja2, dont il reprend la syntaxe, s'en approche. */
const twig: Loader = () => legacy(import('@codemirror/legacy-modes/mode/jinja2').then((m) => m.jinja2));
const diff: Loader = () => legacy(import('@codemirror/legacy-modes/mode/diff').then((m) => m.diff));
const python: Loader = () => legacy(import('@codemirror/legacy-modes/mode/python').then((m) => m.python));
const go: Loader = () => legacy(import('@codemirror/legacy-modes/mode/go').then((m) => m.go));
const groovy: Loader = () => legacy(import('@codemirror/legacy-modes/mode/groovy').then((m) => m.groovy));
const nginx: Loader = () => legacy(import('@codemirror/legacy-modes/mode/nginx').then((m) => m.nginx));

const BY_EXTENSION: Record<string, Loader> = {
  php, phtml: php,
  js: js({}), mjs: js({}), cjs: js({}), jsx: js({ jsx: true }),
  ts: js({ typescript: true }), mts: js({ typescript: true }), cts: js({ typescript: true }),
  tsx: js({ jsx: true, typescript: true }),
  html, htm: html, svelte: html, vue: html,
  css, scss: css, less: css,
  json, jsonc: json, webmanifest: json,
  yml: yaml, yaml, neon: yaml,
  xml, xsd: xml, xlf: xml, xliff: xml, svg: xml, plist: xml,
  md: markdown, markdown,
  rs: rust,
  sql,
  sh: shell, bash: shell, zsh: shell,
  toml,
  dockerfile: docker,
  ini: properties, cfg: properties, conf: properties, properties, env: properties,
  twig,
  diff, patch: diff,
  py: python,
  go,
  groovy, gradle: groovy,
};

const BY_NAME: Record<string, Loader> = {
  dockerfile: docker, containerfile: docker,
  'composer.lock': json,
  'cargo.lock': toml,
  '.bashrc': shell, '.zshrc': shell, '.profile': shell,
  '.editorconfig': properties, '.gitconfig': properties, '.npmrc': properties,
  'nginx.conf': nginx,
  jenkinsfile: groovy,
};

/** Chargeur de la coloration de `name` (nom de fichier), `undefined` pour un texte sans coloration. */
export function languageOf(name: string): Loader | undefined {
  let lower = name.toLowerCase();
  // `phpunit.xml.dist`, `.env.dist` : le suffixe de modèle ne change pas le langage.
  for (const suffix of ['.dist', '.example', '.sample', '.template']) {
    if (lower.endsWith(suffix) && lower.length > suffix.length) lower = lower.slice(0, -suffix.length);
  }
  const byName = BY_NAME[lower];
  if (byName) return byName;
  if (lower === '.env' || lower.startsWith('.env.')) return properties;
  const dot = lower.lastIndexOf('.');
  return dot > 0 ? BY_EXTENSION[lower.slice(dot + 1)] : undefined;
}
