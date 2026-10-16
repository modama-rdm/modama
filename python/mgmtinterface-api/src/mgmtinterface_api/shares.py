"""
Calls into smbdrop, the service that creates the Samba shares used for ingest.

The share client reports every failure as `RuntimeError` and transport problems
as `requests` exceptions; both become a 502 here, because nothing in this
service can act on them and the caller only needs to know to retry later.
"""

import os

import requests
from smbdrop_client import ShareClient
from smbdrop_client.schema import SerializedShare

from .config import Settings
from .errors import APIError


def make_client(settings: Settings) -> ShareClient:
    # Stateless, so one per call is as cheap as one per app and keeps the
    # settings the single place that knows the address.
    return ShareClient(settings.share_api)


def share_path(settings: Settings, relpath: str, session: str) -> str:
    """
    The path smbdrop exports for a session: `smb_base/<project relpath>/<session>`.

    Normalized, because this is also the key for finding an existing share, and
    smbdrop stores whatever path it was given.
    """
    return os.path.normpath(os.path.join(settings.smb_base, relpath, session))


def list_shares(settings: Settings) -> dict[str, SerializedShare]:
    try:
        return make_client(settings).list()
    except (requests.RequestException, RuntimeError) as exc:
        raise _unavailable() from exc


def find_share(settings: Settings, dest_path: str, uid: int) -> SerializedShare | None:
    """
    The share a user already has for a path, if any.

    A share belongs to the user whose uid it impersonates, so two users can
    each have one for the same session. Matching on the normalized path mirrors
    the legacy `add_share_info`.
    """
    for share in list_shares(settings).values():
        if share["uid"] == uid and os.path.normpath(share["dest_path"]) == dest_path:
            return share
    return None


def create_share(
    settings: Settings, dest_path: str, uid: int, gid: int
) -> SerializedShare:
    try:
        result = make_client(settings).create(dest_path=dest_path, uid=uid, gid=gid)
    except (requests.RequestException, RuntimeError) as exc:
        raise _unavailable() from exc
    return result["share"]


def _unavailable() -> APIError:
    return APIError(
        502,
        "share_service_unavailable",
        "The ingest share service is unavailable. Please try again shortly.",
    )
