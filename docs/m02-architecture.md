# M02 worker and session architecture

M02 extends the existing manager state thread. It owns worker enrollment, connection generations, and per-bot logical sessions alongside M01 owner authentication and vault state.

## Selected design

The parent compared three designs and selected mutual TLS with one state owner. This adopts the Astra candidate and the judge's recommendation. It also adopts concrete transport bounds and a populated version-one migration test from Sol. A separate policy broker would split authority. A custom signature exchange would duplicate identity proof supplied by TLS. Best-effort takeover cannot establish that the worker stopped input.

The worker identity is SHA-256 of the verified client certificate DER. This narrows the candidate's public-key fingerprint proposal and avoids custom certificate key parsing. Certificate renewal requires owner revocation and enrollment of the replacement certificate.

## Interfaces and ownership

`link/` owns versioned Rust wire types and the generated JSON Schema. Request, command, task, bot, worker, and computer-session identifiers are distinct UUID types. Generations fence connections. Revisions fence input authority within one session. TypeScript bindings remain C01 work.

`manager/src/manager/state/` extends the existing SQLite owner. Migration 0002 adds workers, hashed invitations, and computer sessions without rebuilding M01 tables. Owner methods reauthorize within the queued operation. Socket tasks own transport only.

The selected transport uses a distinct mutual TLS listener. The daemon initiates WSS, validates the manager CA and hostname, and presents its certificate. Negotiation precedes a new generation and an authoritative acknowledged snapshot. An ownership change remains pending until the worker applies the revision and confirms it.

## Verification units

1. Shared schema and additive state migration. The populated-v1 test proves owner sign-in, bot retrieval, locked startup, vault unlock, exact ciphertext preservation, and credential decryption.
2. Enrollment, TLS, WSS, negotiation, connection fencing, and bounded shutdown.
3. Confirmed ownership, a feature-gated test worker, actual-binary acceptance, and release separation.

The implementation owner works alone in the T3-created worktree. The parent owns independent correctness and comment review. The decision trail is [m02-decisions.tsv](m02-decisions.tsv).

## Acceptance gaps

Actual two-VM deployment requires infrastructure and remains unverified. Local WSS cannot close that acceptance condition. M03 owns durable task journals and recovery. W02 owns real independent desktops. Production work execution remains unavailable in M02.
