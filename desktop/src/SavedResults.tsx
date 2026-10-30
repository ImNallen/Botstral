import { useEffect, useRef, useSyncExternalStore } from 'react';
import type { PairedController } from './paired-application';
import { remoteIssues } from './paired-domain';
import { TaskCard } from './TaskCard';

export function SavedResults({ controller }: { controller: PairedController }) {
  const { snapshot, library, source, busy, issue, notice } = useSyncExternalStore(
    controller.subscribe,
    controller.getState,
  );
  const ref = useRef<HTMLDialogElement>(null);
  const sourceRef = useRef<HTMLElement>(null);
  useEffect(() => {
    if (source.kind === 'ready') sourceRef.current?.focus();
  }, [source]);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  if (library.kind === 'closed') return null;
  const { query } = library;
  const bots = snapshot?.workspace?.bots ?? [];
  const update = (change: Partial<typeof query>) => {
    void controller.library({ ...query, after: null, ...change });
  };
  return (
    <dialog
      ref={ref}
      className="action-review saved-results"
      aria-label="Saved results"
      onCancel={(event) => {
        event.preventDefault();
        controller.closeLibrary();
      }}
    >
      <header className="action-row">
        <h2>Saved results</h2>
        <button onClick={controller.closeLibrary}>Close saved results</button>
      </header>
      {(issue || notice) && (
        <div className="saved-results-feedback">
          {issue && (
            <div className="banner error-banner" role="alert">
              {remoteIssues[issue] ??
                'The native operation could not finish. Refresh to recover current state.'}
            </div>
          )}
          {notice && (
            <div className="banner notice-banner" role="status">
              {notice}
            </div>
          )}
        </div>
      )}
      <p className="small-copy muted">
        Files are sorted by name. Saving verifies the file is available. Refresh to include newly
        saved files.
      </p>
      <div className="action-row">
        <label>
          Result bot
          <select
            value={query.bot ?? ''}
            onChange={(event) => {
              const bot = bots.find((b) => b.id === event.target.value)?.id ?? null;
              update({ bot });
            }}
          >
            <option value="">All bots</option>
            {bots.map((bot) => (
              <option key={bot.id} value={bot.id}>
                {bot.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          File name contains
          <input
            value={query.name}
            maxLength={80}
            onChange={(event) => {
              if (/^[ -~]*$/.test(event.target.value)) update({ name: event.target.value });
            }}
          />
        </label>
        <button onClick={() => update({})}>Refresh results</button>
      </div>
      {library.kind === 'loading' && <p role="status">Loading saved results.</p>}
      {library.kind === 'failed' && (
        <p role="alert">
          {remoteIssues[library.issue] ?? 'Saved results could not be read. Refresh to retry.'}
        </p>
      )}
      {library.kind === 'ready' && (
        <>
          {library.results.page.entries.length === 0 && (
            <p>No saved results match these filters.</p>
          )}
          <ul className="saved-result-list">
            {library.results.page.entries.map((entry) => (
              <li key={entry.id}>
                <strong>{entry.output.declaration.name}</strong>
                <small className="saved-artifact-id">Artifact {entry.id}</small>
                <p className="small-copy">
                  {bots.find((b) => b.id === entry.bot)?.name ?? `Retained bot ${entry.bot}`} ·{' '}
                  {entry.output.state.type === 'unavailable'
                    ? entry.output.state.reason.replaceAll('_', ' ')
                    : entry.output.state.type}
                </p>
                <div className="action-row">
                  <button
                    onClick={() => {
                      void controller.sourceTask(entry.task);
                    }}
                  >
                    Source task
                  </button>
                  <button
                    disabled={busy || entry.output.state.type !== 'available'}
                    onClick={() => {
                      void controller.export(() =>
                        controller.app.exportArtifact(entry.task, entry.id),
                      );
                    }}
                  >
                    Save verified file
                  </button>
                </div>
              </li>
            ))}
          </ul>
          <button
            disabled={!library.results.page.next}
            onClick={() => {
              void controller.library({ ...query, after: library.results.page.next });
            }}
          >
            Next results page
          </button>
        </>
      )}
      {source.kind === 'loading' && <p role="status">Loading source task.</p>}
      {source.kind === 'failed' && (
        <p role="alert">{remoteIssues[source.issue] ?? 'The source task could not be read.'}</p>
      )}
      {source.kind === 'ready' && (
        <section ref={sourceRef} tabIndex={-1} aria-label="Retained source task">
          <h3>Source task</h3>
          <p className="small-copy">
            Task {source.source.task.id} · Bot {source.source.task.bot}
          </p>
          <TaskCard task={source.source.task} controller={controller} busy={busy} />
        </section>
      )}
    </dialog>
  );
}
