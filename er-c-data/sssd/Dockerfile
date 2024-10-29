FROM debian:bookworm-slim
LABEL Dieter Weber <d.weber@fz-juelich.de>

RUN --mount=type=cache,target=/var/cache/apt apt-get -y update && apt-get -y upgrade && \
  apt-get -y install sssd libnss-myhostname libpam-sss libnss-sss libpam0g libpam-runtime

CMD ["/usr/sbin/sssd", "--interactive"]
