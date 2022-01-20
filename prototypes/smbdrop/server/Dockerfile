# syntax=docker/dockerfile:1

FROM debian:stable-slim

RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get -y install samba samba-client winbind python3.9 python3-pip strace

COPY smb.conf /etc/samba/smb.conf

COPY dev.sh /dev.sh
COPY ./ingest-mgmt-service/ /src/ingest-mgmt-service/

RUN --mount=type=cache,target=/root/.cache python3 -m pip install /src/ingest-mgmt-service/

EXPOSE 445/tcp
EXPOSE 5050/tcp

CMD ["/bin/bash", "/dev.sh", "--port", "5050", "--host", "0.0.0.0"]
# CMD ["/bin/bash"]
# CMD ["smbd", "-FS", "--no-process-group"]
