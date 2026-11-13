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


## Patched PHP in the Nextcloud image

The Nextcloud image is based on `nextcloud:23.0.3-apache`, which ships PHP
8.0.18. PHP 8.0 is end-of-life, so the critical CVE-2026-109618 /
CVE-2026-109619 ("PHP2Shell") fixes never got a release. The Dockerfile
therefore rebuilds PHP in the image: it downloads PHP 8.0.30 (last 8.0
release, sha256-pinned), applies the PHP security team's minimal backport
(`deploy/nextcloud/php-cve-2026-109618-109619.patch`, byte-identical to
php-src commit 82f8f31b on branch `PHP-8.0-CVE-2026-109618-109619`) and
installs it over the base image's PHP. The precompiled extensions are kept;
the build fails if any of them stops loading.

Consequences and caveats:

* A request that exceeds `memory_limit` now terminates the Apache worker
  immediately instead of raising a regular PHP fatal error. Nextcloud's
  shutdown handlers do not run in that case, so such requests only show up as
  `Fatal error: Allowed memory size ...` in the container log, not in
  nextcloud.log.
* The rebuilt PHP lives in the image, not in the `nextcloud-html` volume. A
  full container recreation (`up -d` after pulling the image) is required;
  `docker compose restart nextcloud` does not pick up the new image.
* When the base image is bumped, this step must be removed or re-evaluated.
  It is obsolete once the base image ships PHP 8.2.35, 8.3.36, 8.4.27 or
  8.5.12 or later. The Dockerfile refuses to build if the base is not PHP 8.0.
* This is a stop-gap: Nextcloud 23 and PHP 8.0 remain end-of-life with other
  unpatched issues.
