#!/bin/bash
set -eu
set -o pipefail

# locations
CA_DIR=/etc/letsencrypt/ssl/ca
FULLCHAIN_PATH=/etc/letsencrypt/live/fullchain.pem
PRIVKEY_PATH=/etc/letsencrypt/live/privkey.pem
ACME=/opt/acme.sh/acme.sh

### prereqs ###
# print some vars for debugging
echo "ACME contact: $ACME_CONTACT"
echo "ACME domain: $ACME_DOMAIN"
echo "using CA server: $ACME_SERVER"

# ensure ca directory exists and has proper permissions
mkdir -p $CA_DIR

### main logic, wrapping acme.sh

_install_cert() {
    $ACME --install-cert --domain $ACME_DOMAIN \
	--home $CA_DIR \
        --key-file $PRIVKEY_PATH \
        --fullchain-file $FULLCHAIN_PATH \
        --reloadcmd "/usr/bin/reload-nginx.sh"
}

if [ ! -f $FULLCHAIN_PATH ]; then
    $ACME --server $ACME_SERVER --home $CA_DIR --issue --domain $ACME_DOMAIN --webroot /var/www/html/ # --debug
    _install_cert
else
    # install cert even if we didn't renew
    $ACME --server $ACME_SERVER --home $CA_DIR --renew --domain $ACME_DOMAIN --webroot /var/www/html/ || true
    _install_cert
fi
