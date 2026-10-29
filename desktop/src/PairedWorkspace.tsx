import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from 'react';
import type { PairedController } from './paired-application';
import { remoteIssues, type Device, type Task, type Workspace } from './paired-domain';
import { Avatar } from './Avatar';
import { Icon } from './Icon';
import { formatDate } from './format';
import type { RunTemplate } from '@botstral/link';
import { Routines, RoutineInbox } from './Routines';
import { Memory } from './Memory';

function WorkspaceDialog({
  label,
  onClose,
  children,
}: {
  label: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className="action-review"
      aria-label={label}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      {children}
    </dialog>
  );
}
function taskTerminal(task: Task) {
  return ['completed', 'failed', 'cancelled'].includes(task.state);
}
function TaskCard({
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
function Pairing({
  controller,
  local,
  onLocal,
}: {
  controller: PairedController;
  local: boolean;
  onLocal: () => void;
}) {
  const { snapshot, busy } = useSyncExternalStore(controller.subscribe, controller.getState);
  const [label, setLabel] = useState('My desktop');
  const candidate = snapshot?.candidate;
  return (
    <section className="pairing-panel">
      <h1>Pair this desktop</h1>
      <p>
        Connect to your manager with a native device identity. Its credential stays in the operating
        system keychain.
      </p>
      {!snapshot?.pairing && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void controller.run(() => controller.app.beginPairing(label));
          }}
        >
          <label>
            Device name
            <input
              value={label}
              maxLength={100}
              required
              onChange={(event) => setLabel(event.target.value)}
            />
          </label>
          <button className="primary-button" disabled={busy}>
            Create pairing request
          </button>
        </form>
      )}
      {snapshot?.pairing && (
        <>
          <h2>Device fingerprint</h2>
          <code className="fingerprint">{snapshot.pairing.fingerprint}</code>
          <p>
            Have your trusted local administrator authorize exactly this fingerprint for the owner
            role.
          </p>
          <div className="action-row">
            <button
              disabled={busy}
              onClick={() => {
                void controller.run(() => controller.app.importPairing());
              }}
            >
              Import invitation file
            </button>
            {local && (
              <button
                disabled={busy}
                onClick={() => {
                  void controller.run(() => controller.app.pairLocal());
                }}
              >
                Authorize with local manager
              </button>
            )}
          </div>
          <p className="muted small-copy">
            Local authorization opens a native chooser for the authenticated manager trust export.
            Invitation files must be protected with permissions 0600. Pairing links contain a
            short-lived invitation and must be handled privately.
          </p>
        </>
      )}
      {candidate && (
        <section className="trust-review">
          <h2>Confirm manager trust</h2>
          <p>Compare these values with your authenticated local export before continuing.</p>
          <dl>
            <dt>Manager identity</dt>
            <dd>{candidate.manager}</dd>
            <dt>Address</dt>
            <dd>{candidate.endpoint}</dd>
            <dt>Server name</dt>
            <dd>{candidate.server_name}</dd>
            <dt>Exact certificate pin</dt>
            <dd>{candidate.leaf}</dd>
            <dt>Role</dt>
            <dd>{candidate.role}</dd>
            <dt>Device fingerprint</dt>
            <dd>{candidate.fingerprint}</dd>
          </dl>
          <button
            className="primary-button"
            disabled={busy}
            onClick={() => {
              void controller.run(() => controller.app.confirmPairing(candidate.id));
            }}
          >
            Confirm and connect
          </button>
        </section>
      )}
      <button className="detail-action" onClick={onLocal}>
        Open trusted local administration
      </button>
    </section>
  );
}
function Administration({ controller }: { controller: PairedController }) {
  const [devices, setDevices] = useState<Device[]>([]);
  const [fingerprint, setFingerprint] = useState('');
  const [label, setLabel] = useState('Another desktop');
  const [issue, setIssue] = useState<string | null>(null);
  const load = async () => {
    try {
      setDevices(await controller.app.devices());
      setIssue(null);
    } catch {
      setIssue('Sign in through trusted local administration to manage devices.');
    }
  };
  return (
    <section className="details-section">
      <h3>Local device administration</h3>
      <button
        onClick={() => {
          void load();
        }}
      >
        List paired devices
      </button>
      {issue && <p role="alert">{issue}</p>}
      {devices.map((device) => (
        <div className="device-row" key={device.id}>
          <span>
            {device.label}
            <small>{device.revoked ? 'Revoked' : 'Authorized'}</small>
          </span>
          <button
            disabled={device.revoked}
            onClick={() => {
              void (async () => {
                try {
                  await controller.app.revokeDevice(device.id);
                  await load();
                } catch {
                  setIssue('Server revocation is unconfirmed. Refresh the device list.');
                }
              })();
            }}
          >
            Revoke device
          </button>
        </div>
      ))}
      <details>
        <summary>Invite another desktop</summary>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void controller.export(() => controller.app.issueInvitation(fingerprint, label));
          }}
        >
          <label>
            Device name
            <input required value={label} onChange={(event) => setLabel(event.target.value)} />
          </label>
          <label>
            Confirmed device fingerprint
            <input
              required
              pattern="[a-f0-9]{64}"
              value={fingerprint}
              onChange={(event) => setFingerprint(event.target.value)}
            />
          </label>
          <p className="small-copy muted">
            Choose the authenticated trust export, then a private destination for the invitation.
            Deliver the invitation privately.
          </p>
          <button>Save protected invitation</button>
        </form>
      </details>
    </section>
  );
}
function ExecutionSettings({
  workspace,
  bot,
  controller,
  busy,
  settings,
  change,
}: {
  workspace: Workspace;
  bot: Workspace['bots'][number];
  controller: PairedController;
  busy: boolean;
  settings: Settings;
  change: (value: Settings) => void;
}) {
  const session = workspace.sessions.find((s) => s.bot_id === bot.id);
  const worker = workspace.workers.find((w) => w.id === session?.worker_id);
  const permission = workspace.permissions.find((p) => p.bot === bot.id);
  return (
    <details className="execution-settings">
      <summary>Execution settings and prerequisites</summary>
      <p>
        Choose the guest account and limits deliberately. Every proposed effect still needs
        exact-action approval.
      </p>
      {!session || worker?.revoked ? (
        <>
          <p>
            {worker?.revoked
              ? 'The assigned worker was revoked. Choose an enrolled replacement.'
              : 'Connect this bot to an enrolled worker.'}
          </p>
          {workspace.workers
            .filter((w) => !w.revoked)
            .map((worker) => (
              <button
                key={worker.id}
                disabled={busy || worker.phase !== 'ready'}
                onClick={() => {
                  void controller.run(() => controller.app.openSession(bot.id, worker.id));
                }}
              >
                Use worker {worker.id.slice(0, 8)} · {worker.phase}
              </button>
            ))}
        </>
      ) : (
        <p className="small-copy">
          Worker {session.worker_id}. Session {session.status}. Input owner {session.owner}.
        </p>
      )}
      <button
        disabled={busy}
        onClick={() => {
          void controller.run(() =>
            controller.app.permission(bot.id, permission?.revision ?? 1, !permission?.allow),
          );
        }}
      >
        {permission?.allow ? 'Disable execution permission' : 'Allow reviewed execution'}
      </button>
      <div className="settings-grid">
        {(
          [
            ['uid', 'Guest user identifier', 1, 2147483647],
            ['gid', 'Guest group identifier', 1, 2147483647],
            ['seconds', 'Deadline in seconds', 10, 300],
            ['turns', 'Maximum model turns', 1, 8],
            ['tokens', 'Total token budget', 1, 1000000],
            ['output', 'Output tokens per turn', 1, 8192],
          ] satisfies [
            keyof Omit<Settings, 'runtime' | 'process' | 'cost'>,
            string,
            number,
            number,
          ][]
        ).map(([key, name, min, max]) => (
          <label key={key}>
            {name}
            <input
              type="number"
              required
              min={min}
              max={max}
              value={settings[key]}
              onChange={(event) => change({ ...settings, [key]: Number(event.target.value) })}
            />
          </label>
        ))}
        <label>
          Cost limit in US dollars
          <input
            type="number"
            required
            min={0.000001}
            max={10}
            step={0.000001}
            value={settings.cost / 1_000_000}
            onChange={(event) =>
              change({ ...settings, cost: Math.round(Number(event.target.value) * 1_000_000) })
            }
          />
        </label>
      </div>
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={settings.process}
          onChange={(event) => change({ ...settings, process: event.target.checked })}
        />
        Permit bounded Linux process proposals
      </label>
      <label>
        Approved browser runtime digest
        <input
          placeholder="64-character SHA-256, optional for process-only work"
          value={settings.runtime}
          pattern="[a-f0-9]{64}"
          onChange={(event) => change({ ...settings, runtime: event.target.value })}
        />
      </label>
      <p className="muted small-copy">
        Browser and file tools use the supported Linux Playwright profile, an 800 by 600 viewport,
        128 processes, 1 GiB of memory, and the deadline above. Output slots are text for result.txt
        and image for screenshot.png. Process tools use 8 processes and 64 MiB of memory. Both
        capture at most 1024 process output bytes. Use your administrator's approved worker account
        and installed runtime digest.
      </p>
    </details>
  );
}
type Settings = {
  uid: number;
  gid: number;
  seconds: number;
  turns: number;
  tokens: number;
  cost: number;
  output: number;
  runtime: string;
  process: boolean;
};
export function PairedWorkspace({
  controller,
  local,
  onLocal,
  onSignOut,
}: {
  controller: PairedController;
  local: boolean;
  onLocal: () => void;
  onSignOut?: () => void;
}) {
  const state = useSyncExternalStore(controller.subscribe, controller.getState);
  const snapshot = state.snapshot;
  const workspace = snapshot?.workspace;
  const [selected, setSelected] = useState<string | null>(null);
  const [text, setText] = useState('');
  const [settings, setSettings] = useState<Settings>({
    uid: 1000,
    gid: 1000,
    seconds: 120,
    turns: 4,
    tokens: 200000,
    cost: 200000,
    output: 1024,
    runtime: '',
    process: true,
  });
  const [details, setDetails] = useState(false);
  const [hiddenCandidate, setHiddenCandidate] = useState<string | null>(null);
  const bot = workspace?.bots.find((b) => b.id === selected) ?? workspace?.bots[0];
  useEffect(() => {
    if (bot && !workspace?.bots.some((item) => item.id === selected)) setSelected(bot.id);
  }, [bot, workspace?.bots, selected]);
  const session = workspace?.sessions.find((s) => s.bot_id === bot?.id);
  const worker = workspace?.workers.find((w) => w.id === session?.worker_id);
  const config = workspace?.configurations.find((c) => c.bot === bot?.id);
  const permission = workspace?.permissions.find((p) => p.bot === bot?.id);
  const unknown = snapshot?.intents.some((i) => i.status === 'unknown');
  const routineCommand = text.startsWith('/routine ');
  const executionBlocker =
    snapshot?.status !== 'online'
      ? 'Reconnect and recover the workspace before sending.'
      : !workspace?.vault_unlocked
        ? 'Unlock the manager vault in trusted local administration.'
        : !config?.ready
          ? 'Configure and verify a tool-capable inference model for this bot in trusted manager administration.'
          : !session
            ? 'Choose a worker in execution settings.'
            : worker?.revoked
              ? 'The assigned worker was revoked. Choose a replacement in execution settings.'
              : worker?.phase !== 'ready' || session.status !== 'confirmed'
                ? 'Wait for the worker and computer session to become ready.'
                : !permission?.allow
                  ? 'Allow reviewed execution in execution settings.'
                  : unknown
                    ? 'Resolve the saved unknown operation before submitting more work.'
                    : !settings.process && !/^[a-f0-9]{64}$/.test(settings.runtime)
                      ? 'Enable bounded process tools or enter the approved browser runtime digest.'
                      : null;
  const blocker = routineCommand
    ? snapshot?.status !== 'online'
      ? 'Reconnect to send owner commands.'
      : unknown
        ? 'Resolve the saved operation first.'
        : null
    : executionBlocker;
  const template: RunTemplate | null =
    session && config && (settings.process || /^[a-f0-9]{64}$/.test(settings.runtime))
      ? {
          session: session.id,
          session_revision: session.revision,
          configuration_revision: config.revision,
          permission_revision: permission?.revision ?? 1,
          prompt: '',
          duration_seconds: settings.seconds,
          max_turns: settings.turns,
          max_total_tokens: settings.tokens,
          max_cost_microusd: settings.cost,
          max_output_tokens: settings.output,
          max_operations: 16,
          execution_policy: {
            process: settings.process
              ? {
                  profile: 'linux_systemd_v1',
                  account: { uid: settings.uid, gid: settings.gid },
                  environment: {},
                  limits: {
                    runtime_seconds: settings.seconds,
                    output_bytes: 1024,
                    processes: 8,
                    memory_bytes: 64 * 1024 * 1024,
                  },
                }
              : null,
            computer: /^[a-f0-9]{64}$/.test(settings.runtime)
              ? {
                  profile: 'linux_playwright_v1',
                  account: { uid: settings.uid, gid: settings.gid },
                  runtime: { sha256: settings.runtime },
                  limits: {
                    runtime_seconds: settings.seconds,
                    output_bytes: 1024,
                    processes: 128,
                    memory_bytes: 1024 * 1024 * 1024,
                  },
                  viewport: { width: 800, height: 600 },
                  outputs: [
                    { slot: 'text', name: 'result.txt', media_type: 'text_plain', max_bytes: 8192 },
                    {
                      slot: 'image',
                      name: 'screenshot.png',
                      media_type: 'png',
                      max_bytes: 1024 * 1024,
                    },
                  ],
                }
              : null,
          },
        }
      : null;
  const issue = state.issue ?? snapshot?.issue;
  const runs = workspace?.runs.filter((r) => r.bot === bot?.id) ?? [];
  const linked = new Set(runs.flatMap((r) => r.turns.flatMap((t) => (t.task ? [t.task] : []))));
  return (
    <div className={`workspace paired-workspace${details ? ' with-details' : ''}`}>
      <aside className="roster" aria-label="Paired bot roster">
        <header className="roster-header" data-tauri-drag-region>
          <button
            className="circle-button"
            aria-label="Workspace details"
            onClick={() => setDetails(!details)}
          >
            <Icon name="panel" size={18} />
          </button>
        </header>
        <nav className="bot-list" aria-label="Paired bots">
          {workspace?.bots.map((item, index) => (
            <button
              key={item.id}
              className={`${index === 0 ? 'bot-tile' : 'bot-row'}${bot?.id === item.id ? ' selected' : ''}`}
              onClick={() => setSelected(item.id)}
            >
              <Avatar seed={item.id} primary={index === 0} />
              <span className="bot-row-text">
                <strong>{item.name}</strong>
                <small>Paired workspace</small>
              </span>
            </button>
          ))}
        </nav>
        <footer className="roster-footer">
          <p className="small-copy muted">
            {workspace?.owner ?? 'Native owner client'} ·{' '}
            {snapshot?.status.replaceAll('_', ' ') ?? 'Starting'}
          </p>
          <button className="detail-action" onClick={onLocal}>
            Local administration
          </button>
          <button
            className="detail-action"
            disabled={state.busy}
            onClick={() => {
              void controller.run(() => controller.app.refresh());
            }}
          >
            Refresh workspace
          </button>
          <button
            className="detail-action"
            onClick={() => {
              if (onSignOut) onSignOut();
              else void controller.signOut();
            }}
          >
            Sign out of this device
          </button>
        </footer>
      </aside>
      <main className="main-panel">
        <header className="conversation-header" data-tauri-drag-region>
          <div className="conversation-title">
            {bot && <Avatar seed={bot.id} primary />}
            <span>{bot?.name ?? 'Connect your workspace'}</span>
          </div>
          <span className="connection-label">
            {snapshot?.status.replaceAll('_', ' ') ?? 'Starting'}
          </span>
        </header>
        <div className="conversation paired-conversation">
          {issue && (
            <div className="banner error-banner" role="alert">
              {remoteIssues[issue] ??
                'The native operation could not finish. Refresh to recover current state.'}
            </div>
          )}
          {state.notice && (
            <div className="banner notice-banner" role="status">
              {state.notice}
            </div>
          )}
          {workspace && snapshot?.status !== 'online' && (
            <div className="banner offline-banner" role="status">
              Cached workspace. Actions require pinned authentication and complete recovery.
            </div>
          )}
          {workspace && (
            <RoutineInbox
              workspace={workspace}
              controller={controller}
              busy={state.busy || snapshot?.status !== 'online'}
            />
          )}
          {snapshot?.routine_panel.message && (
            <section className="routine-reply" aria-label="Owner command reply">
              <strong>Owner command reply</strong>
              <pre>{snapshot.routine_panel.message}</pre>
            </section>
          )}
          {!workspace ? (
            <Pairing controller={controller} local={local} onLocal={onLocal} />
          ) : bot ? (
            <>
              <Memory
                key={JSON.stringify([
                  workspace.manager,
                  workspace.owner,
                  snapshot.generation,
                  bot.id,
                ])}
                workspace={workspace}
                bot={bot}
                snapshot={snapshot}
                controller={controller}
                busy={state.busy}
              />
              <Routines
                workspace={workspace}
                bot={bot}
                snapshot={snapshot}
                template={template}
                controller={controller}
                busy={state.busy || snapshot.status !== 'online'}
              />
              <p className="thread-explanation">
                Messages start separate durable runs. Type /routine help for authenticated owner
                controls. Context is retained within each run. Earlier runs remain in history.
              </p>
              {!runs.length && (
                <div className="empty-chat">
                  <Avatar seed={bot.id} primary large />
                  <h1>Work with {bot.name}</h1>
                  <p>
                    Send a request after reviewing execution settings. Proposed actions appear here
                    for your approval.
                  </p>
                </div>
              )}
              {runs.map((run) => (
                <section key={run.id} className="run-thread" aria-label={`Run ${run.id}`}>
                  <header>
                    <span>{formatDate(run.created_at)}</span>
                    <span>
                      {run.phase.replaceAll('_', ' ')}
                      {run.reason ? ` · ${run.reason}` : ''}
                    </span>
                    {!['completed', 'cancelled', 'stopped', 'awaiting_attention'].includes(
                      run.phase,
                    ) && (
                      <button
                        disabled={state.busy || run.phase === 'cancelling'}
                        onClick={() => {
                          void controller.run(() => controller.app.cancelRun(run.id));
                        }}
                      >
                        Cancel run
                      </button>
                    )}
                  </header>
                  <p className="owner-message">{run.prompt}</p>
                  {run.turns.map((turn) => (
                    <div key={turn.id} className="assistant-turn">
                      {turn.text && <p className="assistant-message">{turn.text}</p>}
                      {turn.truncated && (
                        <button
                          disabled={state.busy || snapshot.status !== 'online'}
                          onClick={() =>
                            void controller.run(() => controller.app.runDetails(run.id))
                          }
                        >
                          Load full run text
                        </button>
                      )}
                      {turn.task &&
                        workspace.tasks
                          .filter((t) => t.id === turn.task)
                          .map((task) => (
                            <TaskCard
                              key={task.id}
                              task={task}
                              controller={controller}
                              busy={state.busy || snapshot.status !== 'online'}
                            />
                          ))}
                      {turn.result && (
                        <details>
                          <summary>Tool result</summary>
                          <pre>{turn.result}</pre>
                        </details>
                      )}
                    </div>
                  ))}
                </section>
              ))}
              {workspace.tasks
                .filter((t) => t.bot === bot.id && !linked.has(t.id))
                .map((task) => (
                  <TaskCard
                    key={task.id}
                    task={task}
                    controller={controller}
                    busy={state.busy || snapshot.status !== 'online'}
                  />
                ))}
              {snapshot.intents
                .filter(
                  (i) =>
                    i.status === 'unknown' || i.status === 'pending' || i.status === 'attention',
                )
                .map((intent) => (
                  <p key={intent.id} role="status" className="banner notice-banner">
                    {intent.description}. Identity {intent.identity}.{' '}
                    {intent.status === 'unknown'
                      ? 'Outcome unknown. The saved identity will be reconciled without replay.'
                      : intent.status === 'attention'
                        ? 'Left unresolved by you. The original effect may still finish.'
                        : 'Acknowledged. Waiting for authoritative outcome.'}
                    {intent.status === 'unknown' && (
                      <button
                        disabled={state.busy}
                        onClick={() => {
                          if (
                            window.confirm(
                              'This operation may already have committed. Continue with other work while retaining this unresolved identity? Submitting similar work can duplicate its effect.',
                            )
                          )
                            void controller.run(() =>
                              controller.app.acknowledgeUncertainty(intent.id),
                            );
                        }}
                      >
                        Leave unresolved and continue
                      </button>
                    )}
                  </p>
                ))}
            </>
          ) : (
            <p>Create a bot through local administration, then refresh this workspace.</p>
          )}
        </div>
        {workspace && bot && (
          <div className="paired-composer-wrap">
            <ExecutionSettings
              workspace={workspace}
              bot={bot}
              controller={controller}
              busy={state.busy || snapshot?.status !== 'online'}
              settings={settings}
              change={setSettings}
            />
            {blocker && (
              <p className="composer-blocker" role="status">
                {blocker}
              </p>
            )}
            <form
              className="composer"
              aria-label="Run composer"
              onSubmit={(event) => {
                event.preventDefault();
                if (blocker || !text.trim()) return;
                if (routineCommand) {
                  void controller.run(() => controller.app.routineText(bot.id, text));
                  setText('');
                  return;
                }
                if (!session) return;
                const prompt = text;
                void controller.run(() =>
                  controller.app.send({
                    bot: bot.id,
                    session: session.id,
                    text: prompt,
                    account: { uid: settings.uid, gid: settings.gid },
                    runtime: settings.runtime || null,
                    process: settings.process,
                    seconds: settings.seconds,
                    turns: settings.turns,
                    tokens: settings.tokens,
                    cost_microusd: settings.cost,
                    output_tokens: settings.output,
                  }),
                );
                setText('');
              }}
            >
              <button className="composer-plus" aria-label="Attachments unavailable" disabled>
                <Icon name="plus" size={18} />
              </button>
              <textarea
                aria-label="Message"
                placeholder={`Message ${bot.name}`}
                value={text}
                maxLength={4096}
                onChange={(event) => setText(event.target.value)}
              />
              <button className="primary-button" disabled={state.busy || !!blocker || !text.trim()}>
                Send
              </button>
            </form>
          </div>
        )}
      </main>
      {details && (
        <aside className="details-panel">
          <header className="details-header">
            <strong>Workspace details</strong>
            <button
              className="icon-button"
              aria-label="Close details"
              onClick={() => setDetails(false)}
            >
              <Icon name="close" size={16} />
            </button>
          </header>
          <div className="details-content">
            <p>Manager {workspace?.manager ?? snapshot?.candidate?.manager ?? 'Not paired'}</p>
            <p>Applied cursor {workspace?.cursor ?? 0}</p>
            <button
              disabled={state.busy}
              onClick={() => {
                void controller.run(() => controller.app.importTrust());
              }}
            >
              Import authenticated replacement trust
            </button>
            <button
              disabled={state.busy}
              onClick={() => {
                void controller.run(() =>
                  controller.app.lifecycle(snapshot?.status !== 'suspended'),
                );
              }}
            >
              {snapshot?.status === 'suspended' ? 'Resume connection' : 'Pause connection'}
            </button>
            <Administration controller={controller} />
            <button onClick={onLocal}>Open local administration</button>
          </div>
        </aside>
      )}
      {snapshot?.candidate && workspace && snapshot.candidate.id !== hiddenCandidate && (
        <WorkspaceDialog
          label="Confirm replacement manager trust"
          onClose={() => setHiddenCandidate(snapshot.candidate?.id ?? null)}
        >
          <button onClick={() => setHiddenCandidate(snapshot.candidate?.id ?? null)}>
            Close trust review
          </button>
          <Pairing controller={controller} local={local} onLocal={onLocal} />
        </WorkspaceDialog>
      )}
      {snapshot?.review && (
        <WorkspaceDialog
          label="Review exact action"
          onClose={() => {
            if (!state.busy) void controller.run(() => controller.app.refresh());
          }}
        >
          <h2 id="review-heading">Approve this exact action?</h2>
          <p>
            Review the normalized arguments and target. This approval expires in one minute and
            applies only to this immutable execution and permission revision{' '}
            {snapshot.review.permission_revision}.
          </p>
          <pre>{snapshot.review.action}</pre>
          <div className="action-row">
            <button
              disabled={state.busy}
              onClick={() => {
                const review = snapshot.review;
                if (review) void controller.run(() => controller.app.approve(review.id));
              }}
            >
              Approve exact action
            </button>
            <button
              disabled={state.busy}
              onClick={() => {
                void controller.run(() => controller.app.refresh());
              }}
            >
              Close review
            </button>
          </div>
        </WorkspaceDialog>
      )}
    </div>
  );
}
