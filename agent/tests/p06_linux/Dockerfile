FROM ubuntu:24.04
ENV container=docker
RUN apt-get update && apt-get install -y --no-install-recommends systemd systemd-sysv dbus-user-session python3 python3-websocket libnss3-tools curl ca-certificates xz-utils openssl \
    && systemd --version | head -1 | grep '^systemd 255 ' \
    && useradd --uid 10001 --create-home --shell /bin/bash p06 \
    && install -d /var/lib/systemd/linger \
    && touch /var/lib/systemd/linger/p06 \
    && ln -s /dev/null /etc/systemd/system/systemd-binfmt.service \
    && ln -s /dev/null /etc/systemd/system/systemd-sysctl.service \
    && ln -s /dev/null /etc/systemd/system/systemd-modules-load.service \
    && ln -s /usr/lib/systemd/system/user@.service /etc/systemd/system/multi-user.target.wants/user@10001.service
COPY agent/runtime /source/agent/runtime
RUN bash /source/agent/runtime/setup.sh /opt/p06-runtime \
    && chmod -R a+rX /opt/p06-runtime
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
