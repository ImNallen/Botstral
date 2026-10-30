# C03 macOS daily acceptance

The packaged macOS debug app passed the observed conversation, file, approval, cancellation, reconnection, and persistence checks at source `0e3eb53`. C03 remains open. Delivered notifications and native tray Show and Quit remain unproved.

The [acceptance record](evidence/c03/acceptance.json) retains commands, binary identity, export digest, hidden activity observations, and cleanup. The [source manifest](evidence/c03/source.json) identifies the tested component files. The [native frame manifest](evidence/c03/frames/manifest.json) describes ten unmodified app screenshots.

| Gate | Observed outcome |
| --- | --- |
| Ordinary conversations | Both bots completed recorded assistant turns. |
| Second bot setup | Local administration created C03 writing. The fixture API configured and verified its recorded-provider policy. |
| Switching both directions | Each bot showed only its own conversation after switching away and back. |
| Exact review and approval | Native review displayed normalized file contents, paths, worker target, and permission revision. Approval completed the synthetic file task. |
| Task activity and cancellation | The interface showed Queued, Running, Completed, and Cancelled. Operation counts remained zero. Nonzero progress was not proved. |
| Attachment import | The native chooser selected c03-note.txt. The composer showed its name before submission. |
| Verified export | The native save chooser wrote 65 bytes with mode 0600. The file matched the displayed SHA256. |
| Saved result and source | Saved results listed result.txt. Source task opened its retained Completed task. |
| Reconnect | Pause disabled protected actions. Resume restored Online and the same history. |
| Same-binary relaunch | The app reconnected without pairing again. Both histories and terminal task states returned. |
| Close-to-hide | Window close removed the visible window while the app process remained alive. The desktop cache advanced from cursor 54 to 59 and recorded task completion while hidden. |
| Delivered test notification | Inconclusive. The operating system initially disabled Botstral notifications. Enabling them temporarily and repeating the test proved submission only. |
| Delivered hidden activity notification | Inconclusive. Hidden activity completed, but no delivered banner or Notification Center entry was observed. |
| Native tray Show and Quit | Inconclusive. Native control could not expose the status menu. The test used SIGTERM and executable relaunch, which do not prove either tray action. |

Native control returned `timeoutReached` for Notification Center, SystemUIServer, and Control Center. The optional native launch method was unavailable. Keyboard status-menu access did not expose a usable menu. These are observation limits, not reproduced product defects. No implementation correction was justified. No recording API was available. The screenshots are discrete captures, not video.

The binary used the fresh disposable `BOTSTRAL_DESKTOP_PROFILE` override throughout. It was a linker ad hoc debug package, not a signed nightly. No Developer ID private key, production owner state, or paid inference was used. Sign-out removed the exact fixture Keychain entry, confirmed by lookup exit 44. The fixture processes stopped, both endpoints refused connections, temporary credentials and private logs were removed, and notifications returned to their original disabled setting.

The manager, TLS listener, native Keychain, and native file choosers were real. Provider responses were recorded. Worker effects, exported bytes, and stop evidence were synthetic. This run does not prove Linux browser or process execution. [C02 production acceptance](c02-production-acceptance.md) retains the earlier real-worker browser and cancellation proof, with its frozen-source boundaries. It is not new C03 execution proof.

Linux and Windows native integrations, physical sleep, interface changes, signed nightly distribution, updater behavior, and W02 streams and takeover remain outside this record. Independent review belongs to the parent task.

To repeat the run, follow [the native fixture workflow](c02-native-workflow.md) with a fresh fixture root. Create a second bot in local administration. Copy the first bot's inference policy through the fixture-only local API, configure the second at expected revision zero, and verify revision one. Keep fixture credentials and identifiers private. Reuse the exact packaged binary through relaunch. Capture delivered operating-system notifications and the actual tray menu before marking those gates complete.

Verify the retained source identities and their negative controls with:

```sh
python3 -B desktop/fixtures/c02_source_record.py --check-controls --manifest docs/evidence/c03/source.json
```
