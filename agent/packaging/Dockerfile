FROM rust:1.93.0-bookworm@sha256:d0a4aa3ca2e1088ac0c81690914a0d810f2eee188197034edf366ed010a2b382 AS build
WORKDIR /source
ARG BOTSTRAL_SOURCE_REVISION
ENV BOTSTRAL_SOURCE_REVISION=$BOTSTRAL_SOURCE_REVISION
COPY Cargo.toml Cargo.lock release.json ./
COPY manager ./manager
COPY agent ./agent
COPY link ./link
RUN cargo build --locked --release -p botstral-agent --bin botstral-agent --bin botstral-agent-supervisor-check

FROM ubuntu:24.04@sha256:24737cef17f4e22b1977df7d9a05e77c4e28b7da5b8911f8d8ccda136bf3573c
ENV container=docker
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN sed -i -E 's|http://[^ ]+/ubuntu(-ports)?/|https://snapshot.ubuntu.com/ubuntu/20261020T000000Z/|g' /etc/apt/sources.list.d/ubuntu.sources \
    && apt-get -o Acquire::Check-Valid-Until=false -o APT::Update::Error-Mode=any update \
    && apt-get install -y --no-install-recommends systemd systemd-sysv dbus-user-session python3 ca-certificates \
    && systemd --version | head -1 | grep '^systemd 255 ' \
    && dpkg-query -W > /usr/share/botstral-image-packages.txt \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /source/release.json /source/release.json
COPY --from=build /source/manager/packaging /source/manager/packaging
COPY --from=build /source/agent/packaging /source/agent/packaging
COPY --from=build /source/target/release/botstral-agent /tmp/botstral-agent
COPY --from=build /source/target/release/botstral-agent-supervisor-check /tmp/botstral-agent-supervisor-check
RUN python3 /source/agent/packaging/install.py --binary /tmp/botstral-agent --root / \
    && groupadd --gid 10001 botstral-agent \
    && useradd --uid 10001 --gid 10001 --create-home --home-dir /home/botstral-agent --shell /bin/bash botstral-agent \
    && chmod 0700 /home/botstral-agent \
    && install -d -m 0700 -o 10001 -g 10001 /home/botstral-agent/.local/share/botstral-agent /home/botstral-agent/.config/systemd/user/default.target.wants \
    && install -d /etc/systemd/user/default.target.wants \
    && ln -s /usr/lib/systemd/user/botstral-agent.service /etc/systemd/user/default.target.wants/botstral-agent.service \
    && install -d /var/lib/systemd/linger /etc/systemd/system/multi-user.target.wants \
    && touch /var/lib/systemd/linger/botstral-agent \
    && ln -s /usr/lib/systemd/system/user@.service /etc/systemd/system/multi-user.target.wants/user@10001.service \
    && chown -R 10001:10001 /home/botstral-agent \
    && rm -rf /source /tmp/botstral-agent /tmp/botstral-agent-supervisor-check
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
