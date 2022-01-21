#!/bin/bash

set -x
set -e

trap 'chattr -iR /data/;rm -rf /data/holo /data/privateproject /data/sharing /data/adhoc' EXIT

# Start watchdog to 
/watchdog.sh &

# Allow watchdog to start
sleep 1

# Users can only create new projects through the correct process,
# not by just creating a top-level folder
sudo -u testdieter mkdir /data/rogueproject && { echo "failure";exit 1; } || echo "failed successfully"
# Also no rogue top-level files
sudo -u testdieter touch /data/roguefile && { echo "failure";exit 1; } || echo "failed successfully"

echo "Add project"
mkdir /data/holo
chown testdieter:testholo /data/holo
# g+s: inherit group ownership
# +t: sticky bit, prevent deletion
chmod ug+rwx,o-w,g+s,+t /data/holo
# Unfortunately the sticky bit can't be inherited or set via ACL:
# https://unix.stackexchange.com/questions/209009/set-sticky-bit-by-default-for-new-directories-via-acl
# Using a watchdog process watchdog.sh as proposed there

# Default ACL, inherited: group write permission
setfacl -m d:g:testholo:rwX /data/holo
ls -l /data

echo "Create new dataset"
mkdir -p /data/holo/tecnai/raw
chown testdieter:testholo /data/holo/tecnai /data/holo/tecnai/raw
# For new datasets we can set the sticky bit, just to be sure
chmod ug+rwx,o-w,+t /data/holo/tecnai /data/holo/tecnai/raw

echo "Add measurement and user file"
sudo -u testthibaud touch /data/holo/tecnai/raw/image1.dm4
sudo -u testdieter touch /data/holo/tecnai/raw/README.rst

echo "Others can't create"
sudo -u testjanghyun touch /data/holo/tecnai/raw/rogue.file && { echo "failure";exit 1; } || echo "failed successfully"

echo "Seal dataset"
chattr -R +i /data/holo/tecnai/raw/

echo "Confirm immutable"
sudo -u testdieter rm /data/holo/tecnai/raw/README.rst && { echo "failure";exit 1; } || echo "failed successfully"
rm /data/holo/tecnai/raw/README.rst && { echo "failure";exit 1; } || echo "failed successfully"
dd if=/dev/zero of=/data/holo/tecnai/raw/README.rst bs=1 count=1 && { echo "failure";exit 1; } || echo "failed successfully"
sudo -u testdieter dd if=/dev/zero of=/data/holo/tecnai/raw/README.rst bs=1 count=1 && { echo "failure";exit 1; } || echo "failed successfully"
sudo -u testdieter touch /data/holo/tecnai/raw/newfile && { echo "failure";exit 1; } || echo "failed successfully"

echo "Confirm readable"
sudo -u testthibaud dd if=/data/holo/tecnai/raw/README.rst of=/dev/null
echo "Confirm readable by others outside the group"
sudo -u testjanghyun dd if=/data/holo/tecnai/raw/README.rst of=/dev/null

echo "Add mutable files by users"

sudo -u testdieter mkdir /data/holo/tecnai/analyses
sudo -u testdieter touch /data/holo/tecnai/analyses/notebook1

echo "Others can't create"
sudo -u testjanghyun touch /data/holo/tecnai/analyses/rogue.file && { echo "failure";exit 1; } || echo "failed successfully"

# Other users in the group can add to folders that were created by group members
sleep 2  # for watchdog
sudo -u testpenghan cp /data/holo/tecnai/analyses/notebook1 /data/holo/tecnai/analyses/notebook2

# Users can modify their files
sudo -u testdieter dd if=/dev/zero of=/data/holo/tecnai/analyses/notebook1 bs=1 count=1

# But other group members should not be allowed overwrite or delete each other's files
# See above, sticky bit not inheritable or settable with default ACL
# Sticky bit added by watchdog, threfore
sudo -u testpenghan rm /data/holo/tecnai/analyses/notebook1 && { echo "failure";exit 1; } || echo "failed successfully"

# Unfortunately, the default ACL is the same for files and directories,
# that means new files are also writable by all group members
# https://serverfault.com/a/97786
# Group write permission removed by watchdog, therefore
sudo -u testpenghan dd if=/dev/zero of=/data/holo/tecnai/analyses/notebook1 bs=1 count=1 && { echo "failure";exit 1; } || echo "failed successfully"


echo "Add private project"
mkdir /data/privateproject
chown testpenghan:testprivate /data/privateproject
# g+s: inherit group ownership
# +t: sticky bit, prevent deletion
# Sticky bit set to subfolders by watchdog
# Somehow the o-rwx is inherited by subfolders, might have sth to do with ACLs, sticky bit, suid?
# It works anyway, and ACLs allow to share with a designated circle of people
chmod ug+rwx,o-rwx,g+s,+t /data/privateproject

# Default ACL, inherited: group write permission
setfacl -m d:g:testprivate:rwX /data/privateproject

echo "Create new private dataset"
mkdir -p /data/privateproject/tecnai/raw
chown testthibaud:testprivate /data/privateproject/tecnai /data/privateproject/tecnai/raw

# For new datasets we can set the sticky bit, just to be sure
chmod ug+rwx,o-w,+t /data/privateproject/tecnai /data/privateproject/tecnai/raw

echo "Add measurement and user file"
sudo -u testthibaud touch /data/privateproject/tecnai/raw/image1.dm4
sudo -u testpenghan touch /data/privateproject/tecnai/raw/README.rst

ls -lR /data/privateproject/tecnai/

echo "Others can't create or read"
sudo -u testdieter touch /data/privateproject/tecnai/raw/rogue.file && { echo "failure";exit 1; } || echo "failed successfully"
sudo -u testdieter dd if=/data/privateproject/technai/raw/image1.dm4 of=/dev/null && { echo "failure";exit 1; } || echo "failed successfully"

echo "Seal dataset"
chattr -R +i /data/privateproject/tecnai/raw/

echo "Create user folder in private project"
sudo -u testthibaud mkdir /data/privateproject/analyses

sudo -u testpenghan touch /data/privateproject/analyses/notebook1
sudo -u testthibaud touch /data/privateproject/analyses/notebook2

echo "Others can't create or read"
sudo -u testdieter touch /data/privateproject/analyses/rogue.file && { echo "failure";exit 1; } || echo "failed successfully"

# Not listable for people outside the group
sudo -u testdieter ls /data/privateproject/ && { echo "failure";exit 1; } || echo "failed successfully"
# Not readable for people outside the group
sudo -u testdieter dd if=/data/privateproject/analyses/notebook1 of=/dev/null && { echo "failure";exit 1; } || echo "failed successfully"


echo "Create sharing folder"
mkdir /data/sharing

echo "Share a private dataset with others"
# TODO record the immutable flag for all files and re-set it exactly as it was

echo "Unseal dataset"
chattr -R -i /data/privateproject/tecnai/raw/

echo "Link files"
cp -lR /data/privateproject/tecnai /data/sharing/sharedtechnai
# Give read access to group for share
setfacl -R -m g:testshare:rX /data/sharing/sharedtechnai

echo "Reseal dataset"
chattr -R +i /data/privateproject/tecnai/raw/

# Remove all write permission to folders to prevent adding actual data
# to it -- allows shares to be deleted safely
# Only do this to directories since the files are hard links and shouldn't be modified
# Not sure if this is 100 % necessary, but probably good to ensure
find /data/sharing/sharedtechnai -type d -print0 | xargs -0 chmod ugo-w

echo "Others can read the shared files"
sudo -u testdieter dd if=/data/sharing/sharedtechnai/raw/image1.dm4 of=/dev/null

echo "Nobody can add new files to a share"
sudo -u testdieter touch /data/sharing/sharedtechnai/rogue.file && { echo "failure";exit 1; } || echo "failed successfully"
sudo -u testthibaud touch /data/sharing/sharedtechnai/rogue.file && { echo "failure";exit 1; } || echo "failed successfully"

echo "Nobody can add top-level shares"
sudo -u testthibaud touch /data/sharing/rogue.file && { echo "failure";exit 1; } || echo "failed successfully"

# ls -lR /data/
# getfacl -R /data/

# Ad-hoc projects should work without Unix group
mkdir /data/adhoc

echo "Add ad-hoc project"
mkdir /data/adhoc/kidney
chown testthibaud:testerc /data/adhoc/kidney
# g+s: inherit group ownership
# +t: sticky bit, prevent deletion
chmod u+rwx,go-w,g+s,+t /data/adhoc/kidney
# Unfortunately the sticky bit can't be inherited or set via ACL:
# https://unix.stackexchange.com/questions/209009/set-sticky-bit-by-default-for-new-directories-via-acl
# Using a watchdog process watchdog.sh as proposed there

# Default ACL, inherited, as well as actual ACL: write permission for ad-hc users
setfacl -m d:u:testthibaud:rwX,u:testthibaud:rwX /data/adhoc/kidney
setfacl -m d:u:testdieter:rwX,u:testdieter:rwX /data/adhoc/kidney
setfacl -m d:u:testpenghan:rwX,u:testpenghan:rwX /data/adhoc/kidney
ls -l /data

echo "Create new dataset"
mkdir -p /data/adhoc/kidney/tecnai/raw
chown testpenghan:testerc /data/adhoc/kidney/tecnai /data/adhoc/kidney/tecnai/raw
# Don't chmod since that messes up the inherited ACLs
# Rely on watchdog to set sticky bit
# SGID bit is inherited
# echo 'don't chmod u+rwx,go-w,+t /data/adhoc/kidney/tecnai /data/adhoc/kidney/tecnai/raw'

echo "Add measurement and user file"
sudo -u testthibaud touch /data/adhoc/kidney/tecnai/raw/image1.dm4
sudo -u testdieter touch /data/adhoc/kidney/tecnai/raw/README.rst

echo "Others can't create"
sudo -u testjanghyun touch /data/adhoc/kidney/tecnai/raw/rogue.file && { echo "failure";exit 1; } || echo "failed successfully"

echo "Seal dataset"
chattr -R +i /data/adhoc/kidney/tecnai/raw/

echo "Confirm immutable"
sudo -u testdieter rm /data/adhoc/kidney/tecnai/raw/README.rst && { echo "failure";exit 1; } || echo "failed successfully"
rm /data/adhoc/kidney/tecnai/raw/README.rst && { echo "failure";exit 1; } || echo "failed successfully"
dd if=/dev/zero of=/data/adhoc/kidney/tecnai/raw/README.rst bs=1 count=1 && { echo "failure";exit 1; } || echo "failed successfully"
sudo -u testdieter dd if=/dev/zero of=/data/adhoc/kidney/tecnai/raw/README.rst bs=1 count=1 && { echo "failure";exit 1; } || echo "failed successfully"
sudo -u testdieter touch /data/adhoc/kidney/tecnai/raw/newfile && { echo "failure";exit 1; } || echo "failed successfully"

echo "Confirm readable"
sudo -u testthibaud dd if=/data/adhoc/kidney/tecnai/raw/README.rst of=/dev/null
echo "Confirm readable by others outside the group"
sudo -u testjanghyun dd if=/data/adhoc/kidney/tecnai/raw/README.rst of=/dev/null

echo "Add mutable files by users"

sudo -u testdieter mkdir /data/adhoc/kidney/analyses
sudo -u testdieter touch /data/adhoc/kidney/analyses/notebook1

echo "Others can't create"
sudo -u testjanghyun touch /data/adhoc/kidney/analyses/rogue.file && { echo "failure";exit 1; } || echo "failed successfully"

# Other users in the group can add to folders that were created by group members
sleep 2  # for watchdog
sudo -u testpenghan cp /data/adhoc/kidney/analyses/notebook1 /data/holo/tecnai/analyses/notebook2

# Users can modify their files
sudo -u testdieter dd if=/dev/zero of=/data/adhoc/kidney/analyses/notebook1 bs=1 count=1

# But other group members should not be allowed overwrite or delete each other's files
# See above, sticky bit not inheritable or settable with default ACL
# Sticky bit added by watchdog, threfore
sudo -u testpenghan rm /data/adhoc/kidney/analyses/notebook1 && { echo "failure";exit 1; } || echo "failed successfully"

# Unfortunately, the default ACL is the same for files and directories,
# that means new files are also writable by all group members
# https://serverfault.com/a/97786
# Other user write permission removed by watchdog, therefore
sudo -u testpenghan dd if=/dev/zero of=/data/adhoc/kidney/analyses/notebook1 bs=1 count=1 && { echo "failure";exit 1; } || echo "failed successfully"

echo "Run Python database test suite"
python3 /api.py

echo "Cleanup"
chattr -iR /data/holo/tecnai/raw/
chattr -iR /data/privateproject/tecnai/raw/
chattr -iR /data/adhoc/kidney/tecnai/raw/
chattr -iR /data/adhoc/pythoncreated/helloworld/raw/


rm -rf /data/holo
rm -rf /data/adhoc
rm -rf /data/privateproject
rm -rf /data/sharing
