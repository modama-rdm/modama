from functools import lru_cache
from pathlib import Path

from pydantic_settings import BaseSettings, SettingsConfigDict


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

    # Signed-cookie session. Not used yet — the auth slice adds the signing; the
    # values live here so the deployment knobs are in one place.
    session_cookie_name: str = "modama_session"
    session_secret: str = ""
    session_secret_file: str = ""
    session_max_age_seconds: int = 8 * 60 * 60
    session_idle_seconds: int = 30 * 60

    def resolve_session_secret(self) -> str:
        """
        The signing secret, from the mounted file when one is configured.

        Raises `RuntimeError` rather than falling back to a default: an
        unsigned or predictably-signed session cookie is a silent auth bypass,
        so failing to start is the safe outcome.
        """
        if self.session_secret_file:
            return Path(self.session_secret_file).read_text(encoding="utf8").strip()
        if self.session_secret:
            return self.session_secret
        raise RuntimeError(
            "No session secret configured: set MODAMA_SESSION_SECRET_FILE "
            "(preferred) or MODAMA_SESSION_SECRET."
        )


@lru_cache
def get_settings() -> Settings:
    return Settings()
