# Development setup

## Formatting and linting

We are using `ruff` for formatting and linting of Python code; integrated using `pre-commit`. Please run `pre-commit install --install-hooks` to let the formatter and linter run before each commit.

You can also let `ruff` format the code for you, run for example `uvx ruff format <path-to-directory>`.

There's some configuration for `basedpyright` included, but it's type checking and linting
is not part of the CI pipeline yet.

## Test deployment

The test deployment is the whole system on your machine: Nextcloud with its database
and cache, the WebDAV gateway, `permissions`, `watchdog`, `smbdrop` and both management
interfaces. It needs Docker with Compose 2.20 or newer and nothing else on the host: no
`sudo`, no secrets, no LDAP.

```
git clone --recursive gitlab@iffgit.fz-juelich.de:weber/data-management-system.git
cd data-management-system/deploy
docker compose up -d
```

The first start builds all images, including the Rust service `smbdrop` and a
patched PHP for Nextcloud, which takes a good while. Later starts take seconds. Once
`docker compose ps` shows the containers as running, and `smbdrop` and
`mgmtinterface-api` as `healthy`, these are available:

| URL                                   | What                                                   |
|---------------------------------------|--------------------------------------------------------|
| <http://localhost:5001/>              | management interface (legacy, Flask)                   |
| <http://localhost:5001/new/>          | management interface (new, React)                      |
| <http://localhost:5001/mgmt-api/docs> | API of the new management interface                    |
| <http://localhost:1890/>              | landing page, with Nextcloud in `/nextcloud/`          |
| <http://localhost:8081/>              | WebDAV gateway                                         |
| `localhost:1446`                      | SMB shares of `smbdrop`                                |
| `localhost:5051`, `5052`, `5053`      | `smbdrop` APIs: HTTP, events (WebSocket), tokens (HTTPS) |

All ports are only reachable from the machine itself.

`docker compose down` stops everything and keeps the state, so the next `up` continues
where you left off. To start from scratch:

```
docker compose down --volumes
docker compose run --rm testenv-init clean
```

### What `docker compose up` sets up

The service `testenv-init` runs `testconf/init-testenv.sh` before the services that
need its results. It creates

* the folders in `testfolder/` that hold the state of the test system (the data
  folder, the Nextcloud installation and data, the database) with the owners the
  services expect, and
* a certificate authority in `testconf/secrets/CA/`, with the certificate of the
  `smbdrop` server. This is the same CA as in production (see
  [mTLS CA setup](./install.md#mtls-ca-setup)), but without a password.

It only creates what is missing, so running it again changes nothing. You can run it
on its own with `docker compose run --rm testenv-init`.

The other secrets are fixed test values, committed in `testconf/secrets/`. They protect
nothing: all ports are on `localhost`, and there are no real accounts or data. Never put
real secrets there.

Source code of `mgmtinterface`, `mgmtinterface-api` and `permissions` is mounted into
the containers, and these services restart when you change it.

### Users and logins

Production authenticates against LDAP through `sssd`. The test deployment has no LDAP
server, so `sssd` does not start. There are no test users yet: log-ins in the
management interface and the WebDAV gateway need local accounts in the `permissions`
and `webdav-backend` containers, and those are lost when the containers are recreated.
Nextcloud starts with only its administrator `root` (password `secret`) and is not
configured as in production, `applyconfig.py` is not run automatically.

### Using site-specific settings

To test against the real LDAP server, keep the site-specific files in
`/var/local/data-mgmt/`, as in production (see [Installation](./install.md)), and start
with

```
MODAMA_SECRETS_DIR=/var/local/data-mgmt COMPOSE_PROFILES=ldap docker compose up -d
```

`MODAMA_SECRETS_DIR` replaces `testconf/secrets/` and `COMPOSE_PROFILES=ldap` starts
`sssd`. Put both in `deploy/.env` to make them stick. The folder needs the same files as
`testconf/secrets/`, and `sssd.conf`. An existing CA in `CA/` is used as it is.

### If something does not start

* `docker compose logs <service>` first. `testenv-init` shows what it did.
* An error like "bind source path does not exist" for a *file* means that Docker could
  not create it. Docker does not create missing files, only folders.
* If a bind-mounted file shows up as a *folder*, Docker created it because the file was
  missing when the container started. Remove the folder and create the file.
* Ports are fixed. Another process using `1890`, `5001` or `8081` makes `up` fail.

## Nextcloud

For setting up the Nextcloud container for development for the first time, you need to configure its authentication. Currently, we authenticate against the production LDAP server. This needs the site-specific settings described under [Using site-specific settings](#using-site-specific-settings) above. The following steps are necessary:

* Make sure `/var/local/data-mgmt/nextcloud-config-secret.json` exists (can be an empty json file, i.e. `{"system": {}}`) and start the test deployment with `MODAMA_SECRETS_DIR=/var/local/data-mgmt`
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

## Documentation

This documentation is written using [mdBook](https://rust-lang.github.io/mdBook/index.html).

In short:

- `cargo install mdbook`
- `cd docs/guide`
- `mdbook serve --port 9991` (or similar)

## TODO

- [ ] document dev env for `modama-agent` and `smbdrop`

## Releasing

Releases are created with "New Release" in the Deploy->Releases view of the Gitlab repository. Create a version tag, set title (can just be the tag) and write short release notes.

Finally, click on the "release" button!
