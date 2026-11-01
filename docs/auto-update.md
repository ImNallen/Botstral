# Turn on automatic updates

A nightly or stable release reaches the Mac app, the manager VM, and the worker VM after you enable the updater on each host. The package installs the units and does not enable them.

## Put the manifest public key on each host

Copy the release manifest public key to the host through a channel you already trust. A key downloaded next to the release files does not establish trust.

On the manager, save it at `/etc/botstral-manager/manifest-public.pem` with mode `0600`. On the worker, save it at `/etc/botstral-agent/manifest-public.pem` with mode `0600`.

Copy `update.example.json` from `/usr/share/botstral-manager` or `/usr/share/botstral-agent` to `/etc/botstral-manager/update.json` or `/etc/botstral-agent/update.json`. Set `channel_url` to the channel you want and `trusted_key` to the public key path. The default channel URL is nightly. Editing this file is the channel setting. The desktop app does not switch channels.

The first manual install must leave its verified release set at `/var/lib/botstral-manager/installed-release` on the manager and at `~/.local/share/botstral-agent/installed-release` on the worker. A later update uses that set to put the previous package back when health fails. After a successful update the updater replaces that directory with the release it installed.

## Enable the timer

On the manager, run:

```sh
sudo systemctl enable --now botstral-manager-update.timer botstral-manager-update.path
```

The timer runs at 04:00 Europe/Stockholm. The path unit starts the same update when the desktop writes `update.request`.

On the worker, run:

```sh
systemctl --user enable --now botstral-agent-update.timer botstral-agent-update.path
```

The worker timer runs at 04:30 Europe/Stockholm, after the manager window. The worker does not call the manager to decide. It updates only when its own processes are stopped. The worker update unit is a user unit, so the account that enables it must be able to run `dpkg`.

To run a check immediately:

```sh
sudo python3 /usr/lib/botstral-manager/update.py --component manager --state-dir /var/lib/botstral-manager --now
python3 /usr/lib/botstral-agent/update.py --component agent --state-dir "$HOME/.local/share/botstral-agent" --now
```

The updater verifies the signed manifest, stops the service only when the host is idle, keeps a backup, installs the package, and checks health. A failed health check puts the previous package back. When the data schema changed, it also restores the backup taken just before the install. The result is written to `update-report.json` in the state directory.

An unattended manager restart leaves the vault locked. Nothing unlocks it. The desktop says the vault is locked and the owner must unlock it to resume.

## Mac app

The app checks for an update 15 seconds after launch and then every hour. It shows **Restart to update** and does not install until you click that button. The updater signature is verified with the public key in `desktop/src-tauri/tauri.conf.json`.

Store the matching private key as the GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY` on the Release environment. This key has no password. Store an empty `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secret as well. The release job uploads `latest.json` and, when the signing key is present, the updater archive to the `channel-<channel>` release. Those files are not part of the signed immutable release set.
