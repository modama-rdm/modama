FROM nextcloud:apache
LABEL Dieter Weber <d.weber@fz-juelich.de>

ARG aptopts="-y --no-install-recommends"

RUN apt-get -y update && apt-get -y upgrade

RUN apt-get ${aptopts} install iputils-ping procps smbclient libsmbclient-dev

RUN pecl install smbclient
RUN docker-php-ext-enable smbclient
