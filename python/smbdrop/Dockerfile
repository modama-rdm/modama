# syntax=docker/dockerfile:1
# NOTE: this file is for development only, please see
# `er-c-data/smbdrop/Dockerfile` for the production deployment!

FROM debian:12-slim

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get update && apt-get -y install samba samba-client winbind python3.9 python3-pip strace

COPY ./smbdrop/smb.conf /etc/samba/smb.conf

COPY ./ /code

RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/smbdrop-client
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/smbdrop
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/modama-common

EXPOSE 445/tcp
EXPOSE 5050/tcp

CMD ["/bin/bash", "/code/smbdrop/dev.sh", "--port", "5050", "--host", "0.0.0.0"]
# CMD ["/bin/bash"]
