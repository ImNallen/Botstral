import { useEffect, useRef, useState } from 'react';
import type { PairedController } from './paired-application';
import type { MemoryDocument, RemoteSnapshot, Workspace } from './paired-domain';

type Draft = { readonly base: MemoryDocument; readonly markdown: string };
type Editor =
  | { kind: 'view'; message: { text: string; document: MemoryDocument | null } | null }
  | ({ kind: 'editing'; error: string | null } & Draft)
  | ({ kind: 'saving' | 'uncertain' | 'conflict' } & Draft);

function sameDocument(left: MemoryDocument | undefined, right: MemoryDocument) {
  return (
    left?.bot === right.bot &&
    left.kind === right.kind &&
    left.revision === right.revision &&
    left.markdown === right.markdown &&
    JSON.stringify(left.sources) === JSON.stringify(right.sources)
  );
}
function matchesEdit(current: MemoryDocument | undefined, draft: Draft) {
  return (
    Number.isSafeInteger(draft.base.revision + 1) &&
    sameDocument(current, {
      ...draft.base,
      revision: draft.base.revision + 1,
      markdown: draft.markdown,
    })
  );
}
function SavedMemory({ document }: { document: MemoryDocument | undefined }) {
  if (!document)
    return <p>No saved Memory. Initial authoring requires trusted manager administration.</p>;
  return (
    <>
      <p>Saved revision {document.revision}</p>
      {document.markdown === '' ? (
        <p>Saved Memory is blank.</p>
      ) : (
        <pre className="memory-text">{document.markdown}</pre>
      )}
      <h4>Provenance</h4>
      <ol className="memory-sources">
        {document.sources.map((source, index) => (
          <li key={index}>
            {source.type === 'external'
              ? `External reference: ${source.reference}`
              : `${source.type === 'task' ? 'Task' : 'Message'} ${source.id}`}
          </li>
        ))}
      </ol>
    </>
  );
}

export function Memory({
  workspace,
  bot,
  snapshot,
  controller,
  busy,
}: {
  workspace: Workspace;
  bot: Workspace['bots'][number];
  snapshot: RemoteSnapshot;
  controller: PairedController;
  busy: boolean;
}) {
  const current = workspace.documents.find((document) => document.bot === bot.id);
  const [editor, setEditor] = useState<Editor>({ kind: 'view', message: null });
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const unresolved = snapshot.intents.some((intent) => intent.status === 'unknown');
  const online = snapshot.status === 'online';
  const blocked = busy || !online || unresolved;
  const dirty = editor.kind !== 'view' && editor.markdown !== editor.base.markdown;
  const bytes = editor.kind === 'view' ? 0 : new TextEncoder().encode(editor.markdown).length;
  const changed = editor.kind !== 'view' && !sameDocument(current, editor.base);
  useEffect(() => {
    if (editor.kind === 'editing' && changed)
      setEditor({ kind: 'conflict', base: editor.base, markdown: editor.markdown });
  }, [editor, changed]);
  const uncertain = editor.kind === 'uncertain';
  async function save({ base, markdown }: Draft) {
    const draft = { base, markdown };
    setEditor({ kind: 'saving', ...draft });
    await controller.run(() =>
      controller.app.writeMemory({
        bot: bot.id,
        expectedRevision: draft.base.revision,
        markdown: draft.markdown,
      }),
    );
    if (!mounted.current) return;
    const state = controller.getState();
    const latest = state.snapshot?.workspace?.documents.find((document) => document.bot === bot.id);
    if (latest && matchesEdit(latest, draft)) {
      setEditor({
        kind: 'view',
        message: { text: 'Current memory matches this edit', document: latest },
      });
    } else if (
      state.snapshot?.intents.some((intent) => ['unknown', 'pending'].includes(intent.status)) ||
      state.snapshot?.status !== 'online' ||
      ['unknown_outcome', 'offline'].includes(state.issue ?? '')
    ) {
      setEditor({ kind: 'uncertain', ...draft });
    } else if (!sameDocument(latest, draft.base)) {
      setEditor({ kind: 'conflict', ...draft });
    } else {
      setEditor({
        kind: 'editing',
        ...draft,
        error:
          'The edit was not confirmed. Your draft is retained. Review the workspace error before saving again.',
      });
    }
  }
  async function loadCurrent() {
    let loaded: RemoteSnapshot | undefined;
    await controller.run(async () => {
      const value = await controller.app.refresh();
      loaded = value;
      return value;
    });
    if (
      mounted.current &&
      loaded === controller.getState().snapshot &&
      loaded?.status === 'online' &&
      !loaded.intents.some((intent) => intent.status === 'unknown')
    ) {
      setEditor({
        kind: 'view',
        message: {
          text: 'Loaded current Memory. Review it before starting another edit.',
          document: null,
        },
      });
    }
  }
  return (
    <details className="memory-panel">
      <summary>Memory</summary>
      <p className="small-copy muted">
        New runs use saved edits. Historical admitted snapshots remain unchanged. Switching bots or
        owner sessions discards unsent Memory drafts.
      </p>
      <SavedMemory document={current} />
      {editor.kind === 'view' ? (
        <>
          {editor.message &&
            (!editor.message.document || sameDocument(current, editor.message.document)) && (
              <p role="status">{editor.message.text}</p>
            )}
          {current && (
            <div className="action-row">
              <button
                disabled={blocked}
                onClick={() =>
                  setEditor({
                    kind: 'editing',
                    base: current,
                    markdown: current.markdown,
                    error: null,
                  })
                }
              >
                Edit memory
              </button>
              <button
                disabled={blocked || current.markdown === ''}
                onClick={() =>
                  setEditor({ kind: 'editing', base: current, markdown: '', error: null })
                }
              >
                Clear current memory
              </button>
            </div>
          )}
        </>
      ) : (
        <>
          <label className="memory-label" htmlFor="memory-draft">
            Memory draft
          </label>
          <textarea
            id="memory-draft"
            rows={7}
            value={editor.markdown}
            disabled={editor.kind === 'saving' || uncertain}
            aria-describedby="memory-size memory-edit-status"
            onChange={(event) => {
              if (editor.kind === 'editing' || editor.kind === 'conflict')
                setEditor({ ...editor, markdown: event.target.value });
            }}
          />
          <p id="memory-size" role={bytes > 4096 ? 'alert' : undefined}>
            {bytes} / 4096 UTF-8 bytes.{bytes > 4096 ? ' Shorten the draft before saving.' : ''}
          </p>
          <div id="memory-edit-status" role="status" aria-live="polite">
            {editor.kind === 'saving'
              ? 'Saving Memory. Waiting for current state.'
              : uncertain
                ? 'Save outcome is uncertain. Your draft is retained. Refresh to reconcile the saved operation. If you leave it unresolved, load current Memory before editing again.'
                : changed || editor.kind === 'conflict'
                  ? 'Memory changed since this draft began. The current version is shown above. Review your draft against it before saving.'
                  : editor.kind === 'editing' && editor.error
                    ? editor.error
                    : dirty
                      ? 'Unsent Memory draft.'
                      : 'No changes to save.'}
          </div>
          <div className="action-row">
            {(editor.kind === 'editing' || editor.kind === 'conflict') && (
              <>
                <button
                  disabled={
                    blocked || !dirty || bytes > 4096 || changed || editor.kind === 'conflict'
                  }
                  onClick={() => void save(editor)}
                >
                  Save memory
                </button>
                {editor.kind === 'conflict' && current && (
                  <button
                    disabled={blocked}
                    onClick={() =>
                      setEditor({
                        kind: 'editing',
                        base: current,
                        markdown: editor.markdown,
                        error: null,
                      })
                    }
                  >
                    Review draft against current
                  </button>
                )}
                <button
                  onClick={() =>
                    setEditor({
                      kind: 'view',
                      message: { text: 'Unsent Memory draft discarded.', document: null },
                    })
                  }
                >
                  Discard draft
                </button>
              </>
            )}
            {uncertain && (
              <button disabled={blocked} onClick={() => void loadCurrent()}>
                Load current memory
              </button>
            )}
          </div>
        </>
      )}
      {!online && (
        <p role="status">
          Saving Memory is unavailable while disconnected. Your draft is retained. Reconnecting does
          not save it.
        </p>
      )}
      {unresolved && (
        <p role="status">Resolve the unknown operation before editing or saving Memory.</p>
      )}
    </details>
  );
}
