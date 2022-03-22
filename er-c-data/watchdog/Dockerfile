# syntax=docker/dockerfile:1

FROM debian:stable-slim
LABEL Dieter Weber <d.weber@fz-juelich.de>

RUN --mount=type=cache,target=/var/cache/apt apt-get -y update && apt-get -y upgrade && apt-get -y install sudo acl inotify-tools python3 python3-pylibacl libpam0g libpam-runtime libpam-sss libnss-sss python3-sss python3-pip

COPY ./python/permissions/watchdog.sh /watchdog.sh

CMD ["/bin/bash/", "/watchdog.sh"]
