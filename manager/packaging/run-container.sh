#!/bin/sh
set -eu
engine=${1:?Provide docker or podman.}
image=${2:?Provide the immutable manager image reference.}
manager_state=${3:?Provide the private manager state directory.}
manager_config=${4:?Provide the private configuration and TLS directory.}
case "$engine" in docker|podman) ;; *) echo 'Docker or Podman required.' >&2; exit 1;; esac
case "$manager_state:$manager_config" in /*:/*) ;; *) echo 'Absolute state and configuration roots required.' >&2; exit 1;; esac
exec "$engine" run --rm --name botstral-manager --read-only --tmpfs /tmp:mode=700 --user 10001:10001 --cap-drop ALL --security-opt no-new-privileges --publish 43122:43122 --mount "type=bind,src=$manager_state,dst=/var/lib/botstral-manager" --mount "type=bind,src=$manager_config,dst=/etc/botstral-manager,readonly" "$image"
