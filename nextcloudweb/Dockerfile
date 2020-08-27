FROM debian:stable-slim
LABEL Dieter Weber <d.weber@fz-juelich.de>

ARG aptopts="-y --no-install-recommends"

RUN apt-get -y update && apt-get -y upgrade

RUN apt-get ${aptopts} install less iputils-ping

RUN apt-get install ${aptopts} procps smbclient
RUN apt-get install ${aptopts} libsmbclient-dev

RUN apt-get install ${aptopts} apache2 libapache2-mod-php
RUN apt-get install ${aptopts} php-gd php-pgsql php-curl php-mbstring php-intl
RUN apt-get install ${aptopts} php-gmp php-bcmath php-imagick php-xml php-zip php-bz2

RUN apt-get install ${aptopts} php-smbclient php-ldap php-ftp php-imap php-redis ffmpeg php-apcu

RUN apt-get install ${aptopts} gpg gpg-agent wget ca-certificates bzip2

COPY ./nextcloud.asc /root/nextcloud.asc

# Comment out to confirm that signature check fails without the key
RUN gpg --import /root/nextcloud.asc

ARG nextpackage="nextcloud-19.0.1.tar.bz2"

ARG nexturl="https://download.nextcloud.com/server/releases/"

RUN wget -q ${nexturl}${nextpackage}
RUN wget -q ${nexturl}${nextpackage}.asc

# Uncomment to validate signature check effectiveness
# RUN echo "kaputt" >> ${nextpackage}

RUN gpg --verify ./${nextpackage}.asc ./${nextpackage}

RUN tar --directory=/var/www/ -xjf ${nextpackage}

COPY nextcloud-site.conf /etc/apache2/conf-available/nextcloud.conf
RUN a2enconf nextcloud.conf && a2enmod rewrite && a2enmod headers && a2enmod env && a2enmod dir && a2enmod mime

RUN echo "apc.enable_cli = 1" > /etc/php/7.3/cli/conf.d/30-apc.ini

COPY config.php /var/www/nextcloud/config/config.php
RUN mkdir /var/www/nextcloud/custom_apps

EXPOSE 80

RUN chown -R www-data:www-data /var/www/nextcloud

COPY apache.sh /usr/local/bin/
RUN chmod u+x /usr/local/bin/apache.sh

ENTRYPOINT ["/usr/local/bin/apache.sh"]