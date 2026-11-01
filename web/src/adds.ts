import type { AddOutcome } from './contract'

/** An add sent while `before` was the last outcome, waiting for its own. */
export interface PendingAdd {
  moduleId: string
  before: number
}

/** The outcome counter in `added`, to start a `PendingAdd` from. */
export const addSeq = (added: AddOutcome | null | undefined) => added?.seq ?? 0

/**
 * Whether `added` is the outcome of `pending`. Every settings message carries
 * the last outcome, so one from an earlier add of the same module (a refused
 * one, say) can arrive after the retry is sent; its count gives it away.
 */
export function isOutcomeOf(added: AddOutcome | null | undefined, pending: PendingAdd): added is AddOutcome {
  return !!added && added.module_id === pending.moduleId && addSeq(added) > pending.before
}
