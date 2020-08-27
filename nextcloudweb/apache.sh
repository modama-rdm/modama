#!/usr/bin/env sh

service apache2 restart

su - www-data php /var/www/nextcloud/occ maintenance:update:htaccess

tail -f /var/local/nextclouddata/nextcloud.log