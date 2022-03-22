# syntax=docker/dockerfile:1

FROM debian:stable-slim
LABEL Dieter Weber <d.weber@fz-juelich.de>

RUN --mount=type=cache,target=/var/cache/apt apt-get -y update &&\
    apt-get -y upgrade &&\
    apt-get -y install acl python3 python3-pip

RUN --mount=type=cache,target=/root/.cache python3 -m pip install inotify

COPY ./python/permissions/watchdog.py /watchdog.py

CMD ["python3", "/watchdog.py"]
