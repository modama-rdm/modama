#!/bin/bash
set -eu
set -o pipefail

# If privkey/cert exist, enable the https vhost by symlink.
# Otherwise, just plain HTTP is enabled to get letsencrypt
# via HTTP running.
CFG_SITES_SRC=/etc/nginx/sites-available/nginx-site.conf
CFG_SITES_DST=/etc/nginx/sites-enabled/nginx-site.conf
FULLCHAIN=/etc/letsencrypt/ssl/fullchain.pem
if [ -f $FULLCHAIN ]; then
    ln -s -f $CFG_SITES_SRC $CFG_SITES_DST
fi
