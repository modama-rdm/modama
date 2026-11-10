"""
Checks the `X-Agent-Token` modama-agent sends when it opens an ingest share,
against smbdrop's token service.

An agent gets the token from that service with its client certificate, which
names its resource, and the token lives an hour.
It says where a request comes from, not who sends it; the session says who.
With both, a share is opened as the user and bound to the resource, which is
what makes smbdrop offer it to that resource's agents. The legacy endpoint
does the same through `token_client.py`.
"""

import logging
from urllib.parse import urlsplit

import requests

from .config import Settings
from .errors import APIError

log = logging.getLogger(__name__)

# A verify is a signature check and answers at once; the timeout is for a hung
# service, not a slow one.
TIMEOUT_SECONDS = 10

# The agent id in a token is `x-modama-agent://<resource>#<agent>`, as
# modama-common's `AgentId::from_uri` reads it.
AGENT_ID_SCHEME = "x-modama-agent"


def verified_resource(settings: Settings, token: str) -> str:
    """
    The resource a token was issued for.

    A token the service does not accept (not signed by it, or expired) is a
    403, not a 401: the session is fine, and a 401 would send the agent to log
    in again when what it needs is a fresh token.
    """
    try:
        response = requests.put(
            f"{settings.token_api}/verify/",
            data=token.encode("utf8"),
            verify=settings.token_ca_cert,
            timeout=TIMEOUT_SECONDS,
        )
        response.raise_for_status()
        answer = response.json()
    except (requests.RequestException, ValueError) as exc:
        # Logged, because the 502 alone does not say whether the service is
        # down or its certificate was rejected. Python 3.13 rejects one without
        # an Authority Key Identifier, as smbdrop's on the test VM is.
        log.warning("Agent token check with %s failed: %s", settings.token_api, exc)
        raise _unavailable() from exc

    # The service answers 200 either way: `"Invalid"`, or
    # `{"VerifiedAgent": {"agent_id": ...}}`. Anything else counts as invalid,
    # as in the legacy client.
    verified = answer.get("VerifiedAgent") if isinstance(answer, dict) else None
    agent_id = verified.get("agent_id") if isinstance(verified, dict) else None
    if not isinstance(agent_id, str):
        raise _invalid()
    parts = urlsplit(agent_id)
    if parts.scheme != AGENT_ID_SCHEME or not parts.netloc:
        raise _invalid()
    return parts.netloc


def _invalid() -> APIError:
    return APIError(
        403,
        "invalid_agent_token",
        "The agent token is invalid or has expired. Fetch a new one and retry.",
    )


def _unavailable() -> APIError:
    return APIError(
        502,
        "token_service_unavailable",
        "The agent token service is unavailable. Please try again shortly.",
    )
