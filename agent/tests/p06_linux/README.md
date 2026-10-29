# Linux protected sign-in fixture

This fixture uses the production manager, worker, systemd backend, Unix peer admission, mutual TLS endpoint, paired binary entry, owner approval, and sealed Computer runtime. It uses no test bridge or inference provider. All credentials and certificates are generated inside an owned container. Containers publish no host ports.

The driver validates one exact synthetic POST and an applied review. It checks a peer outside the cgroup, a missing client certificate, a changed generation, and a repeated release separately. Evidence contains public metadata, process groups, command exits, and fixed outcomes.

On ARM Docker hosts, systemd runs natively and the x86_64 Rust, Node, and Chromium binaries use the host's existing emulation. The production architecture checks, seals, resource controllers, and deadlines remain active. Pure amd64 systemd failed to start services on the reviewed Docker Desktop host.

The fixture masks systemd's binfmt, sysctl, and module services before boot. Its runner compares Docker's emulation registrations before and after execution. Do not run an unmasked privileged systemd image.

Build release binaries with Rust 1.93, its installed `x86_64-unknown-linux-gnu` target, and Zig available. Choose fresh output and evidence directories.

```sh
python3 agent/tests/p06_linux/build.py --output /tmp/botstral-p06-build-UNIQUE
python3 agent/tests/p06_linux/run.py \
  --binary-directory /tmp/botstral-p06-build-UNIQUE/target/x86_64-unknown-linux-gnu/release \
  --evidence /tmp/botstral-p06-full-UNIQUE
```

The default run installs and seals the full runtime with `agent/runtime/setup.sh`. It records a failure when the production probe refuses.

An explicit smaller fixture removes only the unused full-Chromium and FFmpeg directories, then runs the unchanged `seal.py`. It retains the headless browser, Node, dependencies, and runtime source. Both this fixture and the full runtime passed on the reviewed host. Earlier full-runtime attempts missed the helper readiness deadline.

```sh
python3 agent/tests/p06_linux/run.py \
  --binary-directory /tmp/botstral-p06-build-UNIQUE/target/x86_64-unknown-linux-gnu/release \
  --headless-only --evidence /tmp/botstral-p06-headless-UNIQUE
```

Use `--image IMAGE` to reuse an already built fixture image. The runner removes only its own container. Built images remain for inspection. The smaller fixture alone does not certify the full installation. Neither fixture covers a packaged macOS native-to-Linux session.
