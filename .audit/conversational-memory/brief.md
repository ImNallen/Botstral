# Conversational memory implementation

The owner wants the bot to be the gateway for memory and most everyday interactions. Implement the memory behavior in DESIGN.md and PLAN.md. Individual memory belongs to one bot. Shared memory is available to all bots in the installation. Owners ask the bot to list, create, update, and delete memories. There is no dedicated memory viewer or editor. The bot reports what it saved and its scope after confirmed persistence. Secrets use protected Vault entry and stay out of memory and model inputs. Do not expand this into conversational management for every unrelated subsystem.

## Grounding

The current Document and WriteDocument in link/src/durable.rs require a BotId and support Instructions and Memory. Manager state/memory.rs owns authoritative Markdown, source validation, revision checks, and a rebuildable SQLite search index. State/runs.rs freezes one per-bot document in runs.memory_snapshot. Legacy runs with no snapshot never reload current memory. Admission and follow-up reflect-secret checks guard provider secrets.

ToolProposal in link/src/run.rs includes computer, process, Vibe, connector, and request_wakeup. Provider schemas and parsing are in manager/src/manager/runs/tools.rs and provider.rs. State/runs/settlement.rs executes request_wakeup as a manager tool and persists ManagerToolResult in the same settlement transaction. Worker tool calls have separate exact-action approval boundaries. Run operation and model budgets apply to the whole run.

desktop/src/PairedWorkspace.tsx mounts Memory.tsx. Desktop contract and native mutation-journal code support create_memory and write_memory. These existing editor entry points should go away once the conversational replacement works. Protected provider-key entry exists in local administration. Reusable Vault login items remain unfinished under P06, so do not claim a general password vault exists. Use the genuine supported protected flow and make unsupported handoffs explicit.

Historical evidence in docs/p01-*.md describes already implemented slices. Preserve it and add accurate scope for this feature.

## Required evidence

- Conversation tool definitions reach the provider and real manager settlement, not a prompt keyword parser.
- List and mutation work in both scopes with explicit scope and truthful durable notices.
- Revision conflicts, malformed tools, duplicates, replay, cancellation, limits, and restart cannot cause silent overwrites or repeated writes.
- Individual reads and writes cannot cross bot boundaries. Shared memory is deliberate and survives restart.
- New runs use both applicable scopes. Existing admitted snapshots and legacy no-context behavior remain intact.
- Known stored secrets and secret-storage requests cannot publish secret values to memory. Vault handoff never solicits values in ordinary chat.
- The shipped workspace has no memory viewer or editor. The conversation shows committed memory changes and clear handoff outcomes.
- Appropriate manager, schema/bindings, desktop, native, formatting, and recovery checks pass. Record an interaction video through real components and recorded-provider manager flows if packaged execution is unavailable. Label that limit accurately.

## Design deliverable

Produce a concise Markdown package with a current-flow explanation, caller usage, named types, signatures, storage and migration contract, transaction/replay behavior, secret handling, desktop behavior, and test plan. Consider at least two structurally distinct designs, such as extending scoped documents versus separate scoped memory entries. Recommend the smallest safe design. Read /Users/nallen/.codex/plugins/cache/pstack-claude/pstack/0.9.111/skills/poteto-mode/SKILL.md and architect/references/design-red-flags.md. Read source before proposing changes. Do not implement code or spawn agents.
