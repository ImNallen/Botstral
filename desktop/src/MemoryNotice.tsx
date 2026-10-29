import type { ManagerOutcome, MemoryScope, MemoryRefusal } from '@botstral/link';
import { LoginEntry } from './LoginEntry';

function scopeName(scope: MemoryScope) {
  return scope.type === 'shared' ? 'Shared memory for all bots' : 'Individual memory for this bot';
}
const refusals: Record<MemoryRefusal, string> = {
  invalid_input: 'The memory request was invalid. No change was saved.',
  not_found: 'That memory was not found in the selected scope. No change was saved.',
  revision_conflict:
    'The memory changed since it was read. No change was saved. Ask the bot to list it again.',
  capacity: 'The memory limit was reached. No change was saved.',
  protected_content: 'Protected content was withheld. Use protected Vault entry for secrets.',
  needs_attention: 'Memory publication needs attention. Its outcome is unresolved.',
};

export function MemoryNotice({
  outcome,
  result,
}: {
  outcome: ManagerOutcome;
  result: string | null;
}) {
  if (outcome.type === 'vault') {
    return (
      <article className="memory-notice" aria-label="Vault handoff">
        <p>{result}</p>
        {outcome.outcome.type === 'login_entry' && <LoginEntry bot={outcome.outcome.bot} />}
      </article>
    );
  }
  const memory = outcome.outcome;
  switch (memory.type) {
    case 'created':
    case 'updated':
      return (
        <article className="memory-notice" aria-label="Committed memory change">
          <strong>
            {memory.type === 'created' ? 'Saved' : 'Updated'}. {scopeName(memory.entry.scope)}.
          </strong>
          <p className="saved-memory-text">{memory.entry.markdown}</p>
        </article>
      );
    case 'deleted':
      return (
        <article className="memory-notice" aria-label="Committed memory change">
          <strong>Deleted. {scopeName(memory.scope)}.</strong>
        </article>
      );
    case 'listed':
      return (
        <article className="memory-notice" aria-label="Memory list result">
          <strong>{scopeName(memory.scope)}.</strong>
          {memory.entries.length === 0 ? (
            <p>No saved entries in this scope.</p>
          ) : (
            memory.entries.map((entry) => (
              <div key={entry.id}>
                <p className="saved-memory-text">{entry.markdown}</p>
              </div>
            ))
          )}
        </article>
      );
    case 'refused':
      return (
        <p className="memory-notice" role="status">
          {refusals[memory.reason]}
        </p>
      );
    default: {
      const exhaustive: never = memory;
      return exhaustive;
    }
  }
}
