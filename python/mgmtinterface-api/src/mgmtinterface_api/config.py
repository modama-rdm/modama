import logging
import secrets
from functools import lru_cache
from pathlib import Path

from pydantic import PrivateAttr
from pydantic_settings import BaseSettings, SettingsConfigDict

log = logging.getLogger(__name__)

# Every route of this service lives under this prefix; nginx routes it here and
# forwards the full path. Deliberately carries no version number: the contract
# version travels in the `X-Modama-API-Version` header instead (see
# `versioning.py`), so this path never has to change again. It lives here rather
# than in the package root because both the app factory and the versioning layer
# need it, and importing the root from a submodule would be a cycle.
API_PREFIX = "/mgmt-api"


class Settings(BaseSettings):
    """
    Runtime configuration, read from the environment with the ``MODAMA_``
    prefix.

    The Flask app loads a Python file named by ``MGMTINTERFACE_CONFIG``; this
    service uses environment variables so the container needs no bind-mounted
    config. Secrets are the exception — the deployment mounts those as files, so
    `session_secret_file` names a file to read instead of putting the value in
    the environment.
    """

    model_config = SettingsConfigDict(env_prefix="MODAMA_")

    permissions_api: str = "http://permissions:5050"

    # Signed-cookie session. The name must differ from the Flask app's
    # `mgmtinterface_session` — both are live on the same origin until the
    # cutover (#170).
    session_cookie_name: str = "modama_session"
    session_secret: str = ""
    session_secret_file: str = ""
    # Two lifetimes: `max_age` caps the session no matter how active the user is,
    # `idle` expires it after a quiet stretch. Sliding refresh moves the idle
    # window forward, never the absolute one.
    session_max_age_seconds: int = 8 * 60 * 60
    session_idle_seconds: int = 30 * 60
    # How stale a cookie may get before it is re-issued. This also paces the
    # re-validation against the permissions service, so that check costs one
    # request per user per interval rather than one per request.
    session_refresh_seconds: int = 5 * 60
    # Off only for local HTTP development; a Secure cookie is dropped over http.
    session_cookie_secure: bool = True

    # Brute-force throttling of failed logins (only failures count; a success
    # clears the counter). Independent of any lockout the permissions backend
    # may do on its own.
    login_max_failures: int = 10
    login_failure_window_seconds: int = 5 * 60

    # Additional origins the CSRF check accepts, comma-separated. Needed when a
    # dev server proxies the API under a different origin than the browser sees
    # — vite on :5180 forwards to the VM on :5001 with `changeOrigin`, so the
    # Host header no longer matches the Origin. Leave empty in production.
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
        The cookie name as sent, with the ``__Host-`` prefix when we are able to
        satisfy it.

        The prefix makes the browser reject the cookie unless it is Secure, has
        ``Path=/`` and carries no ``Domain`` — which is what stops another host
        under fz-juelich.de from setting a cookie that gets sent here (a
        "cookie tossing" session-fixation attack). The prefix requires Secure,
        so plain-http development falls back to the bare name.
        """
        if self.session_cookie_secure:
            return f"__Host-{self.session_cookie_name}"
        return self.session_cookie_name

    def resolve_session_secret(self) -> str:
        """
        The signing secret: a mounted file, an environment value, or — failing
        both — a fresh random one for this process.

        There is deliberately no hardcoded fallback: a *predictable* signing key
        would let anyone mint a session cookie. A random per-boot key has no
        such problem; its only cost is that sessions do not survive a restart of
        this container, since the key they were signed with is gone. Configure
        `session_secret_file` when that matters.
        """
        if self.session_secret_file:
            return Path(self.session_secret_file).read_text(encoding="utf8").strip()
        if self.session_secret:
            return self.session_secret
        if self._generated_secret is None:
            self._generated_secret = secrets.token_urlsafe(48)
            log.warning(
                "No session secret configured (MODAMA_SESSION_SECRET_FILE or "
                "MODAMA_SESSION_SECRET); generated a random one. Sessions will "
                "not survive a restart of this service."
            )
        return self._generated_secret


@lru_cache
def get_settings() -> Settings:
    return Settings()
