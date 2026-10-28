# M06 design verdict

Use Sol as the implementation base, with Astra's immutable grant binding and single-use local permit semantics made explicit before coding. Sol and Astra preserve the existing task/process ownership; Luna does not.

Scores are 0–3, with 3 fully specified and 0 contradicting the constraint.

| Criterion | Astra | Sol | Luna |
| --- | ---: | ---: | ---: |
| One authority and dispatch revisions for bot and Vibe | 3 | 2 | 2 |
| No replay of uncertain effects | 3 | 3 | 2 |
| Outbound worker and one process slot preserved | 3 | 3 | 0 |
| Existing task/process lifecycle and small integration interface | 3 | 3 | 1 |
| Executable backup and installation acceptance | 3 | 3 | 2 |
| Total | 15 | 14 | 7 |

Sol is the better implementation map despite Astra's one-point lead: its boundary table names the exhaustive integration points, keeps owner-device administration at parity, binds run result interpretation to the effect ledger, and explicitly prevents Vibe continuing after an unknown effect. Astra is the stronger concise authority model. Neither candidate provides actual installation evidence; both correctly keep acceptance open until executed.

## Indispensable grafts and corrections

1. Add Astra's grant revision to the immutable tool binding or an equivalent approved snapshot. Sol's `BotConnectorGrants` and internal `ConnectorAuthority` contain it, but `ConnectorToolBinding`, `ConnectorToolPolicy`, and `ConnectorSpec` do not. Capturing the current revision only at gateway admission cannot reject a queued approved task after grant revocation and regrant. Persist the captured revision in the prepared turn and task/Vibe approval digest; compare it again at release. Prefer a per-bot, per-connection grant revision to avoid unrelated grant changes invalidating every connector.
2. Make Astra's local permit one-shot rule normative. Once launch material is released, duplicate gateway requests can retrieve status or submit saved results, never receive another executable permit. The helper persists invocation identity before launch; settlement retries cannot execute again. Sol's broader duplicate-outcome wording needs this explicit branch.
3. Preserve Astra's distinction between remote owner discovery and local discovery execution. Sol requires a Computer session and ordinary worker task even for remote discovery. This is coherent but unnecessary coupling. Remote discovery can use the same owner-authorized manager discovery ledger without inventing manager-native bot tasks. If retaining Sol's choice for minimum change, document that installing/discovering a remote connection requires an available worker.
4. Keep Sol's Unknown propagation: bot runs require attention and Vibe stops subsequent connector calls. Adopt Astra's prohibition on automatically resuming an attention-stopped run when late evidence arrives. Late settlement updates evidence, not execution authority.

## Blockers and proof obligations

- Resolve the missing immutable grant revision before implementing shared contracts. This is a concrete authority gap, not an optional hardening step.
- Prove the pinned Vibe adapter with dynamic MCP tools and existing denial callbacks. Both strong candidates leave its actual runtime compatibility open. One running Vibe task must invoke local stdio inside its existing scope without requesting another process slot.
- Prove the one-shot release boundary with a lost permit response, lost settlement response, worker restart, manager restart, revocation before release, and revocation/regrant after approval. The expected ambiguous outcome never launches again.
- Use the pinned, verified 2025-06-18 contract from Sol/Astra. Luna's 2026-07-28 claim is unsupported by its own design and should not be inherited.
- Real authenticated connector, Proxmox pair, independent Linux install, restore, and supported container scope-termination evidence remain required for full M06 acceptance. A fixture or Dockerfile is not that evidence.

## Rejected Luna shape

Luna adds manager-owned run children and a worker stdio RPC rather than adapting ordinary tasks. Its description places a worker-authenticated endpoint in the manager but says the manager issues work through it, leaving direction and delivery ownership unresolved. It starts ephemeral processes without specifying existing Process supervision or how local Vibe calls coexist with the occupied slot. Its dispatch transition does not explicitly recheck every revision after a queued admission. These are architectural gaps, not details to graft into the base.

## Red-flag screen

Sol/Astra have one State writer for policy and effect evidence, one gateway authority, and private transport internals. The task state and connector ledger are justified separate facts: process termination versus possible external effects. Keep that distinction; do not grow the ledger into another scheduler. Avoid duplicating catalog definitions between run and Vibe paths: derive both from the same immutable binding/current-authority intersection. The explicit per-boundary integration is necessary adaptation, not permission to add pass-through layers.
