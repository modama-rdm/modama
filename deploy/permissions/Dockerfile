FROM debian:13-slim
LABEL Dieter Weber <d.weber@fz-juelich.de>

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get -y update && apt-get -y upgrade && \
  apt-get -y install sudo acl libacl1-dev inotify-tools python3 python3-pylibacl \
  libpam0g libpam-runtime libpam-sss libnss-sss python3-sss python3-pip python3-venv

RUN python3 -m venv --system-site-packages /venv/

RUN --mount=type=cache,target=/root/.cache /venv/bin/python3 -m pip install gunicorn

COPY ./python/permissions-client /code/permissions-client/
RUN --mount=type=cache,target=/root/.cache /venv/bin/python3 -m pip install -e /code/permissions-client/

COPY ./python/modama-common /code/modama-common/
RUN --mount=type=cache,target=/root/.cache /venv/bin/python3 -m pip install /code/modama-common/

COPY ./python/permissions /code/permissions/
RUN --mount=type=cache,target=/root/.cache /venv/bin/python3 -m pip install -e /code/permissions/

WORKDIR /code/permissions/

COPY ./deploy/permissions/config.py /config.py

ENV PERMISSIONS_CONFIG /config.py

CMD ["/venv/bin/gunicorn", "--bind", "0.0.0.0:5050", "permissions.server:app", "--log-level=debug"]
