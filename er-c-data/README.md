# Cloning

This project uses git submodules. You can pull all of them automatically when cloning, like this:

```
git clone --recursive gitlab@iffgit.fz-juelich.de:weber/data-management-system.git
```

Or, after cloning, initialize and clone the submodules:

```
git submodule init && git submodule update
```

# Installing requirements

The system requires docker, see https://docs.docker.com/engine/install/

It also requires `sssd` running on the host: `sudo apt install sssd` on Debian.

# Building

The script `build.sh` sets the necessary environment variables and then builds
the docker images using `docker compose`.

# Test and production deployment

Both a test and a production deployment are available. The configuration for the test deployment is defined by a combination of `docker-compose.yml` and `docker-compose.override.yml`, which is the default when using `docker compose up`. Test-specific configuration files except secrets are placed in `testconf/`.

The test deployment uses files in the `testfolder/` directory. The necessary
subfolders with correct ownership and permissions can be created by running
`sudo ./preparefolders.sh` in that directory. It is available under
http://localhost:1890/ and http://localhost:1890/. `sudo ./nukefolders.sh`
removes the folders.

The configuration for production deployment is defined by a combination of `docker-compose.yml` and `docker-compose.prod.yml`. The commands to bring this up and down are contained in the `upprod.sh` and `downprod.sh` scripts. Production-specific configuration files except secrets are placed in `prodconf/`. The production configuration expects Nextcloud-specific data directories in `/var/local/data-mgmt/`. It also expects an NFS server exporting `/er-c-data` as a volume on localhost, as well as local access to the same data in `/er-c-data` for the management interface.

Both test and production deployment expect files with site-specific secrets in `/var/local/data-mgmt/`.

At the moment, the test and production deployment only differ in a number of volumes and ports, meaning the Docker images themselves are the same. If that should change, the build scripts should be adapted to that.

For mgmtinterface the test deployment also overrides the source code copy with a read-only bind mount. Furthermore, it starts a Flask server in development mode instead of a production server. That way a change in the source code is immediately active in the container. This setup can/should be extended to other containers.

A production system should increase the number of permitted inotify watches to match the expected number of folders. See https://www.suse.com/support/kb/doc/?id=000020048 for details.

# How to upgrade Nextcloud

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

# Apply configuration

The applyconfig.py script can apply all settings to a fresh installation that
are required to bring it to a working state in the ER-C data management. It can
be run with `docker compose exec nextcloud python3 /applyconfig.py`. It is not run automatically
since the entrypoint script for Nextcloud is rather complex and doesn't allow to
include custom extensions at the right point in the installation procedure.

It manages the following items:

* Disable `root` user
* Enable required apps
* Initialize and import config
* Create mount for external storage
* Theming

Sensitive settings are kept in separate read-protected files outside the repository, under `/var/local/data-mgmt/`.

The externally supplied file `externalstorage_pw` contains the preshared password for the WebDAV gateway.

## Secrets

`/var/local/data-mgmt/mgmt_secret.py`

```
SECRET_KEY = 'I am so super secret'
```

## NextCloud development setup

For setting up the NextCloud container for development for the first time, you need to configure its authentication. Currently, we authenticate against the production LDAP server. The following steps are necessary:

* Make sure `/var/local/data-mgmt/nextcloud-config-secret.json` exists (can be an empty json file, i.e. `{"system": {}}`)
* Set the correct LDAP settings
    * Run in the `nextcloud` container: `sudo -u www-data php -d memory_limit=-1 /var/www/html/occ user:enable root`
    * Disable any existing LDAP authentication; run `sudo -u www-data php -d memory_limit=-1 /var/www/html/occ ldap:delete s01`
    * Go to http://localhost:1890/nextcloud/ and log in with username `root`, password `secret`
    * Go to the LDAP configuration and enter the correct settings for your environment
        * You can use the default production values in `nextcloud/nextcloud-config.json` as starting values and adapt as needed.
        * Should be accessible at http://localhost:1890/nextcloud/settings/admin/ldap
        * Hostname, port (auto-detection doesn't seem to work), credentials, filters etc.
        * Make sure the wizard is happy with your settings!
* Run in the `nextcloud` container: `sudo -u www-data php -d memory_limit=-1 /var/www/html/occ config:list --private`
* Update the file `/var/local/data-mgmt/nextcloud-config-secret.json` to look like the following:

```
{
    "system": {
        "passwordsalt": "...",
        "secret": "..."
    },
    "apps": {
        "user_ldap": {
            "s01ldap_agent_password": "...",
            "s01ldap_dn": "..."
        }
    }
}
```

(fill the values with the ones from the `occ config:list` output above; you may
need additional keys based on your environment, for example the correct LDAP
server name, base, filters etc...).

Note that the agent password above is not encrypted, just base64 encoded, so
it's a good idea to use an LDAP account with minimal permissions, and/or use
a separate LDAP server for development.
Make sure the `nextcloud-config-secret.json` file has proper permissions, i.e.
owned by root and not world-readable.

After changing this file, restart the `nextcloud` container, as editing the file
may actually create a new file that needs to be re-bind-mounted into the
container. When this is done, re-generate the configuration using
`docker compose exec nextcloud python3 /applyconfig.py`. This will also disable
the `root` user again. Then, login via LDAP should work.

# Create database for SampleDB

The sampledb container is configured to expect the `sampledb` database. It will not be created automatically. In order to create it, run `docker compose exec db su - postgres -c "/usr/bin/createdb sampledb"`.

# Backup

The configuration for FZ Jülich IBM Spectrum Protect is only present for production, meaning any
`docker compose` command requires `-f docker-compose.yml -f docker-compose.prod.yml`. The necessary configuration and password are saved on iff1020 outside the repository. Resources for using the `dsmc` command line client: https://intranet.fz-juelich.de/de/organisation/it-portal/software_services/infrastruktur/datensicherung/nutzen-lernen
