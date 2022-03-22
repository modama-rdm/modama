#!/bin/bash

mkdir -p data/adhoc nextcloud-data nextcloud-html postgres
chmod o-rwx -R nextcloud-data
chmod ugo+rwx data
chown 33:33 -R nextcloud-html nextcloud-data
chown root:root data data/adhoc
