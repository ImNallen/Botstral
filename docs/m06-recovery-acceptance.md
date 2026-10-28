# M06 recovery and packaging acceptance

Recovery and packaging are implemented and verified independently of the connector work. This record does not close M06.

## Observed checks

`make verify` passed on the coordinator checkout after recovery integration. It runs formatting, Clippy, workspace tests, packaging tests, production builds, and both nonproduction release refusals. [The full log](evidence/m06-parent/core-before-connectors.log) predates connector integration.

The independent Astra review reproduced two defects. Worker relocation retained unusable absolute paths, and Vibe diagnostics expected the wrong manifest shape. Both focused checks [failed before correction](evidence/m06-parent/recovery-before.log). All 12 packaging tests [pass after correction](evidence/m06-parent/recovery-after.log). Worker restoration now requires the original absolute state path. Vibe diagnostics accept the production file-list manifest and reject modified runtime bytes. [The review report](m06-recovery-review.md) retains the findings and rejected hypotheses. [Issue 32](https://github.com/ImNallen/Botstral/issues/32) tracks the corrections.

The actual manager-state restoration test verifies original owner login, expired old sessions, locked startup, original-passphrase unlock, credential decryption, history, authoritative Markdown, and artifact retrieval. It does not substitute provider credentials for future connector-credential usability acceptance.

## Native Linux checks

[The Proxmox report](evidence/m06-parent/proxmox-packaging/report.json) records source `bcf96860712bc586c2cb02fed415068e5532a2fc`, the transferred source-archive digest, the compiled binary digest, and distinct hashed machine identities. Both existing Ubuntu 24.04.5 guests are x86_64. The driver stages fresh component installations and does not replace or activate existing services.

Both guests passed the 12 packaging checks. The Computer VM compiled the workspace with Rust 1.93.0, passed actual manager-state restoration, and ran the production worker backend's launch and empty-scope stop probe. Evidence includes [manager installation](evidence/m06-parent/proxmox-packaging/manager-install.json), [worker installation](evidence/m06-parent/proxmox-packaging/agent-install.json), [restoration](evidence/m06-parent/proxmox-packaging/linux-restore.log), and [scope termination](evidence/m06-parent/proxmox-packaging/worker-supervisor.log).

Repeat with `python3 scripts/m06-proxmox-packaging.py --output /private/new-evidence`. The optional `--keep-fixtures` retains the exact temporary roots named in the report for subsequent integrated acceptance. No service remains running in those roots after these checks.

## Docker Linux checks

Both corrected images were rebuilt and accepted on Docker Linux arm64. [The summary](evidence/m06-parent/packaging-summary.json) records immutable image identities and verifies the installed recovery and diagnostics file hashes against the coordinator source.

After connector integration, [both final images](evidence/m06-parent/integrated-images.json) were rebuilt and accepted again. The installed worker binary contains the current embedded connector helper. Its production scope-stop and journal restoration checks passed. The earlier retained Proxmox staging roots were subsequently [verified quiescent and removed](evidence/m06-parent/proxmox-cleanup.json).

A final installer check reproduced both packages being accepted into one installation root. The two-order regression [failed before correction](evidence/m06-parent/cohost-install-before.log). The installer now rejects that unsupported layout before writing the second package, and [all 13 packaging checks pass](evidence/m06-parent/cohost-install-after.log). [Issue 33](https://github.com/ImNallen/Botstral/issues/33) tracks this correction. Both images were rebuilt and accepted again after it; final image identities are recorded separately.

The manager runs as user 10001 with dropped capabilities and a read-only root filesystem. Its actual TLS and file-configuration startup passed. The worker runs systemd 255 and the production Process backend. Its scope-stop proof, concurrent selected-root refusal, production journal backup, original-path restoration, retained incarnation, and certificate-byte recovery passed. [The worker result](evidence/m06-parent/docker-worker.json) retains those observations.

The initial worker rerun [failed in the fixture](evidence/m06-parent/docker-worker-before-fixture.json) when OverlayFS refused renaming an image-layer directory. A fresh fixture directory fixes that harness assumption. The production recovery implementation and image were unchanged for the passing rerun. The failed record remains failed.

## Remaining gates

[Integrated Linux acceptance](m06-integrated-acceptance.md) subsequently verified retained connector credentials after restored vault unlock, authenticated remote execution, actual Vibe stdio, and scope cancellation across the Proxmox pair. That run restores the manager bundle and restarts the retained worker. It does not claim a second worker restoration. Final whole-branch review remains open. The pinned official filesystem server's [reference setup evidence](evidence/m06-parent/reference-connector.json) remains distinct from the integrated official-SDK filesystem backend.

Podman and Docker x86_64 were not exercised. Full Computer and Vibe runtime installation is outside this packaging subset's evidence. Selected runtime roots must satisfy the documented regular-file recovery policy. Retained worker identity still requires the operator to retire the original instance. Multi-root restoration does not provide atomic publication across filesystems.
