# syntax=docker/dockerfile:1

FROM debian:stable-slim

RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get -y install samba samba-client winbind python3.9 python3-pip strace

RUN --mount=type=cache,target=/root/.cache python3 -m pip install gunicorn

COPY ./python/smbdrop/smb.conf /etc/samba/smb.conf

COPY ./python/smbdrop-client /code/smbdrop-client
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/smbdrop-client

COPY ./python/smbdrop /code/smbdrop
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/smbdrop

EXPOSE 445/tcp
EXPOSE 5051/tcp

CMD ["gunicorn", "--worker-class", "sync", "--workers", "1", "--bind", "0.0.0.0:5051", "smbdrop.server:app"]
