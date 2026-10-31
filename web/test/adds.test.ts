// Run with `pnpm test` (node --test). Node strips the types itself.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { addSeq, isOutcomeOf } from '../src/adds.ts'

test('an add waits for its own outcome, not an earlier one', () => {
  const refused = { module_id: 'github', error: 'already in the config', seq: 3 }
  // Retried after the refusal: settings still carrying it are not the answer.
  const retry = { moduleId: 'github', before: addSeq(refused) }
  assert.equal(isOutcomeOf(refused, retry), false)
  assert.equal(isOutcomeOf({ module_id: 'github', seq: 4 }, retry), true)
  assert.equal(isOutcomeOf({ module_id: 'other', seq: 4 }, retry), false)
  assert.equal(isOutcomeOf(null, retry), false)
  // First add since the server started.
  assert.equal(isOutcomeOf({ module_id: 'github', seq: 1 }, { moduleId: 'github', before: addSeq(undefined) }), true)
})
