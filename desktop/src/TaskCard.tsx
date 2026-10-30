import type { PairedController } from './paired-application';
import type { Task } from './paired-domain';

function taskTerminal(task: Task) {
  return ['completed', 'failed', 'cancelled'].includes(task.state);
}
export function TaskCard({
  task,
  controller,
  busy,
}: {
  task: Task;
  controller: PairedController;
  busy: boolean;
}) {
  const act = (action: () => ReturnType<typeof controller.app.refresh>) => {
    void controller.run(action);
  };
  return (
    <article className="task-card" aria-label={`Task ${task.id}`} id={`task-${task.id}`}>
      <header>
        <strong>Task activity</strong>
        <span className="state-label">{task.state.replaceAll('_', ' ')}</span>
      </header>
      <p className="small-copy muted">
        {task.progress} operations reported.{' '}
        {task.state === 'cancelling'
          ? 'Cancellation requested. Waiting for authoritative stop evidence.'
          : ''}
      </p>
      <details>
        <summary>Exact action and target</summary>
        <pre>{task.action}</pre>
      </details>
      {task.evidence !== '[]' && (
        <details>
          <summary>Execution evidence</summary>
          <pre>{task.evidence}</pre>
        </details>
      )}
      <div className="action-row">
        {!taskTerminal(task) && !task.approval_consumed && (
          <button disabled={busy} onClick={() => act(() => controller.app.review(task.id))}>
            Review action
          </button>
        )}
        {!taskTerminal(task) && (
          <button
            disabled={busy || task.state === 'cancelling'}
            onClick={() => act(() => controller.app.cancelTask(task.id))}
          >
            Cancel task
          </button>
        )}
        {!taskTerminal(task) && !task.approval_revoked && (
          <button disabled={busy} onClick={() => act(() => controller.app.revokeApproval(task.id))}>
            Revoke approval
          </button>
        )}
      </div>
      {task.outputs.map((output) => (
        <div key={output.declaration.slot} className="artifact-row">
          <span>
            {output.declaration.name}
            <small>
              {output.state.type === 'available'
                ? `${output.state.content.length} bytes · ${output.state.content.sha256}`
                : output.state.type === 'unavailable'
                  ? output.state.reason.replaceAll('_', ' ')
                  : output.state.type}
            </small>
          </span>
          {output.state.type === 'available' && (
            <button
              disabled={busy}
              onClick={() => {
                const state = output.state;
                if (state.type === 'available')
                  void controller.export(() => controller.app.exportArtifact(task.id, state.id));
              }}
            >
              Save verified file
            </button>
          )}
        </div>
      ))}
    </article>
  );
}
