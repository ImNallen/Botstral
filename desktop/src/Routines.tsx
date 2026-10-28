import { useState } from 'react';
import type { RoutineAction, RoutineDefinition, RunTemplate } from '@botstral/link';
import type { PairedController } from './paired-application';
import type { RemoteSnapshot, Workspace } from './paired-domain';
import {
  occurrenceLabel,
  routineTriggerSchema,
  type RoutineTrigger,
  type RoutineView,
} from './routine-domain';

const date = (at: number) => new Date(at * 1000).toLocaleString();
function triggerLabel(trigger: RoutineTrigger): string {
  switch (trigger.type) {
    case 'once':
      return `Once at ${date(trigger.at)}`;
    case 'interval':
      return `Every ${trigger.seconds} seconds from ${date(trigger.anchor)} UTC anchor`;
    case 'daily':
      return `Daily at ${String(trigger.hour).padStart(2, '0')}:${String(trigger.minute).padStart(2, '0')} in ${trigger.timezone}`;
    case 'event':
      return `Publisher topic ${trigger.topic}`;
    default: {
      const exhaustive: never = trigger;
      return exhaustive;
    }
  }
}
export function RoutineInbox({
  workspace,
  controller,
  busy,
}: {
  workspace: Workspace;
  controller: PairedController;
  busy: boolean;
}) {
  if (!workspace.inbox.length) return null;
  return (
    <section className="routine-inbox" aria-label="Durable inbox">
      <h2>Inbox</h2>
      <p className="small-copy muted">
        Saved by your manager while this app is closed. Acknowledging an entry grants no approval.
      </p>
      {workspace.inbox.map((entry) => (
        <article className="routine-card" key={entry.id}>
          <strong>{occurrenceLabel(entry.status)}</strong>
          <small>{date(entry.created_at)}</small>
          {entry.message && <p>{entry.message}</p>}
          {entry.run && <p className="small-copy">Run {entry.run}</p>}
          <button
            disabled={busy}
            onClick={() =>
              void controller.run(() =>
                controller.app.routine({
                  id: crypto.randomUUID(),
                  action: { type: 'acknowledge_inbox', id: entry.id },
                }),
              )
            }
          >
            Acknowledge inbox entry
          </button>
        </article>
      ))}
    </section>
  );
}
function RoutineCard({
  routine,
  snapshot,
  controller,
  busy,
}: {
  routine: RoutineView;
  snapshot: RemoteSnapshot;
  controller: PairedController;
  busy: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [definition, setDefinition] = useState(routine.definition_json);
  const [history, setHistory] = useState(false);
  const act = (action: RoutineAction) =>
    void controller.run(() => controller.app.routine({ id: crypto.randomUUID(), action }));
  const entries = snapshot.routine_panel.history.filter((o) => o.routine === routine.id);
  const last = entries.at(-1);
  return (
    <article className="routine-card" aria-label={`Routine ${routine.name}`}>
      <header>
        <strong>{routine.name}</strong>
        <span className="state-label">{routine.state}</span>
      </header>
      <p>{triggerLabel(routine.trigger)}</p>
      <p className="small-copy">
        {routine.next_at
          ? `Next run ${date(routine.next_at)}`
          : routine.trigger.type === 'event'
            ? 'Waiting for an authenticated matching event.'
            : 'No scheduled run pending.'}
      </p>
      <div className="action-row">
        <button
          disabled={busy}
          onClick={() => {
            setDefinition(routine.definition_json);
            setEditing(!editing);
          }}
        >
          Edit routine
        </button>
        <button
          disabled={busy || routine.state !== 'enabled'}
          onClick={() => act({ type: 'test', id: routine.id, expected_revision: routine.revision })}
        >
          Run live bounded test
        </button>
        <button
          disabled={busy}
          onClick={() =>
            act({
              type: 'set_paused',
              id: routine.id,
              expected_revision: routine.revision,
              paused: routine.state === 'enabled',
            })
          }
        >
          {routine.state === 'enabled' ? 'Pause routine' : 'Resume routine'}
        </button>
        <button
          disabled={busy}
          onClick={() => {
            setHistory(true);
            void controller.run(() => controller.app.routineHistory(routine.id, null));
          }}
        >
          Routine history
        </button>
        <button
          disabled={busy}
          onClick={() =>
            void controller.run(() =>
              controller.app.routinePreview(routine.trigger, Math.floor(Date.now() / 1000)),
            )
          }
        >
          Preview next times
        </button>
        <button
          disabled={busy}
          onClick={() =>
            act({ type: 'delete', id: routine.id, expected_revision: routine.revision })
          }
        >
          Delete routine
        </button>
      </div>
      {editing && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void controller.run(() =>
              controller.app.saveRoutine(routine.id, routine.revision, definition),
            );
            setEditing(false);
          }}
        >
          <label>
            Approved routine definition
            <textarea
              aria-label={`Definition for ${routine.name}`}
              rows={14}
              value={definition}
              onChange={(event) => setDefinition(event.target.value)}
              maxLength={24576}
            />
          </label>
          <p className="small-copy">
            Review the schedule, instruction, pinned bindings, relative limits, and wakeup grant.
            Stale bindings block execution until you save an approved replacement.
          </p>
          <button disabled={busy}>Save routine changes</button>
        </form>
      )}
      {history && (
        <div aria-label={`History for ${routine.name}`}>
          {!entries.length && <p>No recorded occurrences.</p>}
          {entries.map((o) => (
            <p key={o.id}>
              {date(o.scheduled_at)}. {occurrenceLabel(o.status)}.{' '}
              {o.skipped_count > 0 ? `${o.skipped_count} earlier occurrences coalesced.` : ''}
              {o.run ? ` Run ${o.run}.` : ''}
            </p>
          ))}
          {last && entries.length === 32 && (
            <button
              disabled={busy}
              onClick={() =>
                void controller.run(() => controller.app.routineHistory(routine.id, last.id))
              }
            >
              Older occurrences
            </button>
          )}
        </div>
      )}
    </article>
  );
}
function Publishers({
  workspace,
  bot,
  controller,
  busy,
}: {
  workspace: Workspace;
  bot: Workspace['bots'][number];
  controller: PairedController;
  busy: boolean;
}) {
  const [name, setName] = useState('');
  const [topics, setTopics] = useState('');
  const [digest, setDigest] = useState('');
  return (
    <details>
      <summary>Publisher webhook topics</summary>
      <p className="small-copy">
        Install a publisher using the SHA-256 digest of a random bearer credential with at least 32
        bytes. Keep the credential in the publisher's protected configuration. Requests use your
        manager's public TLS address and the path shown below.
      </p>
      {workspace.publishers
        .filter((p) => p.bot === bot.id)
        .map((p) => (
          <article className="routine-card" key={p.id}>
            <strong>{p.name}</strong>
            <p>
              {p.revoked
                ? 'Revoked'
                : p.last_error
                  ? `Delivery failed: ${p.last_error}`
                  : p.last_received_at
                    ? `Last received ${date(p.last_received_at)}`
                    : 'Waiting for publisher'}
            </p>
            <p className="small-copy">/events/v1/publishers/{p.id}</p>
            <p>Topics. {p.topics.join(', ')}.</p>
            <button
              disabled={busy || p.revoked}
              onClick={() =>
                void controller.run(() =>
                  controller.app.routine({
                    id: crypto.randomUUID(),
                    action: { type: 'revoke_publisher', id: p.id, expected_revision: p.revision },
                  }),
                )
              }
            >
              Revoke publisher
            </button>
          </article>
        ))}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void controller.run(() =>
            controller.app.routine({
              id: crypto.randomUUID(),
              action: {
                type: 'install_publisher',
                id: crypto.randomUUID(),
                bot: bot.id,
                name,
                topics: topics
                  .split(',')
                  .map((t) => t.trim())
                  .filter(Boolean),
                credential_sha256: digest,
              },
            }),
          );
        }}
      >
        <label>
          Publisher name
          <input required maxLength={100} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label>
          Allowed topics separated by commas
          <input required value={topics} onChange={(e) => setTopics(e.target.value)} />
        </label>
        <label>
          Credential SHA-256 digest
          <input
            required
            pattern="[a-f0-9]{64}"
            value={digest}
            onChange={(e) => setDigest(e.target.value)}
            autoComplete="off"
          />
        </label>
        <button disabled={busy}>Install authenticated publisher</button>
      </form>
    </details>
  );
}
function copyName(name: string): string {
  const encoder = new TextEncoder();
  let prefix = '';
  let bytes = 0;
  for (const character of name) {
    const length = encoder.encode(character).length;
    if (bytes + length > 95) break;
    prefix += character;
    bytes += length;
  }
  return `${prefix} copy`;
}
export function Routines({
  workspace,
  bot,
  snapshot,
  template,
  controller,
  busy,
}: {
  workspace: Workspace;
  bot: Workspace['bots'][number];
  snapshot: RemoteSnapshot;
  template: RunTemplate | null;
  controller: PairedController;
  busy: boolean;
}) {
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('');
  const [prompt, setPrompt] = useState('');
  const [kind, setKind] = useState('daily');
  const [clock, setClock] = useState('09:00');
  const [zone, setZone] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone);
  const [seconds, setSeconds] = useState(3600);
  const [instant, setInstant] = useState(new Date(Date.now() + 3600000).toISOString().slice(0, 16));
  const [publisher, setPublisher] = useState('');
  const [topic, setTopic] = useState('');
  const [field, setField] = useState('');
  const [value, setValue] = useState('');
  const [catchup, setCatchup] = useState(300);
  const [wakeups, setWakeups] = useState(false);
  const [maxRuns, setMaxRuns] = useState(workspace.routine_budget?.max_runs ?? 100);
  const [maxTokens, setMaxTokens] = useState(workspace.routine_budget?.max_tokens ?? 2000000);
  const [maxCost, setMaxCost] = useState(workspace.routine_budget?.max_cost_microusd ?? 1000000);
  const [issue, setIssue] = useState<string | null>(null);
  const trigger = (): RoutineTrigger => {
    const parts = clock.split(':');
    return routineTriggerSchema.parse(
      kind === 'daily'
        ? { type: 'daily', hour: Number(parts[0]), minute: Number(parts[1]), timezone: zone }
        : kind === 'interval'
          ? { type: 'interval', anchor: Math.floor(Date.now() / 1000) + 60, seconds }
          : kind === 'once'
            ? { type: 'once', at: Math.floor(new Date(instant + 'Z').getTime() / 1000) }
            : { type: 'event', publisher, topic, equals: field ? { [field]: value } : {} },
    );
  };
  const act = (action: RoutineAction) =>
    void controller.run(() => controller.app.routine({ id: crypto.randomUUID(), action }));
  const routines = workspace.routines.filter((r) => r.bot === bot.id);
  return (
    <details className="routines-panel" open={creating || routines.length > 0}>
      <summary>Routines and follow-ups</summary>
      <p className="small-copy">
        Your manager runs saved work while clients are closed. Live tests use real budgets and
        approvals. Preview performs no work.
      </p>
      <p className="small-copy muted">
        Daily gap times move to the first valid minute. Repeated times run once at the earlier
        instant. Missed work coalesces within the catchup window. Unresolved work skips overlap.
        Resume never backfills paused time.
      </p>
      <div className="action-row">
        <button disabled={busy} onClick={() => setCreating(!creating)}>
          Create routine
        </button>
        <button
          disabled={busy}
          onClick={() =>
            void controller.run(() => controller.app.routineText(bot.id, '/routine help'))
          }
        >
          Conversation command help
        </button>
      </div>
      <details>
        <summary>Cumulative owner budget</summary>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            act({
              type: 'set_budget',
              budget: { max_runs: maxRuns, max_tokens: maxTokens, max_cost_microusd: maxCost },
            });
          }}
        >
          <p className="small-copy">
            All routines and wakeup chains share these lifetime ceilings. Editing a ceiling does not
            reset spent or reserved amounts.
          </p>
          <div className="settings-grid">
            <label>
              Total runs
              <input
                type="number"
                min={1}
                max={1000000}
                value={maxRuns}
                onChange={(e) => setMaxRuns(Number(e.target.value))}
              />
            </label>
            <label>
              Total tokens
              <input
                type="number"
                min={1}
                value={maxTokens}
                onChange={(e) => setMaxTokens(Number(e.target.value))}
              />
            </label>
            <label>
              Total cost in micro USD
              <input
                type="number"
                min={1}
                value={maxCost}
                onChange={(e) => setMaxCost(Number(e.target.value))}
              />
            </label>
          </div>
          <button disabled={busy}>Save owner budget</button>
        </form>
      </details>
      <Publishers workspace={workspace} bot={bot} controller={controller} busy={busy} />
      {creating && (
        <form
          className="routine-create"
          onSubmit={(event) => {
            event.preventDefault();
            try {
              if (!template) {
                setIssue('Configure a computer session and verified model first.');
                return;
              }
              const definition: RoutineDefinition = {
                bot: bot.id,
                name,
                trigger: trigger(),
                template: { ...template, prompt },
                catchup_seconds: catchup,
                wakeups: wakeups
                  ? {
                      horizon_seconds: 86400,
                      minimum_spacing_seconds: 300,
                      max_pending: 2,
                      max_total: 10,
                    }
                  : null,
              };
              act({ type: 'create', id: crypto.randomUUID(), definition });
              setCreating(false);
              setIssue(null);
            } catch {
              setIssue('Review the schedule and publisher fields.');
            }
          }}
        >
          <label>
            Routine name
            <input
              required
              maxLength={100}
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <label>
            Routine instruction
            <textarea
              required
              maxLength={kind === 'event' ? 2048 : 4096}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
            />
          </label>
          <label>
            Trigger
            <select value={kind} onChange={(e) => setKind(e.target.value)}>
              <option value="daily">Daily in a time zone</option>
              <option value="interval">UTC anchored interval</option>
              <option value="once">One time</option>
              <option value="event">Publisher topic subscription</option>
            </select>
          </label>
          {kind === 'daily' && (
            <div className="settings-grid">
              <label>
                Local time
                <input
                  type="time"
                  required
                  value={clock}
                  onChange={(e) => setClock(e.target.value)}
                />
              </label>
              <label>
                IANA time zone
                <input required value={zone} onChange={(e) => setZone(e.target.value)} />
              </label>
            </div>
          )}
          {kind === 'interval' && (
            <label>
              Interval in seconds
              <input
                type="number"
                min={60}
                max={31536000}
                value={seconds}
                onChange={(e) => setSeconds(Number(e.target.value))}
              />
            </label>
          )}
          {kind === 'once' && (
            <label>
              UTC date and time
              <input
                type="datetime-local"
                required
                value={instant}
                onChange={(e) => setInstant(e.target.value)}
              />
            </label>
          )}
          {kind === 'event' && (
            <>
              <label>
                Authenticated publisher
                <select required value={publisher} onChange={(e) => setPublisher(e.target.value)}>
                  <option value="">Select publisher</option>
                  {workspace.publishers
                    .filter((p) => p.bot === bot.id && !p.revoked)
                    .map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                </select>
              </label>
              <label>
                Named topic
                <input required value={topic} onChange={(e) => setTopic(e.target.value)} />
              </label>
              <div className="settings-grid">
                <label>
                  Optional matching field
                  <input value={field} onChange={(e) => setField(e.target.value)} />
                </label>
                <label>
                  Exact string value
                  <input value={value} onChange={(e) => setValue(e.target.value)} />
                </label>
              </div>
            </>
          )}
          <label>
            Catchup window in seconds
            <input
              type="number"
              min={0}
              max={86400}
              value={catchup}
              onChange={(e) => setCatchup(Number(e.target.value))}
            />
          </label>
          <label>
            <input
              type="checkbox"
              checked={wakeups}
              onChange={(e) => setWakeups(e.target.checked)}
            />
            Allow at most ten cumulative wakeups, two pending, five minutes apart, within one day
          </label>
          <p className="small-copy">
            This routine pins the current session, permission revision, model revision, and
            execution settings. Each run uses a fresh relative deadline.
          </p>
          {!workspace.routine_budget && (
            <p>
              Save a cumulative owner budget before execution. Missing budgets produce a blocked
              inbox entry.
            </p>
          )}
          <div className="action-row">
            <button
              type="button"
              disabled={busy}
              onClick={() => {
                try {
                  void controller.run(() =>
                    controller.app.routinePreview(trigger(), Math.floor(Date.now() / 1000)),
                  );
                } catch {
                  setIssue('Review the trigger fields.');
                }
              }}
            >
              Preview schedule without running
            </button>
            <button disabled={busy || !template}>Save enabled routine</button>
          </div>
          {issue && <p role="alert">{issue}</p>}
        </form>
      )}
      {snapshot.routine_panel.preview.length > 0 && (
        <p aria-label="Schedule preview">
          Next times. {snapshot.routine_panel.preview.map(date).join('; ')}.
        </p>
      )}
      {routines.map((r) => (
        <RoutineCard
          key={r.id}
          routine={r}
          snapshot={snapshot}
          controller={controller}
          busy={busy}
        />
      ))}
      <details>
        <summary>Bot lifecycle</summary>
        <p className="small-copy">
          Copies contain paused definitions and require new bindings. Deletion stops new work and
          requests cancellation; dispatched effects still need reconciliation.
        </p>
        <div className="action-row">
          <button
            disabled={busy}
            onClick={() =>
              act({
                type: 'duplicate_bot',
                bot: bot.id,
                copy: crypto.randomUUID(),
                name: copyName(bot.name),
              })
            }
          >
            Duplicate bot with paused routines
          </button>
          <button disabled={busy} onClick={() => act({ type: 'delete_bot', bot: bot.id })}>
            Delete bot and stop work
          </button>
        </div>
      </details>
    </details>
  );
}
