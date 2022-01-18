FROM nextcloud:apache
LABEL Dieter Weber <d.weber@fz-juelich.de>

ARG aptopts="-y --no-install-recommends"

RUN apt-get -y update && apt-get -y upgrade

RUN apt-get ${aptopts} install iputils-ping procps smbclient libsmbclient-dev cron

RUN pecl install smbclient
RUN docker-php-ext-enable smbclient

COPY ./nextcloud.patch /usr/src/nextcloud.patch

RUN cd /usr/src/nextcloud/ && patch -p2 -l --verbose < ../nextcloud.patch
