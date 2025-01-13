# syntax=docker/dockerfile:1

FROM debian:11-slim
LABEL Dieter Weber <d.weber@fz-juelich.de>

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get -y update &&\
    apt-get -y upgrade &&\
    apt-get -y install acl python3 python3-pip

COPY ./python/PyInotify /PyInotify

RUN --mount=type=cache,target=/root/.cache python3 -m pip install -e /PyInotify

COPY ./python/permissions/watchdog.py /watchdog.py

STOPSIGNAL SIGINT

CMD ["python3", "/watchdog.py"]
