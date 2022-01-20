import os
import sys
import uuid
import json
import signal
import logging
from typing import Optional, TypedDict, Dict
import subprocess

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
    def __init__(self) -> None:
        self._proc: Optional[subprocess.Popen[bytes]] = None

    def start(self) -> None:
        log.debug("Starting samba service...")
        # -F: foreground
        self._proc = subprocess.Popen(
            ["strace", "-f", "-o", "/var/log/strace-smb.log", "-s", "1024", "samba", "-i", "-d", "3"],
            stdin=None,
            stdout=sys.stdout,
            stderr=sys.stderr
        )

        log.debug("Samba running in pid %d", self._proc)

    def stop(self) -> None:
        if self._proc is None:
            raise RuntimeError("cannot stop samba service: not running")
        self._proc.send_signal(signal.SIGTERM)
        self._proc.wait()  # FIXME: handle non-zero return

    def reload(self) -> None:
        if self._proc is None:
            raise RuntimeError("cannot reload samba service: not running")
        self._proc.send_signal(signal.SIGHUP)

    def is_running(self) -> bool:
        if self._proc is None:
            return False  # did not start yet
        if self._proc.poll() is None:
            return True  # we don't have a return value -> still running
        return False

    @property
    def pid(self) -> int:
        if not self.is_running() or self._proc is None:
            raise RuntimeError("cannot get pid: not running")
        return self._proc.pid


def config_template(shares: Shares) -> str:
    parts = []
    for k, v in shares.items():
        parts.append("")

    return "\n\n".join(parts)


class SambaConfigManager:
    def __init__(self, config_path: str, procman: SambaProcessManager):
        self._procman = procman
        self._config_path = config_path

    def update_shares(self, shares: Shares) -> None:
        new_config = config_template(shares)
        with open(self._config_path, "w") as f:
            f.write(new_config)
        if self._procman.is_running():
            self._procman.reload()


class ShareManager:
    def __init__(
        self,
        config_manager: SambaConfigManager,
        shares_config: str = "/tmp/shares.json",
        lock_path: str = "/tmp/shares.lock",
    ):
        # FIXME: better default value for config/lock path!
        self._share_lock = fasteners.InterProcessLock(lock_path)
        self._shares_config_path = shares_config
        self._config_manager = config_manager

    def _add_share(self, share: Share) -> Shares:
        # NOTE: only call this while holding the self._share_lock!
        shares = self._read_shares()
        shares[share["share_id"]] = share
        self._write_shares(shares)
        return shares

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
        path = os.path.join(
            os.path.dirname(self._shares_config_path),
            ".meh.json~tmp"
        )
        with open(path, "w") as f:
            f.write(json.dumps(shares))
        os.rename(src=path, dst=self._shares_config_path)

    def _read_shares(self) -> Shares:
        # NOTE: when doing a read/modify/update-cycle,
        # only call while holding the self._share_lock!
        if not os.path.exists(self._shares_config_path):
            # we want to create an empty, new share config if we don't already have one
            return {}
        with open(self._shares_config_path, "r") as f:
            loaded_shares: Shares = json.load(f)
            return loaded_shares

    def _update_samba(self, shares: Shares) -> None:
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

    def delete_share(self, share_id: str) -> Shares:
        with self._share_lock:
            shares = self._remove_share(share_id)
            self._update_samba(shares)
            return shares

    def update(self) -> Shares:
        with self._share_lock:
            shares = self._read_shares()
            self._update_samba(shares)
            return shares
