# syntax=docker/dockerfile:1

FROM debian:13-slim

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get update && apt-get -y install samba samba-client winbind python3.11 python3.11-venv python3-pip strace

RUN --mount=type=cache,target=/root/.cache python3 -m venv /venv/

RUN --mount=type=cache,target=/root/.cache /venv/bin/pip install gunicorn

COPY ./python/smbdrop/smb.conf /etc/samba/smb.conf

COPY ./python/smbdrop-client /code/smbdrop-client
RUN --mount=type=cache,target=/root/.cache /venv/bin/pip install /code/smbdrop-client

COPY ./python/modama-common /code/modama-common/
RUN --mount=type=cache,target=/root/.cache /venv/bin/pip install /code/modama-common/

COPY ./python/smbdrop /code/smbdrop
RUN --mount=type=cache,target=/root/.cache /venv/bin/pip install /code/smbdrop

EXPOSE 445/tcp
EXPOSE 5051/tcp

# NOTE: this has to be limited to a single sync worker currently to
# make the samba subprocess work, and to make sure things are properly
# synchronized
CMD ["/venv/bin/gunicorn", "--worker-class", "sync", "--workers", "1", "--bind", "0.0.0.0:5051", "smbdrop.server:app", "--log-level=debug"]
