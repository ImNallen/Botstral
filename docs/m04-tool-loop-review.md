# M04 tool-loop review

This is a historical slice record. Its pending-work statements describe that checkpoint. See [M04 completion acceptance](m04-completion.md) for the current milestone result.

The change adds durable model turns above existing worker task authority. It preserves the single-turn API and uses exact child approvals. M04 remains open.

## Independent source review

GPT-6 Astra, GPT-6.1 Sol, and GPT-6 Luna reviewed the implementation at `46f284e`. Sol and Luna reported no findings. Astra confirmed the parent's concern that provider-truncated final text lost its finish reason and appeared completed.

The HTTP regression reproduced `completed` instead of `stopped`. Commit `a5d2ebd` retains the text finish reason, reports `output_limit`, preserves partial output and settled usage, and verifies unchanged state after restart without another call. The correction is tracked in [issue 20](https://github.com/ImNallen/Botstral/issues/20).

A fresh Astra source and evidence-log review found no correction regression. Its remaining flags concerned test counts and a pending full-verification result. The final acceptance record updates both. Expected connection failures at injected worker crash boundaries did not fail their enclosing tests or the completed verification command.

Comment review led to four narration deletions. No correctness suppressions or concrete workaround defects were found. Helper extractions were not needed to correct behavior.

## Production acceptance corrections

The parent also ran the previously unobserved Linux lane. It exposed an incorrect enrollment fixture and unsuitable debug-worker selection for production readiness checks. Commit `64dec67` fixes the protected invitation JSON, selects the release worker, and makes failed fixture stages and phases visible without logging credentials. No production authority or timeout changed.

The final production run passed real browser, shared-file, process, follow-up model, and artifact checks with recorded provider responses. The [acceptance record](m04-tool-loop-acceptance.md) includes the exact source and commands. The parent reviewed these small fixture corrections directly.

## Decisions and limits

Model the Domain kept runs, model turns, and immutable worker children separate. Boundary Discipline kept provider wire data private and owner execution policy outside model-selected arguments. Prove It Works required the native desktop path and actual Linux worker execution, which exposed fixture defects that compilation missed.

Design checks came first. One isolated code owner implemented the coupled provider, ledger, approval, and recovery changes. The parent integrated only after that owner stopped, then verified and reviewed the result.

Source and evidence-log review used a supplied transcript summary, not a full transcript audit. Live provider behavior, live two-VM acceptance, Vibe, vision, and daily spending controls remain outside this delivery. No merge is authorized by this review.
