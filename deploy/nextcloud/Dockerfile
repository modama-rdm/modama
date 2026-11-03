FROM nextcloud:23.0.3-apache
LABEL Dieter Weber <d.weber@fz-juelich.de>

ARG aptopts="-y --no-install-recommends"

# Debian 11 LTS ended 2026-08-31; bullseye-security moved to archive.debian.org on
# 2026-09-12. security.debian.org still serves the index but not the packages, so
# apt-get upgrade/install fail with 404 unless all suites come from the archive.
RUN sed -i \
    -e 's|http://deb.debian.org/debian|http://archive.debian.org/debian|g' \
    -e 's|http://security.debian.org/debian-security|http://archive.debian.org/debian-security|g' \
    /etc/apt/sources.list

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    apt-get -y update && apt-get -y upgrade && \
    apt-get ${aptopts} install iputils-ping procps cron sudo python3

# PHP: rebuild the interpreter with the PHP security team's minimal backport for
# CVE-2026-109618 / CVE-2026-109619 ("PHP2Shell", unauthenticated RCE via OOM /
# stale global state; Apache mod_php is the confirmed-exploitable SAPI). PHP 8.0
# is EOL, so there is no release; the fix is the single commit on the branch
# https://github.com/php/php-src/tree/PHP-8.0-CVE-2026-109618-109619
# (82f8f31b16a035c1b67cc253e79f7cc288f4cc20, "Avoid state corruption via bailout"),
# vendored unchanged as php-cve-2026-109618-109619.patch.
#
# The base image ships PHP 8.0.18. We move to the last 8.0 release (8.0.30, which
# the backport branch is based on and which contains the 8.0.19-8.0.30 security
# fixes as well) and apply the patch on top. The extension API (20200930) and the
# public struct layouts are unchanged within 8.0.x; the patch adds exported
# symbols and changes a few inline/macro helpers, none of which the precompiled
# extensions (gd, intl, imagick, redis, apcu, memcached, ...) rely on, so they
# are kept as-is. opcache, which does depend on Zend internals, is part of the
# source tree and is rebuilt. The build mirrors the official docker-library/php
# 8.0 Dockerfile: same configure options (taken from php-config), same
# CFLAGS/LDFLAGS from the image's environment. Integrity: the sha256 is the one
# pinned in docker-library/php and published on php.net for 8.0.30; the GPG
# signature is not verified (no key material in the image), so PHP_ASC_URL is
# intentionally not set.
#
# Behavioural changes of the backport worth knowing about:
#  * hitting memory_limit (or an allocation-size overflow) now terminates the
#    worker process with _exit(1) after printing "Fatal error: Allowed memory
#    size ..." to stderr. Nextcloud's shutdown handlers (lock release, tmp file
#    cleanup, nextcloud.log entry) do not run in that case; the client sees a
#    closed connection. Look for that line in the container log.
#  * session.upload_progress is force-disabled and multipart part headers other
#    than Content-Disposition/Content-Type are dropped. Neither affects
#    Nextcloud (uploads go through WebDAV PUT / chunking).
#
# This step must be removed or re-evaluated when the base image is bumped; the
# guard below makes the build fail if the base no longer ships PHP 8.0.
COPY ./php-cve-2026-109618-109619.patch /usr/src/php-cve-2026-109618-109619.patch

ENV PHP_VERSION=8.0.30 \
    PHP_URL=https://www.php.net/distributions/php-8.0.30.tar.xz \
    PHP_ASC_URL= \
    PHP_SHA256=216ab305737a5d392107112d618a755dc5df42058226f1670e9db90e77d777d9

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=target=/var/lib/apt/lists,type=cache,sharing=locked \
    set -eux; \
    case "$(php -r 'echo PHP_VERSION;')" in 8.0.*) ;; *) echo "base image no longer ships PHP 8.0, drop this step" >&2; exit 1;; esac; \
    php -i | grep -q '^PHP Extension => 20200930$'; \
    php -m | sort > /tmp/php-modules-before; \
    phpBuildDeps="apache2-dev libargon2-dev libcurl4-openssl-dev libonig-dev libreadline-dev libsodium-dev libsqlite3-dev libssl-dev libxml2-dev zlib1g-dev"; \
    apt-get -y update; \
    apt-get ${aptopts} install $phpBuildDeps; \
    cd /usr/src; \
    rm -f php.tar.xz php.tar.xz.asc; \
    curl -fsSL -o php.tar.xz "$PHP_URL"; \
    echo "$PHP_SHA256 *php.tar.xz" | sha256sum -c -; \
    docker-php-source extract; \
    cd /usr/src/php; \
    patch -p1 < /usr/src/php-cve-2026-109618-109619.patch; \
    export CFLAGS="$PHP_CFLAGS" CPPFLAGS="$PHP_CPPFLAGS" LDFLAGS="$PHP_LDFLAGS"; \
    ./configure $(php-config --configure-options); \
    make -j "$(nproc)"; \
    find -type f -name '*.a' -delete; \
    make install; \
    find /usr/local -type f -perm '/0111' -exec sh -euxc 'strip --strip-all "$@" || :' -- '{}' +; \
    cp -v php.ini-* "$PHP_INI_DIR/"; \
    cd /; \
    docker-php-source delete; \
    apt-get purge -y --auto-remove -o APT::AutoRemove::RecommendsImportant=false $phpBuildDeps; \
# smoke tests: right version, patch symbols present, every previously loaded extension still loads
    php --version | grep -q "^PHP $PHP_VERSION "; \
    nm -D /usr/lib/apache2/modules/libphp.so | grep -q zend_mm_safe_error_ex; \
    if php -m 2>&1 >/dev/null | grep .; then echo 'extension load warnings' >&2; exit 1; fi; \
    php -m | sort | diff /tmp/php-modules-before -; \
    rm /tmp/php-modules-before

# Advisory GHSA-6wmf-4rv5-p3rp recommends keeping session upload progress off; the
# patch already neutralises it, this makes the intent explicit.
RUN echo 'session.upload_progress.enabled=0' > "$PHP_INI_DIR/conf.d/zz-cve-2026-109619.ini"

RUN mkdir -p /usr/local/share/ca-certificates/fzj/

COPY ./HARICA_TLS_ECC_Root_CA_2021.crt ./HARICA_TLS_RSA_Root_CA_2021.crt /usr/local/share/ca-certificates/fzj/

RUN update-ca-certificates

COPY ./nextcloud-config.json /nextcloud-config.json
COPY ./externalstorage.json /externalstorage.json
COPY ./applyconfig.py /applyconfig.py

COPY ./nextcloud.patch /usr/src/nextcloud.patch
RUN cd /usr/src/nextcloud/ && patch -p2 -l --verbose < ../nextcloud.patch
