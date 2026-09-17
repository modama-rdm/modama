FROM debian:13-slim

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get -y update && \
    apt-get -y upgrade && \
    apt-get -y install python3-full python3-pip

RUN --mount=type=cache,target=/root/.cache python3 -m venv /venv/

RUN --mount=type=cache,target=/root/.cache /venv/bin/pip install uvicorn

COPY ./python/permissions-client /code/permissions-client
RUN --mount=type=cache,target=/root/.cache /venv/bin/pip install /code/permissions-client/

# This one last to speed up iteration on the API
COPY ./python/mgmtinterface-api /code/mgmtinterface-api
RUN --mount=type=cache,target=/root/.cache /venv/bin/pip install -e /code/mgmtinterface-api/

WORKDIR /code/mgmtinterface-api

# Python rather than curl: the slim image has no http client installed.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/venv/bin/python3", "-c", \
         "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8000/mgmt-api/healthz', timeout=3)"]

CMD ["/venv/bin/uvicorn", "mgmtinterface_api.main:app", "--host", "0.0.0.0", "--port", "8000"]
