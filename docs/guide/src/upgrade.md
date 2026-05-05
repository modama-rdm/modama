# Upgrading

## How to upgrade Nextcloud

Currently, the additional auth mechanism that avoids saving user credentials in
the database is contained in nextcloud.patch. This patch is applied as part of
building the Docker image until this change is possibly upstreamed in the future.

In the Nextcloud Docker image there is a "baked in" version of the source code
in /usr/src/nextcloud and the "production" version of the source code in
/var/www/html. The "production" source code is overwritten with the baked-in
version if the upgrade script detects that the baked-in version is newer than
the production version, minus some configuration files.

For that reason, the patch is applied to the "baked in" Docker image. In order
to force an update in case the patch changes, the version.php is updated as
well. Here, the original version tuple is replaced with an extended one. This
has to be adapted to match the source version of the Docker image.

After the patch is applied successfully, the upgrade procedure should proceed
normally as described in the Nextcloud documentation.

