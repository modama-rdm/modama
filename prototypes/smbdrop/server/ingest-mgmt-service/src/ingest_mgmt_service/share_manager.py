import os
import uuid
import json
import signal
import logging
from typing import TypedDict

import fasteners

"""
This module manages dynamic creation and deletion of Samba shares.

# Things to figure out
We need to serialize creation/deletion requests to make sure we don't mess
up the config. For example, if we have an existing share S1 and two requests
in flight, one to create S2 and one to delete S1, there could be different
outcomes, depending on how the read/modify/update parts are "scheduled".

("update" here is a reload of the samba service)

## Scenario 1:
create S2: read
create S2: modify
delete S1: read
delete S1: modify
create S2: update
delete S1: update
=> S2 is not properly created

## Scenario 2:
create S2: read
create S2: modify
delete S1: read
delete S1: modify
delete S1: update
create S2: update
=> S1 is not properly deleted

In other words: the read/modify/update cycle needs to be a critical section.

We also need some persistence, to make sure we have a reliable source of truth
for the currently existing shares, which survives restarts of this service.
"""


log = logging.getLogger(__name__)


class Share(TypedDict):
    share_id: str
    dest_path: str


Shares = Dict[str, Share]  # id-to-Share mapping


class SambaProcessManager:
    def __init__(self):
        self._proc = None

    def start(self):
        log.debug("Starting samba service...")
        proc = subprocess.Popen(...)

        log.debug("Samba running in pid %d", self._proc)

    def stop(self):
        if self.self._proc is None:
            raise RuntimeError(f"cannot stop samba service: pid is None")
        os.kill(self._pid, signal.SIGTERM)

    def reload(self):
        if self.self._proc is None:
            raise RuntimeError(f"cannot reload samba service: pid is None")
        os.kill(self._pid, signal.SIGHUP)


class SambaConfigManager:
    def __init__(self, template_path: str):
        pass

    def update_shares(self, shares: Shares) -> None:
        # update samba config
        # 
        # reload samba service
        # -> kill -HUP `pid`
        pass


class ShareManager:
    def __init__(
        self,
        samba_config_manager: SambaConfigManager,
        config="/tmp/shares.json",
        lock_path="/tmp/shares.lock",
    ):
        # FIXME: better default value for config/lock path!
        self._share_lock = fasteners.InterProcessLock(lock_path)
        self._persistent_config_path = config
        self._config_manager = samba_config_manager

    def _add_share(self, share: Share):
        # NOTE: only call this while holding the self._share_lock!
        shares = self._read_shares()
        shares[share["share_id"]] = share
        self._write_shares(shares)

    def _remove_share(self, share_id: str) -> Shares:
        # NOTE: only call while holding the self._share_lock!
        shares = self._read_shares()
        if share_id not in shares:
            raise ValueError(f"share id {share_id} not found!")
        del shares[share_id]
        self._write_shares(shares)
        return shares

    def _write_shares(self, shares: Shares) -> None:
        # NOTE: only call while holding the self._share_lock!
        shares = list(shares)
        path = os.path.join(
            os.path.dirname(self._persistent_config_path),
            ".meh.json~tmp"
        )
        with open(path, "w") as f:
            f.write(json.dumps(shares))
        os.rename(src=path, dst=self._persistent_config_path)

    def _read_shares(self) -> Shares:
        # NOTE: when doing a read/modify/update-cycle, only call while holding the self._share_lock!
        with open(self._persistent_config_path, "r") as f:
            return json.load(f)

    def _update_samba(self, shares: Shares):
        # delegate to SambaConfigManager
        self._config_manager.update_shares(shares)

    def create_share(self, dest_path: str) -> Share:
        """
        Create a new samba share in the give configuration file
        and return its id.
        """
        share_id = str(uuid.uuid4())
        share: Share = {
            "share_id": share_id,
            "dest_path": dest_path,
            # TODO: add timestamp
        }
        with self._share_lock:
            shares = self._add_share(share)
            self._update_samba(shares)
        return share

    def delete_share(self, share_id: str) -> None:
        with self._share_lock:
            shares = self._remove_share(share_id)
            self._update_samba(shares)
