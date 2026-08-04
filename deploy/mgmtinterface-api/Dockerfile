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

CMD ["/venv/bin/uvicorn", "mgmtinterface_api:app", "--host", "0.0.0.0", "--port", "8000"]
