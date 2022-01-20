#!/bin/bash

set -e

# --use-rfc2307 - Use AD to store posix attributes (recommended by samba docs)
# --domain      - A name for the domain
samba-tool domain provision --use-rfc2307 --domain=er-c-ingest --realm INGEST.LOCALHOST --server-role=dc --dns-backend=SAMBA_INTERNAL

cat << EOF >> /etc/samba/smb.conf
[users]
path = 
read only = no
force create mode = 0600
force directory mode = 0700

[homes]
comment = Home Directories
valid users = %S
read only = No
create mask = 0700
directory mask = 0700
browseable = No
EOF

mkdir -p /data/drop/new-dataset/
chmod 755 /data/drop/new-dataset/
chown 1000:1000 /data/drop/new-dataset/

chown root:users /var/lib/samba/usershares/
chmod 775 /var/lib/samba/usershares/
chmod +t /var/lib/samba/usershares/

samba-tool user create alex 'abcd;123' --uid-number=1000 --gid-number=1000 --unix-home=/data/drop/new-dataset/

FLASK_APP=ingest_mgmt_service:app flask run "$@"
