# Coordinator verification

The coordinator integrated the implementation and verified source `85f385cecf39e4146854115fd3c1cff78a0d93d5`. [Source digests](source-files.json) identify the exact changed code. The original implementation record retains its earlier source scope. New evidence logs remove trailing blank lines for repository whitespace checks.

## Corrections

Packaged interaction found [issue 48](https://github.com/ImNallen/Botstral/issues/48). Locking the Vault made an expected credential-read refusal block the paired workspace and its protected unlock dialog. The [before run](locked-unlock-before.log) reports `left: "blocked"` and `right: "online"`. The [after run](locked-unlock-after.log) passes locked review and metadata reads, preserved owner connectivity and protected unlock. Expected locked reads remain refused. Authentication and transport failures retain their existing failure path.

A focused runtime probe found [issue 49](https://github.com/ImNallen/Botstral/issues/49). Restoring older metadata with unchanged journal and profile identifiers cleared protection on a newer profile. The [before probe](profile-rollback-before.log) reports `captureProtected: false`. The profile marker now binds possible-release history. [Runtime checks](profile-rollback-after.log) and [worker checks](profile-tests.log) preserve protection after metadata rollback, interrupted publication and legacy provenance. Both durable records precede private release.

## Checks

- `./scripts/verify.sh` exited zero. [Repository log](repository.log) includes production builds and both nonproduction release refusals.
- `./scripts/verify-desktop.sh` exited zero. [Desktop log](desktop.log) includes generated bindings, frontend tests, native checks, actual-manager integration, the new locked-unlock regression and the debug application build.
- The [focused driver](../20261029T215738773182Z/commands.json) passed all focused gates, including 22 browser cases and 10 nested complete-runtime capture cases in [browser.json](../20261029T215738773182Z/browser.json).

That driver's first broad run used an alternate Cargo target directory. A packaging test consequently could not find the source-root manager binary. Its [failed setup log](../20261029T215738773182Z/repository.log) remains unchanged. The two default-path commands above passed afterward. The focused gates did not need another run.

## Packaged native interaction

The coordinator built and drove the actual macOS application against isolated local manager state through native accessibility controls. [Entry results](native-entry.json) record a committed login, metadata-only listing, empty fields on reopening and zero private-value matches across 27 owned operational files. The initial input-automation attempt was refused before a write receipt; setting and observing the masked native fields then saved successfully.

After the correction, a fresh isolated pairing stayed online after locking and a refused metadata read. The native unlock dialog opened and returned `Vault unlocked.`. Cancelling another dialog returned `Protected entry cancelled. No values were sent.`. [Unlock results](native-unlock.json) identify the native source digests, successful sign-out and absent fixture keychain entry. Both temporary fixture roots were stopped and removed. [Initial cleanup](native-cleanup.json) records removal of the first owned test credential.

The rebuilt application's attempted reuse of its first test credential [waited inside the macOS keychain lookup](native-keychain-wait.log). Its cause was not established. A fresh isolated profile completed the corrected interaction. This does not establish application-update or cross-build keychain continuity. No packaged native-to-Linux sign-in is claimed. The existing videos remain accurately labeled component recordings.

## Review and scope

The initial browser/worker review passed at `3527f3c`, before the coordinator's two corrections. Final independent review and Linux private-TLS/cgroup execution are separate gates. No release verdict is inferred from these local tests.

The first supported adapter remains limited to native forms at the top level or in one same-origin child frame. Unsupported controls and custom submission refuse. This preserves the exact-fill boundary; broader site support and protected visual browsing remain open. No whole P06 milestone checkbox is completed by this record.
