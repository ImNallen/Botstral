# Conversational memory

Ask a bot to list, create, update, or delete an individual or shared memory. Individual entries belong to that bot. Shared entries are available to all bots in the installation. The conversation shows the persisted text and scope after a successful change. It also shows list results if the next model reply fails. There is no memory viewer or editor.

Conversation starts with manager-only tools. A worker, computer session, and computer permission are unnecessary for memory chat. Owner authentication, an unlocked Vault, verified inference configuration, and run budgets still apply. Enabling worker tools retains the existing session and approval checks.

## Tool contract

`memory` accepts a strict `MemoryProposal`: `list`, `create`, `update`, or `delete`, with explicit `individual` or `shared` scope. Each mutation affects one entry. Update and delete require the entry identifier and expected revision from a list result. A conflict saves nothing. The model cannot select another bot or supply provenance.

The manager resolves ownership from the admitted run and records `MemorySource::Run { bot, run, turn }`. Imported entries retain their original source references. `ManagerOutcome` carries typed results into conversation messages independently of model prose. Proposed arguments never produce a committed notice.

Entries contain nonblank Markdown up to 4,096 UTF-8 bytes. Each scope holds at most 16 live entries and 8 KiB of serialized context; combined context is limited to 16 KiB. The installation permits 256 canonical entry files, including tombstones. Limits refuse changes without truncating text.

## Persistence and recovery

Private root-level `entry-<id>.md` files are authoritative. A metadata header holds identity, resolved scope, revision, and source; the body holds Markdown. Deletion replaces the same file with a tombstone. These names do not match the legacy `memory-*.md` format. Instructions retain their existing document path and derived index. Memory listing scans the bounded entry collection directly.

Migration 12 adds a publication journal and nullable `runs.memory_entries_snapshot`. An intent commits the actual run, turn, attempt, and call identities, billed response and accounting, expected prior digest, exact intended bytes, and typed receipt before file publication. The run waits for publication and cannot resend that provider call. Publication checks private files, syncs the temporary file, renames it, and syncs the directory. A second transaction finalizes the receipt and run result.

Recovery runs before dependent memory reads, new snapshots, or run dispatch. Matching intended bytes finalize the receipt. Matching prior bytes permit publication. Any other state requires attention and blocks dependent work without overwriting the file. Completed intents are never replayed to reconstruct memory. Cancellation, deadlines, configuration checks, duplicate-call checks, and budgets apply before intent admission. Cancellation afterward preserves the admitted write and its notice.

Startup leaves the Vault locked. Pending memory work waits for unlock so the manager can check the secret inventory before publishing or reading context. Legacy documents import once after unlock, using a deterministic identifier, original text, revision, and provenance. Blank documents produce no live entry. Originals move to root-level `legacy-memory-<bot>.md` archives. Import intents recover interrupted publication and archival. Obsolete owner `WriteDocument` Memory mutations are rejected. Readable stale desktop journals report uncertainty without replaying those writes.

New runs freeze an explicit entry array, including `[]`. A null new column selects the old frozen Document snapshot. Both columns null mean no memory. Later changes never enter an admitted run's frozen context. Removed historical source records do not invalidate that context. Transcript compaction preserves typed outcomes.

## Secret boundary

Known-secret checks cover the current encrypted Mistral provider key and retained connector credential epochs: remote bearer values and local connector environment values. Connector values are decrypted with their existing installation and connection bindings. These values are rejected in admission, publication, import, list results, frozen context, and provider dispatch. Public run views, retry responses, and cancellation responses withhold content that becomes sensitive after rotation. Rejected candidate values and secret-bearing provider response identifiers are not copied into manager failure receipts or journals. The desktop removes a run submission from its mutation journal after the manager refuses it.

Reflection checks include literal values and recursively decoded valid JSON strings, including quoted and Unicode-escaped forms. They do not identify arbitrary unknown text, every encoding, or provider keys no longer retained by the installation. Rotation does not erase historical files or backups. The current inventory prevents newly sensitive historical content from being sent or displayed through these memory paths.

Explicit secret-storage requests are directed to `request_vault_entry`. This tool accepts only the closed purposes `provider_key` and `reusable_login`; it has no value field. The provider-key result names the protected trusted local administration path, `PUT /v1/provider-keys/mistral`, requiring owner authentication and an unlocked Vault. Reusable login items remain unsupported until P06. The handoff saves no secret and never asks for its value in chat.

## Verification

Run `python3 scripts/verify-conversational-memory.py --full`. The script uses local recorded HTTP provider responses, manager settlement, native pairing, and real conversation components. It records commands, exit codes, and logs under `docs/evidence/conversational-memory/`.

For the interaction video, install Playwright outside the repository and pass its `index.mjs` path with `--playwright-module`. Set `BOTSTRAL_CHROME` to an installed Chrome executable, or install Playwright Chromium and its video dependencies. The video script also accepts `--baseline` to rebuild the accepted baseline components from Git without another checkout.

The [acceptance record](evidence/conversational-memory/acceptance.md) distinguishes component replay, native integration, debug builds, and unexecuted production environments. Historical evidence remains unchanged.
