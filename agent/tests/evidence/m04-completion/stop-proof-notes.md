# Stop proof regression evidence

The original live cancellation error branch remains unknown. This change addresses two reproduced proof failures, rather than claiming either caused the original cancellation.

Before the fix, real Linux user services reproduced both failures. A completed systemctl stop followed by an injected lost response returned an error. Returning before a delayed real systemctl stop completed returned scope still populated. The baseline log records two passed and two failed tests.

The fix authenticates the existing boot, host, service invocation, and cgroup before requesting stop. It pins the cgroup directory and checks device and inode identity. It then checks authoritative cgroup.events for up to two seconds even if the stop request fails. An absent cgroup also proves emptiness. Missing events on an existing directory, read failures, replacements, unknown identity, and populated scopes fail closed. A final systemctl observation and kernel boot read validate identity, followed by another kernel emptiness check.

The production public stop path uses the same implementation exercised by the regressions. The private future parameter permits deterministic loss or delay of the command response without mocking Linux cgroups or systemd identity.

The existing command limits are three seconds for the initial identity read, five seconds for stop, and three seconds for the final identity read. The additional emptiness poll is two seconds. These sum to thirteen seconds, below the eighteen second worker shutdown proof limit. This is a configured bound, not a measured performance claim.

The final Linux command was run over the existing botstral-computer SSH alias in /home/botstral/m04-final-acceptance.GycLnu/source.

```
/home/botstral/.cargo/bin/cargo test -p botstral-agent stop_tests -- --include-ignored --nocapture
```

The final log records nine passed tests. Eight operate real unique transient services and real Linux cgroups. They cover normal production stop, a stop error after actual stop, delayed emptiness, a still populated scope despite stop error, wrong boot, wrong invocation, a replacement invocation after stop, and an active service whose process exited naturally leaving an empty cgroup. Retained units must be inactive or failed. Result classification includes both pre-stop and post-stop observations. The journal prints a 256-character bounded, control-character sanitized stop proof diagnostic. One local filesystem test verifies that a missing population file on an existing directory is refused. Tests create unique unit names and stop only their own services. No existing fixture roots, databases, authentication files, or provider services were accessed.

Full native Vibe cancellation remains the root agent's integration validation. No commit or paid provider call was made by this worker. Independent review is pending.
