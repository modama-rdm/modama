# Development setup

## Formatting and linting

We are using `ruff` for formatting and linting of Python code; integrated using `pre-commit`. Please run `pre-commit install --install-hooks` to let the formatter and linter run before each commit.

You can also let `ruff` format the code for you, run for example `uvx ruff format <path-to-directory>`.

There's some configuration for `basedpyright` included, but it's type checking and linting
is not part of the CI pipeline yet.

## Nextcloud

For setting up the Nextcloud container for development for the first time, you need to configure its authentication. Currently, we authenticate against the production LDAP server. The following steps are necessary:

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
