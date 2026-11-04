import type { ForgeProvider } from './types';

/** Nom d'un fournisseur de forge, tel que l'interface l'affiche. */
export const FORGE_NAMES: Record<ForgeProvider, string> = { gitlab: 'GitLab', github: 'GitHub' };

/** Nom de la forge en vigueur ; « la forge » sans réglage. */
export const forgeName = (provider: ForgeProvider | null | undefined): string =>
  provider ? FORGE_NAMES[provider] : 'la forge';

/** Préfixe du numéro d'une MR : `!12` sur GitLab, `#12` pour une pull request GitHub. */
export const mrPrefix = (provider: ForgeProvider | null | undefined): string => (provider === 'github' ? '#' : '!');
