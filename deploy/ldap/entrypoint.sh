#!/bin/bash
# Load the test data the first time, then run the server in the foreground.
set -euo pipefail

SUFFIX="dc=iff,dc=kfa-juelich,dc=de"
ROOTDN="cn=admin,$SUFFIX"
CONF=/run/slapd.conf

# slapd allocates per file descriptor and aborts with Docker's limit of about a billion
ulimit -n 1024

mkdir -p /run/slapd
chown openldap:openldap /run/slapd /var/lib/ldap

# Nobody needs this password but the load below, and it only works on this container's
# network, so a random one is fine
ROOTPW=$(head -c 24 /dev/urandom | base64)
install -m 640 -o root -g openldap /etc/ldap/slapd.conf.in "$CONF"
echo "rootpw $ROOTPW" >> "$CONF"

# StartTLS, if there is a certificate. The key is only readable for root where it comes
# from; slapd does not run as root.
if [ -e /certs/server/ldap.chain.pem ]; then
    install -d -m 750 -o root -g openldap /run/ldap-tls
    install -m 644 /certs/ca/pub/ca.cert.pem /certs/server/ldap.chain.pem /run/ldap-tls/
    install -m 640 -o root -g openldap /certs/server/ldap.priv.pem /run/ldap-tls/
    cat >> "$CONF" <<TLS
TLSCACertificateFile /run/ldap-tls/ca.cert.pem
TLSCertificateFile /run/ldap-tls/ldap.chain.pem
TLSCertificateKeyFile /run/ldap-tls/ldap.priv.pem
TLS
else
    echo "no certificate for ldap, running without StartTLS"
fi

SLAPD=(slapd -f "$CONF" -u openldap -g openldap)

if [ ! -e /var/lib/ldap/data.mdb ]; then
    echo "loading test data"
    # The server has to run for this: memberOf is maintained by an overlay that only
    # sees changes made through the server.
    "${SLAPD[@]}" -h "ldap://127.0.0.1:389/"
    until ldapsearch -x -H ldap://127.0.0.1 -b "" -s base > /dev/null 2>&1; do sleep 0.2; done
    ldapadd -x -H ldap://127.0.0.1 -D "$ROOTDN" -w "$ROOTPW" -f /test-data.ldif
    kill "$(cat /run/slapd/slapd.pid)"
    while [ -e /run/slapd/slapd.pid ]; do sleep 0.2; done
fi

# -d 256: stay in the foreground and log connections
exec "${SLAPD[@]}" -h "ldap://0.0.0.0:389/" -d 256
