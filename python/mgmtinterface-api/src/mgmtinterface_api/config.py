import logging
import secrets
from functools import lru_cache
from pathlib import Path

from pydantic import PrivateAttr
from pydantic_settings import BaseSettings, SettingsConfigDict

log = logging.getLogger(__name__)

# All routes of this service are under this prefix. nginx routes it here and
# forwards the full path. It carries no version number; the contract version is
# in the `X-Modama-API-Version` header (see `versioning.py`). It is defined
# here rather than in the package root because both the app factory and the
# versioning layer need it, and importing the root from a submodule would be a
# circular import.
API_PREFIX = "/mgmt-api"

# Default name of the session cookie. Module-level because the OpenAPI security
# scheme in `security.py` needs the name at import time, before a `Settings`
# instance exists. A deployment can still override the setting; the OpenAPI
# document then shows the default name rather than the configured one.
DEFAULT_SESSION_COOKIE_NAME = "modama_session"

#: The name with the `__Host-` prefix, which is what is sent whenever the cookie
#: is Secure. See `Settings.effective_cookie_name`.
HOST_PREFIXED_SESSION_COOKIE_NAME = f"__Host-{DEFAULT_SESSION_COOKIE_NAME}"


class Settings(BaseSettings):
    """
    Runtime configuration, read from the environment with the ``MODAMA_``
    prefix.

    Unlike the Flask app, which loads a Python file named by
    ``MGMTINTERFACE_CONFIG``, this service is configured through environment
    variables, so the container needs no bind-mounted config file. Secrets are
    the exception: the deployment mounts them as files, and
    `session_secret_file` names the file to read.
    """

    model_config = SettingsConfigDict(env_prefix="MODAMA_")

    permissions_api: str = "http://permissions:5050"
    # smbdrop, which creates the Samba shares for ingest. Shares are addressed
    # by the path of the session as smbdrop sees it, which is
    # `smb_base/<project relpath>/<session>`; both containers mount the data
    # tree at the same place.
    share_api: str = "http://smbdrop:5051"
    smb_base: str = "/data"
    # Host and workgroup the web interface shows a person for connecting to an
    # ingest share by hand, as seen from the intranet. The share service does
    # not report them, so the API supplies them with the share. modama-agent
    # does not use them: an instrument computer may reach the Samba server another
    # way, so the agent mounts with its own configured host, port and domain.
    ingest_smb_host: str = "moellenstedt.iff.kfa-juelich.de"
    ingest_workgroup: str = "ER-C-INGEST"
    # smbdrop's token service, which checks the `X-Agent-Token` modama-agent
    # sends to bind an ingest share to its resource. It speaks https with a
    # certificate from the deployment's own CA, so the CA certificate is needed
    # to trust it; the deployment mounts it where the Flask app reads it.
    token_api: str = "https://smbdrop:5053"
    token_ca_cert: str = "/certs/ca/pub/ca.cert.pem"

    # The WebDAV server in front of the data tree, which the browse endpoint
    # lists folders through. That server authenticates by username alone
    # (wsgidav's `useronly` controller) and serves each request as that Unix
    # user, so the file system decides what a user sees. A `PROPFIND` of a
    # folder with tens of thousands of entries takes well under a second; the
    # timeout is for a hung server, not a slow folder.
    webdav_api: str = "http://webdav:8080"
    webdav_timeout_seconds: int = 30

    # Where a browsed path opens elsewhere: the bases the browse endpoint builds
    # its links from. The defaults are the production values, as in the Flask
    # app's `default_config.py`; the test stack overrides the Nextcloud one.
    nextcloud_base: str = (
        "https://er-c-data.fz-juelich.de/nextcloud/apps/files/?dir=/ER-C-Data"
    )
    jupyter_base: str = (
        "https://moellenstedt.iff.kfa-juelich.de/user-redirect/lab/tree/er-c-data"
    )
    libertem_base: str = (
        "https://moellenstedt.iff.kfa-juelich.de/user-redirect/libertem/"
        "#action=open&path=/storage/er-c-data"
    )
    image_viewer_base: str = (
        "https://moellenstedt.iff.kfa-juelich.de/user-redirect/image-viewer/"
        "?path=/storage/er-c-data"
    )
    sampledb_base: str = "https://erc-samples.fz-juelich.de/"
    sampledb_erc3_base: str = "https://erc-3-samples.fz-juelich.de/"
    elabftw_erc3_base: str = "https://erc3elab.app.fz-juelich.de/"
    # Transfer targets: the ssh host with the data tree's path on it for rsync
    # and sftp, and the Samba share as a UNC path for Windows and as an smb://
    # URL for everything else.
    moellenstedt_base: str = "moellenstedt.iff.kfa-juelich.de:/storage/er-c-data"
    main_smb_base: str = "\\\\er-c-data-smb.fz-juelich.de\\er-c-data"
    smb_url_base: str = "smb://er-c-data-smb.fz-juelich.de/er-c-data"

    # Signed-cookie session. The name must differ from the Flask app's
    # `mgmtinterface_session`, since both run on the same origin until the
    # cutover (#170).
    session_cookie_name: str = DEFAULT_SESSION_COOKIE_NAME
    session_secret: str = ""
    session_secret_file: str = ""
    # With this set, an unconfigured session secret becomes a random per-boot
    # key instead of a startup failure. Development and tests only.
    dev_mode: bool = False
    # `max_age` limits the session regardless of activity, `idle` ends it after a
    # period without requests. Refreshing the cookie extends the idle window but
    # not the absolute one.
    session_max_age_seconds: int = 12 * 60 * 60
    session_idle_seconds: int = 2 * 60 * 60
    # Absolute limit for a session opened with `persistent: true` ("keep me
    # logged in"). Such a session has no idle timeout, so this is its only
    # bound.
    session_persistent_max_age_seconds: int = 7 * 24 * 60 * 60
    # How old a cookie may get before it is re-issued. The account is re-checked
    # against the permissions service at the same time, so that check happens
    # once per user per interval rather than on every request.
    session_refresh_seconds: int = 5 * 60
    # Off only for local HTTP development; a Secure cookie is dropped over http.
    session_cookie_secure: bool = True

    # Brute-force throttling of failed logins (only failures count; a success
    # clears the counter). Independent of any lockout the permissions backend
    # may do on its own.
    login_max_failures: int = 10
    login_failure_window_seconds: int = 5 * 60

    # Additional origins the CSRF check accepts, comma-separated. Needed when a
    # dev server proxies the API under a different origin than the browser sees.
    # vite on :5180 forwards to the VM on :5001 with `changeOrigin`, so the Host
    # header does not match the Origin. Leave empty in production.
    csrf_extra_origins: str = ""

    _generated_secret: str | None = PrivateAttr(default=None)

    @property
    def extra_origins(self) -> frozenset[str]:
        return frozenset(
            origin.strip()
            for origin in self.csrf_extra_origins.split(",")
            if origin.strip()
        )

    @property
    def effective_cookie_name(self) -> str:
        """
        The cookie name as sent: with the ``__Host-`` prefix when the cookie is
        Secure.

        Browsers reject a ``__Host-`` cookie unless it is Secure, has ``Path=/``
        and has no ``Domain``. This prevents another host under fz-juelich.de
        from setting a cookie that is sent here (cookie tossing, a form of
        session fixation). Since the prefix requires Secure, plain-http
        development uses the bare name.
        """
        if self.session_cookie_secure:
            return f"__Host-{self.session_cookie_name}"
        return self.session_cookie_name

    def resolve_session_secret(self) -> str:
        """
        The signing secret, read from a mounted file or from the environment.

        There is no hardcoded fallback, because a known signing key would let
        anyone forge a session cookie. Without a configured secret the service
        refuses to start, so a deployment that forgets to mount the file fails
        at startup instead of running on a temporary key that invalidates all
        sessions at the next restart. `dev_mode` enables that temporary key.
        """
        if self.session_secret_file:
            return Path(self.session_secret_file).read_text(encoding="utf8").strip()
        if self.session_secret:
            return self.session_secret
        if not self.dev_mode:
            raise RuntimeError(
                "No session secret configured. Set MODAMA_SESSION_SECRET_FILE "
                "to a mounted file (or MODAMA_SESSION_SECRET), or set "
                "MODAMA_DEV_MODE=1 to sign with a random per-boot key."
            )
        if self._generated_secret is None:
            self._generated_secret = secrets.token_urlsafe(48)
            log.warning(
                "MODAMA_DEV_MODE and no session secret configured; generated a "
                "random one. Sessions will not survive a restart of this "
                "service."
            )
        return self._generated_secret


@lru_cache
def get_settings() -> Settings:
    return Settings()
