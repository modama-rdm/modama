FROM nextcloud:23.0.3-apache
LABEL Dieter Weber <d.weber@fz-juelich.de>

ARG aptopts="-y --no-install-recommends"

# Debian 11 LTS ended 2026-08-31; bullseye-security moved to archive.debian.org on
# 2026-09-12. security.debian.org still serves the index but not the packages, so
# apt-get upgrade/install fail with 404 unless all suites come from the archive.
RUN sed -i \
    -e 's|http://deb.debian.org/debian|http://archive.debian.org/debian|g' \
    -e 's|http://security.debian.org/debian-security|http://archive.debian.org/debian-security|g' \
    /etc/apt/sources.list

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get -y update && apt-get -y upgrade

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get ${aptopts} install iputils-ping procps cron sudo python3

RUN mkdir -p /usr/local/share/ca-certificates/fzj/

COPY ./HARICA_TLS_ECC_Root_CA_2021.crt ./HARICA_TLS_RSA_Root_CA_2021.crt /usr/local/share/ca-certificates/fzj/

RUN update-ca-certificates

COPY ./nextcloud-config.json /nextcloud-config.json
COPY ./externalstorage.json /externalstorage.json
COPY ./applyconfig.py /applyconfig.py

COPY ./nextcloud.patch /usr/src/nextcloud.patch
RUN cd /usr/src/nextcloud/ && patch -p2 -l --verbose < ../nextcloud.patch
