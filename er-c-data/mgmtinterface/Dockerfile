FROM debian:stable-slim
LABEL Dieter Weber <d.weber@fz-juelich.de>

RUN --mount=type=cache,target=/var/cache/apt apt-get -y update && \
    apt-get -y upgrade && \
    apt-get -y install python3 python3-pip

RUN --mount=type=cache,target=/root/.cache python3 -m pip install gunicorn

COPY ./python /code

RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/smbdrop-client/
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/permissions-client/
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/mgmtinterface-client/
RUN --mount=type=cache,target=/root/.cache python3 -m pip install -e /code/mgmtinterface/

WORKDIR /code/mgmtinterface/src/mgmtinterface

COPY ./er-c-data/mgmtinterface/entrypoint.sh /entrypoint.sh
COPY ./er-c-data/mgmtinterface/config.py /config.py

ENV MGMTINTERFACE_CONFIG /config.py

CMD ["/bin/bash", "/entrypoint.sh"]