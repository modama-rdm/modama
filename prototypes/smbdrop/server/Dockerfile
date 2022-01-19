FROM debian:stable-slim

RUN apt-get update && apt-get -y install samba samba-client

COPY smb.conf /etc/samba/smb.conf

EXPOSE 445/tcp

CMD ["/bin/bash"]

# -F: foreground
# -S: log to stdout
# --no-process-group: doesn't work in docker without this, at least in the foreground
CMD ["smbd", "-FS", "--no-process-group"]
