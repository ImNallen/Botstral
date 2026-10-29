import { useEffect, useRef, useState } from 'react';
import type { PairedController } from './paired-application';
import type { MemoryDocument, RemoteSnapshot, Workspace } from './paired-domain';

type DraftBase =
  | {
      readonly kind: 'creation';
      readonly sourceMessage: Workspace['memory_sources'][number]['id'] | null;
    }
  | { readonly kind: 'existing'; readonly document: MemoryDocument };
type Draft = { readonly base: DraftBase; readonly markdown: string };
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
function sameBase(current: MemoryDocument | undefined, base: DraftBase) {
  return base.kind === 'creation' ? !current : sameDocument(current, base.document);
}
function matchesDraft(
  current: MemoryDocument | undefined,
  draft: Draft,
  bot: MemoryDocument['bot'],
) {
  if (draft.base.kind === 'creation') {
    return (
      draft.base.sourceMessage !== null &&
      sameDocument(current, {
        bot,
        kind: 'memory',
        revision: 1,
        markdown: draft.markdown,
        sources: [{ type: 'message', id: draft.base.sourceMessage }],
      })
    );
  }
  const document = draft.base.document;
  return (
    Number.isSafeInteger(document.revision + 1) &&
    sameDocument(current, {
      ...document,
      revision: document.revision + 1,
      markdown: draft.markdown,
    })
  );
}
function SavedMemory({ document }: { document: MemoryDocument | undefined }) {
  if (!document)
    return <p>No saved Memory. Select a message and write a preference to create it.</p>;
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
  const sources = workspace.memory_sources.filter((source) => source.bot === bot.id);
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
  const dirty =
    editor.kind !== 'view' &&
    editor.markdown !== (editor.base.kind === 'existing' ? editor.base.document.markdown : '');
  const bytes = editor.kind === 'view' ? 0 : new TextEncoder().encode(editor.markdown).length;
  const changed = editor.kind !== 'view' && !sameBase(current, editor.base);
  const selectedMessage =
    editor.kind !== 'view' && editor.base.kind === 'creation' ? editor.base.sourceMessage : null;
  const sourceMissing =
    editor.kind !== 'view' &&
    editor.base.kind === 'creation' &&
    !sources.some((source) => source.id === selectedMessage);
  useEffect(() => {
    if (editor.kind === 'editing' && changed)
      setEditor({ kind: 'conflict', base: editor.base, markdown: editor.markdown });
  }, [editor, changed]);
  const uncertain = editor.kind === 'uncertain';
  async function save({ base, markdown }: Draft) {
    const draft = { base, markdown };
    setEditor({ kind: 'saving', ...draft });
    await controller.run(() => {
      if (base.kind === 'existing')
        return controller.app.writeMemory({
          bot: bot.id,
          expectedRevision: base.document.revision,
          markdown,
        });
      if (base.sourceMessage === null) throw new Error('memory_source_invalid');
      return controller.app.createMemory({
        bot: bot.id,
        sourceMessage: base.sourceMessage,
        markdown,
      });
    });
    if (!mounted.current) return;
    const state = controller.getState();
    const latest = state.snapshot?.workspace?.documents.find((document) => document.bot === bot.id);
    if (latest && matchesDraft(latest, draft, bot.id)) {
      setEditor({
        kind: 'view',
        message: {
          text:
            base.kind === 'creation'
              ? 'Current memory matches this creation'
              : 'Current memory matches this edit',
          document: latest,
        },
      });
    } else if (
      state.snapshot?.intents.some((intent) => ['unknown', 'pending'].includes(intent.status)) ||
      state.snapshot?.status !== 'online' ||
      ['unknown_outcome', 'offline'].includes(state.issue ?? '')
    ) {
      setEditor({ kind: 'uncertain', ...draft });
    } else if (!sameBase(latest, draft.base)) {
      setEditor({ kind: 'conflict', ...draft });
    } else {
      setEditor({
        kind: 'editing',
        ...draft,
        error:
          'The save was not confirmed. Your draft is retained. Review the workspace error before saving again.',
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
      if (editor.kind === 'uncertain' && editor.base.kind === 'creation') {
        const latest = loaded.workspace?.documents.find((document) => document.bot === bot.id);
        if (matchesDraft(latest, editor, bot.id) && latest) {
          setEditor({
            kind: 'view',
            message: { text: 'Current memory matches this creation', document: latest },
          });
        } else {
          setEditor(
            latest
              ? { kind: 'conflict', base: editor.base, markdown: editor.markdown }
              : {
                  kind: 'editing',
                  base: editor.base,
                  markdown: editor.markdown,
                  error:
                    'No matching saved Memory was found. Your draft is retained. Review it before saving again.',
                },
          );
        }
        return;
      }
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
                    base: { kind: 'existing', document: current },
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
                  setEditor({
                    kind: 'editing',
                    base: { kind: 'existing', document: current },
                    markdown: '',
                    error: null,
                  })
                }
              >
                Clear current memory
              </button>
            </div>
          )}
          {!current && (
            <>
              {sources.length === 0 && (
                <p>
                  No source messages are available for this bot. A conversation message is required
                  to create Memory.
                </p>
              )}
              <button
                disabled={blocked || sources.length === 0}
                onClick={() =>
                  setEditor({
                    kind: 'editing',
                    base: { kind: 'creation', sourceMessage: null },
                    markdown: '',
                    error: null,
                  })
                }
              >
                Create memory
              </button>
            </>
          )}
        </>
      ) : (
        <>
          {editor.base.kind === 'creation' && (
            <>
              <label className="memory-label" htmlFor="memory-source">
                Source message
              </label>
              <select
                id="memory-source"
                value={editor.base.sourceMessage ?? ''}
                disabled={editor.kind === 'saving' || uncertain}
                onChange={(event) => {
                  const source = sources.find((source) => source.id === event.target.value);
                  if (editor.kind === 'editing' || editor.kind === 'conflict')
                    setEditor({
                      ...editor,
                      base: { kind: 'creation', sourceMessage: source?.id ?? null },
                    });
                }}
              >
                <option value="">Select a source message</option>
                {sourceMissing && editor.base.sourceMessage !== null && (
                  <option value={editor.base.sourceMessage} disabled>
                    Selected message is unavailable
                  </option>
                )}
                {sources.map((source) => (
                  <option key={source.id} value={source.id}>
                    {source.text || '(Blank message)'} ({source.id})
                  </option>
                ))}
              </select>
              {sourceMissing && editor.base.sourceMessage !== null && (
                <p role="alert">
                  The selected message is no longer available for this bot. Your draft is retained.
                  Select a source before saving.
                </p>
              )}
              {sources.length === 0 && (
                <p>
                  No source messages are available for this bot. A conversation message is required
                  to create Memory.
                </p>
              )}
            </>
          )}
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
                    blocked ||
                    !dirty ||
                    bytes > 4096 ||
                    changed ||
                    sourceMissing ||
                    editor.kind === 'conflict'
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
                        base: { kind: 'existing', document: current },
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
