from datetime import datetime

from fastapi import APIRouter
from pydantic import BaseModel

from ..versioning import DEPRECATIONS, LATEST_VERSION, SUPPORTED_VERSIONS

router = APIRouter(prefix="/versions", tags=["versions"])


class DeprecatedVersion(BaseModel):
    version: int
    deprecated_since: datetime
    #: After this time the version is no longer served and requests get a 400.
    sunset: datetime


class VersionsResponse(BaseModel):
    supported: list[int]
    latest: int
    deprecated: list[DeprecatedVersion]


@router.get("")
def read_versions() -> VersionsResponse:
    """
    The contract versions this server supports.

    Unauthenticated and unversioned on purpose: a locally installed client
    calls this at startup, before it has a session or knows which version to
    use. That way it can report a problem in its own log, where the person
    running the instrument sees it, instead of failing in the middle of an
    ingest later.
    """
    return VersionsResponse(
        supported=list(SUPPORTED_VERSIONS),
        latest=LATEST_VERSION,
        deprecated=[
            DeprecatedVersion(
                version=version,
                deprecated_since=deprecation.since,
                sunset=deprecation.sunset,
            )
            for version, deprecation in sorted(DEPRECATIONS.items())
        ],
    )
