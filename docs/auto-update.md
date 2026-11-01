# Automatic updates

The nightly channel is the default. The matching public keys are the release manifest key on each server and the Tauri updater key in the Mac app.

## Public key

The release verifier uses `keys/manifest-public.pem` by default. Automatic updates still read `/etc/botstral-manager/manifest-public.pem` on the manager and `/etc/botstral-agent/manifest-public.pem` on the worker, mode `0600`. A key downloaded next to the release files does not establish trust.

The Mac app verifies updater signatures with the public key in `desktop/src-tauri/tauri.conf.json`.

## Channel

Copy `update.example.json` to `update.json` in `/etc/botstral-manager` or `/etc/botstral-agent`. Set `channel_url` and `trusted_key`. The default `channel_url` is nightly. The desktop app does not switch channels.

## Timer

```sh
sudo systemctl enable --now botstral-manager-update.timer botstral-manager-update.path
sudo systemctl enable --now botstral-agent-update.timer botstral-agent-update.path
```

The manager timer runs at 04:00 Europe/Stockholm. The worker timer runs at 04:30. The Mac app checks within an hour of launch.
