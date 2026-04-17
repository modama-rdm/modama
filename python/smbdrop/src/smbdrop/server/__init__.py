import logging
from typing import Dict

from flask import Flask, request
import jsonschema

from smbdrop_client.schema import (
    CREATE_SHARE_REQUEST_SCHEMA,
    CreateShareRequest,
    CreateShareResponse,
    DeleteShareResponse,
    ListSharesResponse,
)
from smbdrop.share_manager import SambaProcessManager, SambaConfigManager, ShareManager
from smbdrop.mount_manager import MountManager

app = Flask(__name__)

logging.basicConfig(level=logging.INFO)

# FIXME: yuck - in addition to being a side-effect on import, we need to make
# sure we only run this in a very specific deployment (single process - can
# allow for multiple threads though)
pm = SambaProcessManager()
cm = SambaConfigManager(
    config_path="/etc/samba/smb-dynamic.conf",
    procman=pm,
)
mm = MountManager()
sm = ShareManager(config_manager=cm, mount_manager=mm)
sm.update()
pm.start()


@app.route("/share/", methods=["POST"])
def create_share() -> CreateShareResponse:
    request_data: CreateShareRequest = request.json  # type:ignore
    jsonschema.validate(schema=CREATE_SHARE_REQUEST_SCHEMA, instance=request_data)
    share = sm.create_share(
        dest_path=request_data["dest_path"],
        uid=request_data["uid"],
        gid=request_data["gid"],
    )
    return dict(share)


@app.route("/share/<share_id>/", methods=["DELETE"])
def delete_share(share_id: str) -> DeleteShareResponse:
    sm.delete_share(share_id)
    return {"status": "ok"}


@app.route("/share/", methods=["GET"])
def list_shares() -> ListSharesResponse:
    shares = sm.list()
    return shares
