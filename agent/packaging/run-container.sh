#!/bin/sh
set -eu
engine=${1:?Provide docker or podman.}
image=${2:?Provide the immutable worker image reference.}
worker_home=${3:?Provide the private worker home volume path.}
case "$engine" in docker|podman) ;; *) echo 'Docker or Podman required.' >&2; exit 1;; esac
case "$worker_home" in /*) ;; *) echo 'Absolute private worker home required.' >&2; exit 1;; esac
[ -d "$worker_home" ] || { echo 'Worker home must already exist with UID 10001 and mode 0700.' >&2; exit 1; }
if [ "$engine" = podman ]; then
    [ "$(podman info --format '{{.Host.OS}} {{.Host.Security.Rootless}}')" = "linux false" ] || { echo 'Rootful Linux Podman required.' >&2; exit 1; }
    exec podman run --rm --name botstral-computer --privileged --systemd=always --cgroupns=host --stop-signal=SIGRTMIN+3 --tmpfs /run --tmpfs /run/lock --tmpfs /tmp --mount type=bind,src=/sys/fs/cgroup,dst=/sys/fs/cgroup --mount "type=bind,src=$worker_home,dst=/home/botstral-agent" "$image"
fi
[ "$(docker info --format '{{.OSType}}')" = linux ] || { echo 'Linux Docker required.' >&2; exit 1; }
case "$(docker info --format '{{json .SecurityOptions}}')" in *rootless*) echo 'Rootless Docker is unsupported.' >&2; exit 1;; esac
exec docker run --rm --name botstral-computer --privileged --cgroupns=host --stop-signal=SIGRTMIN+3 --tmpfs /run --tmpfs /run/lock --tmpfs /tmp --mount type=bind,src=/sys/fs/cgroup,dst=/sys/fs/cgroup --mount "type=bind,src=$worker_home,dst=/home/botstral-agent" "$image"
