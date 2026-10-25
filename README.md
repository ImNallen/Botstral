# Botstral

Botstral is a local, single-owner manager for named bot identities. Owner credentials, bots, and browser sessions persist in SQLite. Bots do not execute tasks yet.

Use Go 1.26, Node 26, and npm 11.

```sh
make deps
make build
./bin/botstral --data-dir ./data --port 8080 --setup
```

Open the separate setup link printed in the terminal. Create your owner username and password, then sign in at `http://127.0.0.1:8080`. Setup expires after 15 minutes and closes after owner creation. It never signs you in or replaces an existing owner.

For subsequent starts, omit `--setup`. Use the same data directory to keep your bots and sessions. An uninitialized start without `--setup` exits before listening. The manager binds only to `127.0.0.1`. Remote access, reverse proxies, paired devices, model connections, and task execution are outside M01.

The binary embeds the production frontend. Run it from any directory without `web/` or Node installed. Protect and retain the data directory. It contains `botstral.sqlite` and its SQLite journal files. Stop the manager before copying the entire directory for a backup.

Each browser session expires seven days after sign-in. Use **Sessions** to revoke one session, including the current one, or **Sign out** to end the current session. No authentication is stored in local storage.

```sh
make dev
```

Development serves `web/dist` from the manager at port 8080 and watches frontend changes. Refresh the browser after a rebuild. Restart `make dev` after Go changes. Development data lives in `.local/data`. Interrupting the command stops both process groups.

```sh
npm --prefix web exec playwright install chromium
make verify
```

`make verify` builds the frontend, runs Go race tests and vet, checks TypeScript, builds `bin/botstral`, and runs built-binary and Playwright acceptance checks. CI runs the same command. Browser screenshots and failure traces appear in `web/test-results`. On Linux, install browser system dependencies with `npm --prefix web exec playwright install --with-deps chromium`.

The acceptance script copies the binary outside the repository. It proves owner creation, embedded assets, the same bot ID after restart and explicit sign-in, durable cookies, anonymous rejection, setup isolation and closure, revocation, and sign-out. Browser tests cover the real forms, narrow layout, connection loss, preserved drafts, and late responses after sign-out.

See [PLAN.md](PLAN.md) for the remaining manager, client, and shared-computer milestones.
