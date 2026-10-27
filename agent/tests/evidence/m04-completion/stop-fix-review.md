# Independent stop-fix security and correctness review

Independent approval of the reviewed implementation and focused runner. No unresolved medium or higher finding remains. This approves source, not milestone closure or an unexecuted live acceptance campaign.

Reviewed the uncommitted four-file diff against 6acbe06e2c7467de2296892cf0a34cf58b07903a. Read AGENTS.md, the requested poteto-mode instructions, the security and correctness review skill, caller lifecycle handling, transport retry behavior, scope validation, fixture shutdown, the independent tail acceptance plan, and retained regression logs. No provider calls, remote launches, secret reads, commits, or production writes were made. Python runner syntax was independently compiled in memory.

## Production assessment

The proof retains the pre-stop unit invocation and a file descriptor pinning the original cgroup inode. It rejects changed boot/host, wrong invocation, replaced cgroup, unreadable population, still-populated cgroup, and an active service with an empty cgroup. Stop command failure alone no longer defeats independently observed kernel termination. A final stopped-or-absent unit observation and a second kernel emptiness check prevent treating an empty active unit as stopped. Cancellation continues through the existing durable journal stopped transition; failure still becomes Unproved. Diagnostics are bounded and control characters are replaced.

The new population poll has a two-second deadline after the stop command, not a two-second end-to-end bound. The explicit command budgets are three seconds for the initial show, five for stop, two for the population poll, and three for the final show, approximately thirteen seconds plus local filesystem and scheduling overhead. This fits the existing eighteen-second worker shutdown budget for this operation. The code does not claim to recover every possible already-Unproved state or every external unit race.

The cgroup check depends on the existing trusted systemd/cgroup boundary. A malicious privileged actor concurrently moving processes or replacing units is outside what these sampled observations can prove. The change does not introduce a new escape from that boundary.

## Finding resolved during review

Medium acceptance coverage finding at agent/tests/m04_two_vm.py lifecycle_restart. The initial version waited only two seconds after manager HTTP startup, while worker transport retry can reach five seconds. It could pass unchanged task/call assertions without exercising recovered worker attachment.

The final reviewed version waits for a newer ready worker generation and confirmed session before the observation window. It also compares native execution identifiers and requires all recorded scopes to remain empty after restart. This resolves the finding. The runner still asserts unchanged cancelled task process evidence and accounting. Budget refusal requires both task and coding-call lookup to return not found. Cleanup, shutdown, privacy, and redaction failures remain failures of the scoped run.

The focused suite reserves one 53,249-microdollar call and cannot set live_acceptance=true. The ledger must preserve existing 911,785 holds, giving 965,034 total. This is source review of the reservation path, not an assertion that a new reservation has occurred.

## Evidence and limits

stop-proof-before.log records the lost-response and delayed-emptiness regressions failing before the fix. stop-proof-after.log records nine passing stop tests, including rejection of wrong boot, wrong invocation, replacement, population, unavailable population, and an active empty service. stop-final-linux.log records eight real-systemd ignored stop tests passing on the final source. It also records the actual pinned Vibe integration passing with the completed task, settled call, process, output, and exact artifact-byte assertions after manager restart. The recorded browser/file/process/artifact integration passes in the same final-source log. These are retained execution logs inspected independently, not tests launched by this reviewer. The broader Process suite was still running at review completion.

The completed-artifact integration uses a recorded provider endpoint and stops its worker before restarting the manager. It covers durable completed-result retrieval, not live worker reattachment; the focused two-VM lifecycle lane supplies that separate branch. A two-second post-reattachment observation is a bounded replay regression, not proof against arbitrarily delayed future behavior.

The focused live lane, complete final-source Process regressions, installed binary/source provenance, and final gate-to-evidence reconciliation remain required before milestone closure. The historical failed full-run report must remain failed. Follow tail-acceptance-review.md for aggregate source applicability; this report does not convert prior live failures into passes.

## Reviewed SHA-256 values

- agent/src/process/systemd.rs. 8aabd666a84e2e954f88bc6324db806af5e75ba63994ab7e6a493d29fa1245b5.
- agent/src/journal/process.rs. 9a813836871a8b6ac198c3afe98fc5aa090028e7e56251a5f4368a8508657318.
- agent/tests/m04_two_vm.py. 144c4db7d7409d7aa26645e1b611791048a1e19da3ea46e60465aa925cdea882.
- manager/src/manager/inference/coding_production.rs. 5c7c080541eafdd4cf19d3c6d37a3ca5b7922cf1e4ad62873776c5e77f821c0d.
