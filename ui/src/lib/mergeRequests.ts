import type { MergeRequest, MrAction, PipelineStatus } from './types';

/** Actions proposées : le domaine revérifie tout au moment de l'aperçu. */
export function actionsOf(mr: MergeRequest, mine: boolean): { label: string; action: MrAction }[] {
  const out: { label: string; action: MrAction }[] = [{ label: 'Commenter', action: { kind: 'comment', body: '' } }];
  if (!mine) return [...out, { label: 'Approuver', action: { kind: 'approve' } }];
  if (mr.pipeline === 'failed' || mr.pipeline === 'canceled') out.push({ label: 'Relancer', action: { kind: 'retryPipeline' } });
  if (mr.mergeStatus === 'need_rebase' || mr.hasConflicts) out.push({ label: 'Rebaser', action: { kind: 'rebase' } });
  if (mr.state === 'draft') out.push({ label: 'Prête', action: { kind: 'markReady' } });
  else out.push({ label: 'Draft', action: { kind: 'markDraft' } });
  if (mr.state === 'opened' && mr.mergeStatus === 'mergeable') {
    out.push({ label: mr.wagon ? 'Merger (sans squash)' : 'Merger', action: { kind: 'merge', expectedSha: mr.sha } });
  }
  return out;
}

/** Libellés de `detailed_merge_status` ; une valeur inconnue s'affiche telle quelle. */
export const MERGE_STATUS: Record<string, string> = {
  mergeable: 'prête à merger',
  not_approved: 'approbation manquante',
  requested_changes: 'changements demandés',
  ci_must_pass: 'pipeline requis',
  ci_still_running: 'pipeline en cours',
  discussions_not_resolved: 'discussions ouvertes',
  draft_status: 'draft',
  need_rebase: 'rebase nécessaire',
  conflict: 'conflits',
  blocked_status: 'bloquée par une autre MR',
  not_open: 'fermée',
  checking: 'vérification…',
  unchecked: 'non vérifiée',
};

export const PIPELINE: Record<PipelineStatus, { glyph: string; label: string }> = {
  success: { glyph: '✓', label: 'Pipeline réussi' },
  failed: { glyph: '✕', label: 'Pipeline en échec' },
  running: { glyph: '◐', label: 'Pipeline en cours' },
  manual: { glyph: '▸', label: 'Pipeline en attente d’un job manuel' },
  canceled: { glyph: '–', label: 'Pipeline annulé' },
};
