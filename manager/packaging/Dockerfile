FROM rust:1.93.0-bookworm@sha256:d0a4aa3ca2e1088ac0c81690914a0d810f2eee188197034edf366ed010a2b382 AS build
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY manager ./manager
COPY agent ./agent
COPY link ./link
RUN cargo build --locked --release -p botstral-manager --bin botstral-manager

FROM ubuntu:24.04@sha256:24737cef17f4e22b1977df7d9a05e77c4e28b7da5b8911f8d8ccda136bf3573c
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN sed -i -E 's|http://[^ ]+/ubuntu(-ports)?/|https://snapshot.ubuntu.com/ubuntu/20261020T000000Z/|g' /etc/apt/sources.list.d/ubuntu.sources \
    && apt-get -o Acquire::Check-Valid-Until=false -o APT::Update::Error-Mode=any update \
    && apt-get install -y --no-install-recommends python3 ca-certificates \
    && dpkg-query -W > /usr/share/botstral-image-packages.txt \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /source/manager/packaging /source/manager/packaging
COPY --from=build /source/target/release/botstral-manager /tmp/botstral-manager
RUN python3 /source/manager/packaging/install.py --binary /tmp/botstral-manager --root / \
    && groupadd --gid 10001 botstral-manager \
    && useradd --uid 10001 --gid 10001 --home-dir /var/lib/botstral-manager --shell /usr/sbin/nologin botstral-manager \
    && install -d -m 0700 -o 10001 -g 10001 /var/lib/botstral-manager \
    && chown -R 10001:10001 /etc/botstral-manager \
    && rm -rf /source /tmp/botstral-manager
USER 10001:10001
EXPOSE 43122
ENTRYPOINT ["/usr/lib/botstral-manager/launch.py", "--config", "/etc/botstral-manager/service.json"]
