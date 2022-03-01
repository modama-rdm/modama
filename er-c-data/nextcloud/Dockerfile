FROM nextcloud:apache
LABEL Dieter Weber <d.weber@fz-juelich.de>

ARG aptopts="-y --no-install-recommends"

RUN --mount=type=cache,target=/var/cache/apt apt-get -y update && apt-get -y upgrade

RUN --mount=type=cache,target=/var/cache/apt apt-get ${aptopts} install iputils-ping procps cron sudo python3

COPY ./nextcloud-config.json /nextcloud-config.json
COPY ./externalstorage.json /externalstorage.json
COPY ./applyconfig.py /applyconfig.py

COPY ./nextcloud.patch /usr/src/nextcloud.patch
RUN cd /usr/src/nextcloud/ && patch -p2 -l --verbose < ../nextcloud.patch
