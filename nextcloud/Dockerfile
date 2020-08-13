FROM nextcloud:apache
LABEL Dieter Weber <d.weber@fz-juelich.de>

RUN apt-get -y update && apt-get -y upgrade

RUN apt-get -y install iputils-ping

RUN apt-get install -y procps smbclient
RUN apt-get -y install libsmbclient-dev
RUN pecl install smbclient
RUN docker-php-ext-enable smbclient
