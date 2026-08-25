from datetime import datetime

from fastapi import APIRouter
from pydantic import BaseModel

from ..versioning import DEPRECATIONS, LATEST_VERSION, SUPPORTED_VERSIONS

router = APIRouter(prefix="/versions", tags=["versions"])


class DeprecatedVersion(BaseModel):
    version: int
    deprecated_since: datetime
    #: After this moment the version is gone and requests for it get a 400.
    sunset: datetime


class VersionsResponse(BaseModel):
    supported: list[int]
    latest: int
    deprecated: list[DeprecatedVersion]


@router.get("")
def read_versions() -> VersionsResponse:
    """
    Which contract versions this server speaks.

    Unauthenticated and itself unversioned, on purpose: a locally installed
    client calls this at startup, before it knows either. That lets it complain
    in its own log — where the person running the instrument will see it —
    rather than failing halfway through an ingest months later.
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
