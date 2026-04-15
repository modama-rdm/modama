FROM debian:11-slim
LABEL Dieter Weber <d.weber@fz-juelich.de>

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get -y update && \
    apt-get -y upgrade && \
    apt-get -y install python3 python3-pip

RUN --mount=type=cache,target=/root/.cache python3 -m pip install gunicorn

COPY ./python/smbdrop-client /code/smbdrop-client
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/smbdrop-client/

COPY ./python/permissions-client /code/permissions-client
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/permissions-client/

COPY ./python/mgmtinterface-client /code/mgmtinterface-client
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/mgmtinterface-client/

# This one last to speed up iteration on the GUI
COPY ./python/mgmtinterface /code/mgmtinterface
RUN --mount=type=cache,target=/root/.cache python3 -m pip install -e /code/mgmtinterface/

WORKDIR /code/mgmtinterface/src/mgmtinterface

COPY ./deploy/mgmtinterface/config.py /config.py

ENV MGMTINTERFACE_CONFIG /config.py

CMD ["gunicorn", "--bind", "0.0.0.0:5000", "mgmtinterface:app", "--log-level=debug"]
