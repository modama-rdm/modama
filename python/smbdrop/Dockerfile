# syntax=docker/dockerfile:1

FROM debian:12-slim

RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get -y install samba samba-client winbind python3.9 python3-pip strace

COPY ./smbdrop/smb.conf /etc/samba/smb.conf

COPY ./ /code

RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/smbdrop-client
RUN --mount=type=cache,target=/root/.cache python3 -m pip install /code/smbdrop

EXPOSE 445/tcp
EXPOSE 5050/tcp

CMD ["/bin/bash", "/code/smbdrop/dev.sh", "--port", "5050", "--host", "0.0.0.0"]
# CMD ["/bin/bash"]
