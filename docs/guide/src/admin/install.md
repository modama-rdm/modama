# Installation

## Requirements

The system requires docker, see https://docs.docker.com/engine/install/

## Building from source

The script `build.sh` sets the necessary environment variables and then builds
the docker images using `docker compose`.

## Test and production deployment

Both a test and a production deployment are available. The configuration for the test deployment is defined by a combination of `docker-compose.yml` and `docker-compose.override.yml`, which is the default when using `docker compose up`. Test-specific configuration files except secrets are placed in `testconf/`.

The test deployment needs no preparation: `docker compose up` in `deploy/` creates the
folders in `testfolder/` that hold its state, and a CA for it, by itself. It is available under
http://localhost:1890/ (Nextcloud) and http://localhost:5001/ (management interface).
See [Development](./development.md#test-deployment) for details and how to reset it.

The configuration for production deployment is defined by a combination of `docker-compose.yml` and `docker-compose.prod.yml`. The commands to bring this up and down are contained in the `upprod.sh` and `downprod.sh` scripts. Production-specific configuration files except secrets are placed in `prodconf/`. The production configuration expects Nextcloud-specific data directories in `/var/local/data-mgmt/`. It also expects an NFS server exporting `/er-c-data` as a volume on localhost, as well as local access to the same data in `/er-c-data` for the management interface.

The production deployment expects files with site-specific secrets in `/var/local/data-mgmt/`. The test deployment uses throwaway values from `testconf/secrets/` instead, unless `MODAMA_SECRETS_DIR` points elsewhere.

At the moment, the test and production deployment only differ in a number of volumes and ports, meaning the Docker images themselves are the same. If that should change, the build scripts should be adapted to that.

For mgmtinterface the test deployment also overrides the source code copy with a read-only bind mount. Furthermore, it starts a Flask server in development mode instead of a production server. That way a change in the source code is immediately active in the container. This setup can/should be extended to other containers.

A production system should increase the number of permitted inotify watches to match the expected number of folders. See https://www.suse.com/support/kb/doc/?id=000020048 for details.

# Apply configuration

The `applyconfig.py` script can apply all settings to a fresh installation that
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

`/var/local/data-mgmt/modama_session_secret` signs the session cookies of the
`mgmtinterface-api` service. It is that service's equivalent of `SECRET_KEY`, read from a
mounted file rather than from config. Create it before the stack is started for the first
time after the update:

```
openssl rand -base64 48 > /var/local/data-mgmt/modama_session_secret
chmod 600 /var/local/data-mgmt/modama_session_secret
```

The service refuses to start without it. If the file is missing when `docker compose up`
runs, Docker creates a directory of that name in its place, which has to be removed before
the file can be created. `docker-compose.prod.yml` mounts the file read-only at
`/session_secret` and points `MODAMA_SESSION_SECRET_FILE` at it. Changing the file signs
everyone out.

## mTLS CA setup

Certificates and key material is stored in `/var/local/data-mgmt/CA/` in
production, which has to be created on deployment. The test deployment creates its own CA
automatically. There is a
command-line tool to manage the certificates, which can be run via
`deploy/ca.sh`. On initial deployment, run `deploy/ca.sh initialize-ca` to
generate a key pair and the CA certificate. Using `deploy/ca.sh enroll-server`
and `deploy/ca.sh enroll-agent`, additional servers and agents can be enrolled.
This will create keypairs and client- or server-certificates in the
aforementioned path.

The `smbdrop` service will expect a server with name `smbdrop` to be enrolled.

Since `/deploy/ca.sh` will run the `ca:latest` tag from
`iffregistry.fz-juelich.de/weber/data-management-system/`, one can build and tag
it locally for development work on this container or to work independent of the
container registry:

```
docker build -f modama-rs/Dockerfile-ca-cli modama-rs/ -t iffregistry.fz-juelich.de/weber/data-management-system/ca:latest
```

## sssd.conf

sssd.conf is private and placed in `/var/local/data-mgmt/` since it contains
usename and password used to bind to LDAP. Unfortunately we didn't manage yet to
split the configuration into generic and private parts. A redacted template with
the working IFF configuration is included in `./sssd/sssd.conf.template`. Note
that SSSD will only accept the config file if it is only readable by root. The LDAP
configuration is very similar to the Nextcloud LDAP configuration.

# Backup

The configuration for FZ Jülich IBM Spectrum Protect is only present for production, meaning any
`docker compose` command requires `-f docker-compose.yml -f docker-compose.prod.yml`. The necessary configuration and password are saved on iff1020 outside the repository. Resources for using the `dsmc` command line client: https://intranet.fz-juelich.de/de/organisation/it-portal/software_services/infrastruktur/datensicherung/nutzen-lernen
