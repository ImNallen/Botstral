# Legacy import identifier correction

The automated review on [PR 47](https://github.com/ImNallen/Botstral/pull/47) identified a real renderer compatibility failure. Legacy import used raw hash bytes as a UUID, leaving its version and variant bits unconstrained. Bot `00000000-0000-4000-8000-000000000001` produced `63d45adc-66fd-1a0e-ddc7-0dd9a90605bc`. The desktop rejected that identifier and therefore the complete workspace snapshot.

The correction uses `uuid::Builder::from_custom_bytes` to set standard version 8 and variant bits while retaining deterministic hashing. The same bot now produces `63d45adc-66fd-8a0e-9dc7-0dd9a90605bc`.

The manager regression imports an actual legacy document, checks its resulting identifier, and verifies repeated reads preserve the entry. Its captured `ManagerOutcome` is checked in as [the renderer fixture](../../../desktop/fixtures/legacy-memory-outcome.json). The desktop regression passes that outcome through the complete `remoteSnapshotSchema` boundary.

## Verification

- The [manager regression before correction](../../../.audit/pr47-monitor/legacy-before.log) failed with `Microsoft` instead of `RFC4122`.
- The [renderer regression before correction](../../../.audit/pr47-monitor/desktop-before.log) failed with `expected false to be true` for snapshot acceptance.
- The [manager regression after correction](../../../.audit/pr47-monitor/legacy-after.log) passed.
- The [renderer regression after correction](../../../.audit/pr47-monitor/desktop-after.log) passed all five conversation cases.
- The [focused memory suite](../../../.audit/pr47-monitor/memory-tests.log) passed 27 tests, including import and publication recovery.
- [All frontend tests](../../../.audit/pr47-monitor/frontend-tests.log) passed 33 tests, with the existing opt-in projection case skipped. Type checking, generated binding checks, Rust formatting, and strict manager Clippy also passed.

Regenerate the captured outcome with this command before running the renderer regression:

```sh
BOTSTRAL_LEGACY_MEMORY_OUTCOME="$PWD/desktop/fixtures/legacy-memory-outcome.json" cargo test -p botstral-manager --locked --lib legacy_import_identifiers_are_renderer_compatible_and_stable
npm run test --workspace @botstral/desktop -- src/conversational-memory.test.tsx
```

The earlier full repository and desktop verification records remain evidence for their named source revision. This correction has the focused verification above and triggers fresh pull-request checks.
