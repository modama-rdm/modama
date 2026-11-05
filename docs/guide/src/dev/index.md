# Development setup

## Formatting and linting

We are using `ruff` for formatting and linting of Python code; integrated using `pre-commit`. Please run `pre-commit install --install-hooks` to let the formatter and linter run before each commit.

You can also let `ruff` format the code for you, run for example `uvx ruff format <path-to-directory>`.

There's some configuration for `basedpyright` included, but it's type checking and linting
is not part of the CI pipeline yet.

## Test deployment

The test deployment is the whole system on your machine: Nextcloud with its database
and cache, the WebDAV gateway, `permissions`, `watchdog`, `smbdrop`, both management
interfaces, `sssd` and a directory server with a few test users instead of the production
LDAP. It needs Docker with Compose 2.20 or newer and nothing else on the host: no `sudo`,
no secrets, no access to the production LDAP.

```
git clone --recursive gitlab@iffgit.fz-juelich.de:weber/data-management-system.git
cd data-management-system/deploy
./uptest.sh
./runtest.sh
```

`uptest.sh` is to the test deployment what `upprod.sh` is to production: it runs
`docker compose up -d`, waits until the services are healthy and Nextcloud has installed
itself, and then applies the Nextcloud configuration (LDAP, external storage) with
`applyconfig.py`, which Nextcloud's own start does not. `runtest.sh` then
[checks](#checking-the-deployment) that everything works. Arguments of `uptest.sh` go to
`docker compose up`, for example `./uptest.sh --build` to rebuild images.

The first start builds all images, including the Rust service `smbdrop` and a
patched PHP for Nextcloud, which takes a good while. Later starts take seconds. Plain
`docker compose up -d` works too, it just leaves out the Nextcloud configuration. Once
`docker compose ps` shows the containers as running, and `ldap`, `smbdrop` and
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

`./downtest.sh` stops everything and keeps the state, so the next `uptest.sh` continues
where you left off. To start from scratch, use `./downtest.sh --reset`. It deletes the
state, the volumes of the directory server and the CA, which belong together:
Nextcloud remembers the users of the directory by ID, and a new directory has new IDs.

### What `docker compose up` sets up

The service `testenv-init` runs `testconf/init-testenv.sh` before the services that
need its results. It creates

* the folders in `testfolder/` that hold the state of the test system (the data
  folder, the Nextcloud installation and data, the database) with the owners the
  services expect, and
* a certificate authority in `testconf/secrets/CA/`, with the certificates of the
  `smbdrop` server and the test directory server `ldap`. This is the same CA as in production (see
  [mTLS CA setup](./install.md#mtls-ca-setup)), but without a password.

`testenv-init` only creates what is missing; you can re-run it
on its own with `docker compose run --rm testenv-init` to ensure the environment is set up properly.

The other secrets are fixed test values, committed in `testconf/secrets/`. They protect
nothing: all ports are on `localhost`, and there are no real accounts or data. Never put
real secrets there.

Source code of `mgmtinterface`, `mgmtinterface-api` and `permissions` is mounted into
the containers, and these services restart when you change it. They run the development
servers of their respective frameworks, so their behavior might differ slightly from
production, for example debug mode might be enabled.

### Users and logins

The service `ldap` is a directory server with the same tree as the production
(FreeIPA) directory, `cn=accounts,dc=iff,dc=kfa-juelich,dc=de`. `sssd` and Nextcloud use it
as they use the production LDAP, so the services see the users like in production:
through `sssd` and PAM, with the group `iff` as the condition for access.

| User      | Password       | Group     |                                           |
|-----------|----------------|-----------|-------------------------------------------|
| `alice`   | `testpassword` | `iff`     |                                           |
| `bob`     | `testpassword` | `iff`     |                                           |
| `carol`   | `testpassword` | `iff`     |                                           |
| `mallory` | `testpassword` | `outsiders` | exists, but is denied access by `sssd`, as an account outside `iff` is in production |

These accounts are defined in `ldap/test-data.ldif`; edit it to add users. The directory
is loaded when its volume is created, so changes need `./downtest.sh --reset`.
`ldapbind` (password `bind-test-password`) is the read-only account that `sssd` and
Nextcloud search with. The server is only reachable from the other containers; to look at
it, run `ldapsearch` in the container, for example
`docker compose exec ldap ldapsearch -x -LLL -H ldap://localhost -D uid=ldapbind,cn=sysaccounts,cn=etc,dc=iff,dc=kfa-juelich,dc=de -w bind-test-password -b cn=accounts,dc=iff,dc=kfa-juelich,dc=de uid`.
All of this is test data; the passwords are in the repository.

To check that a user is known to a service:

```
docker compose exec permissions getent passwd alice
docker compose exec permissions getent group iff
```

Nextcloud itself comes up with only its administrator `root` (password `secret`).
`uptest.sh` sets up the LDAP connection and the external storage `ER-C-Data`, as in
production, with `applyconfig.py`; that also disables `root`. (With plain
`docker compose up`, run `docker compose exec nextcloud python3 /applyconfig.py` once
Nextcloud has installed itself.) Then `alice` can log in at <http://localhost:1890/nextcloud/>. Her
Nextcloud user name is not `alice` but the `entryUUID` of her entry, as in production;
`docker compose exec nextcloud sudo -u www-data php /var/www/html/occ user:list` shows
the names, which WebDAV URLs need (`/nextcloud/remote.php/dav/files/<name>/`).

### Testing against the production LDAP

Sometimes only the real directory will do. This is possible without a production
deployment: keep the site-specific files in `/var/local/data-mgmt/`, as in production (see
[Installation](./install.md)), and start the test deployment with

```
MODAMA_SECRETS_DIR=/var/local/data-mgmt docker compose up -d
```

`MODAMA_SECRETS_DIR` replaces `testconf/secrets/`, which holds the settings for the test
directory. Put it in `deploy/.env` to make it stick. The folder needs `sssd.conf`,
`nextcloud-config-secret.json` (with the LDAP host, port, bind DN and password, see
[Nextcloud](#nextcloud)), and `mgmt_secret.py` and `externalstorage_pw`, as in production.
A CA in `CA/` is used as it is; if there is none, one is created for `smbdrop`. The test
directory still starts, but nothing uses it. The data of the test deployment is
the same either way, so after a switch `./downtest.sh --reset` avoids Nextcloud
users of the other directory.

### Checking the deployment

`./runtest.sh` checks the running deployment and prints a line per check, `PASS` or `FAIL`
with the reason. It exits with 1 if any check failed, so it can be used in scripts. It
checks that

* all services run, the ones with a health check are healthy, and `testenv-init`
  succeeded and finds everything in place if run again,
* the web endpoints answer as expected (management interfaces and API, `smbdrop`,
  landing page, Nextcloud, WebDAV gateway),
* the certificate of `smbdrop` is issued by the test CA for its name, and not trusted
  otherwise,
* the directory has `memberOf`, does not give out passwords and needs a bind,
* the test users and the group `iff` are known to `permissions` and the WebDAV gateway,
  and log in: right password, wrong password, and `mallory` who is outside `iff`,
* `alice` logs in to Nextcloud and reaches `ER-C-Data` through the gateway, which uses
  every part of the chain from Nextcloud to the directory.

If `sssd` is set up for another directory (`MODAMA_SECRETS_DIR`, see below), the checks of
the test users are skipped. The checks are plain shell functions in `runtest.sh`, run with
`check "what" command...`; when you add a service or a feature, add a check next to the
existing ones. Check that it can fail, too: stop a service and run the script.

### If something does not start

* `docker compose logs <service>` first. `testenv-init` shows what it did.
* An error like "bind source path does not exist" for a *file* means that Docker could
  not create it. Docker does not create missing files, only folders.
* If a bind-mounted file shows up as a *folder*, Docker created it because the file was
  missing when the container started. Remove the folder and create the file.
* Ports are fixed. Another process using `1890`, `5001` or `8081` makes `up` fail.

## Nextcloud

For setting up the Nextcloud container for development for the first time, you need to configure its authentication. Currently, we authenticate against the production LDAP server. In the test deployment this works out of the box, see [Users and logins](#users-and-logins). To use the production LDAP server instead, which needs the site-specific settings described under [Testing against the production LDAP](#testing-against-the-production-ldap) above, the following steps are necessary:

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

The version tag should be in form `vX.Y.Z`.

Finally, click on the "release" button! This will trigger a `tag` run of the CI pipeline, which will in addition to the normal CI run, re-tag the container images at the end to both `latest` and the `vX.Y.Z` tag.
