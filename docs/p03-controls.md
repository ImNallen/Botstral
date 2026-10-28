# Control durable work

Upgrade the manager and paired clients together for owner protocol version three. Existing protected desktop caches and journals remain readable.

Open **Routines and follow-ups** in a paired bot workspace. Save a cumulative owner budget, then create a routine. Choose a daily IANA schedule, a UTC interval, a one-time UTC instant, or a publisher topic. Review the current execution settings before saving.

Use **Preview schedule without running** to calculate times. **Run live bounded test** admits real work under the saved permissions, approval rules, overlap policy, and budgets. Pause prevents future admissions. Resume schedules from the current time and never backfills paused time.

Use **Edit routine** to review the complete saved definition. Run and nested Vibe deadlines are relative durations in seconds. Session identity, session revision, permission revision, and model configuration revision remain pinned. To repair a stale binding, save an explicitly reviewed replacement. Missing readiness creates a blocked occurrence and inbox entry.

## Conversation commands

Type `/routine help` in the owner composer. These are authenticated owner commands. Assistant text cannot issue them.

- `/routine list` lists the bot's definitions.
- `/routine create <RoutineDefinition JSON>` creates a saved definition. The request identity also identifies retried creation.
- `/routine edit <id> <revision> <RoutineDefinition JSON>` replaces a definition.
- `/routine test <id> <revision>` starts a live bounded test.
- `/routine pause <id> <revision>` pauses future work.
- `/routine resume <id> <revision>` resumes without backfill.
- `/routine history <id>` reads recent occurrences.
- `/routine delete <id> <revision>` deletes the live definition.

Copy the current definition from the app editor when using the JSON commands. Every mutation checks its expected revision. The native journal preserves request identities across a lost reply or restart.

## Publisher webhook topics

Open **Publisher webhook topics**. Generate a random bearer credential containing at least 32 bytes of entropy in the publisher's protected configuration. Install its lowercase SHA-256 digest in the app. The credential itself stays outside conversation and routine definitions.

Send `POST /events/v1/publishers/<publisher-id>` to the manager's public TLS address. Set `Authorization: Bearer <credential>` and `Content-Type: application/json`. Use this envelope with a current Unix timestamp.

```json
{
  "id": "build-42",
  "timestamp": 1793200000,
  "topic": "build.done",
  "fields": { "branch": "main", "success": true }
}
```

Subscribe a routine to an allowed named topic. Matching uses at most eight exact scalar equality conditions. Payloads contain at most sixteen scalar fields. Body size is limited to 8192 bytes. A publisher can send sixty authenticated requests per minute. Events must be no more than twenty-four hours old or five minutes in the future.

A successful response follows the SQLite receipt commit. Repeating identical bytes with the same publisher and event identifier returns that receipt. Changed bytes conflict. Receipts remain durable; freshness limits reject old replay attempts. Paused subscriptions do not backfill acknowledged deliveries. Revoking a publisher fences admission and future dispatch. A locked vault returns a retryable service-unavailable response.

## Inbox and lifecycle

The inbox loads saved results, failures, and decisions after reconnect. Acknowledging an entry grants no execution approval. Exact task approval remains a separate reviewed action. Full run text can be loaded when a workspace preview is truncated.

**Duplicate bot with paused routines** creates fresh definitions without history, sessions, secrets, grants, or reservations. Configure new bindings before activation. **Delete bot and stop work** tombstones the bot and requests cancellation. Deletion cannot undo an already dispatched external effect.

Wakeups require the saved owner grant. Descendants share its cumulative limits and the owner's budget. Changing a budget ceiling does not erase spent or reserved amounts. Inbox recovery does not provide native push delivery while the app is closed.
