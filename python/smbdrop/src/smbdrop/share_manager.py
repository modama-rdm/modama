import os
import pwd
import sys
import time
import uuid
import json
import signal
import secrets
import logging
from typing import Optional, Dict, Tuple
from typing_extensions import TypedDict
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
    share_name: str
    dest_path: str
    created_timestamp: int

    # temporary credentials:
    username: str
    password: str

    # mapped to this unix user:
    uid: int
    gid: int


class User(TypedDict):
    # temporary credentials:
    username: str
    password: str

    # mapped to this unix user:
    uid: int


Shares = Dict[str, Share]  # id-to-Share mapping
Users = Dict[str, User]  # Unix uid to Samba user mapping


class Config(TypedDict):
    shares: Shares
    users: Users


class SambaProcessManager:
    def __init__(self) -> None:
        self._proc: Optional[subprocess.Popen[bytes]] = None

    def start(self) -> None:
        log.debug("Starting samba service...")
        # -d debug level
        # -F foreground
        # -S log to stdout
        self._proc = subprocess.Popen(
            ["smbd", "-FS", "--no-process-group", "-d3"],
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


# FIXME: adjust create/directory modes?
samba_share_cfg_tpl = """
[%(share_name)s]
path = "%(dest_path)s"
read only = no
force create mode = 0600
force directory mode = 0700
valid users = %(username)s
"""


def config_template(shares: Shares) -> str:
    parts = []

    for share in shares.values():
        parts.append(samba_share_cfg_tpl % share)

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
        shares_config: str = "/etc/smbdrop/shares.json",
        lock_path: str = "/etc/smbdrop/shares.lock",
    ):
        """

        Parameters
        ----------
        config_manager : SambaConfigManager
            [description]
        shares_config : str, optional
            Path to the shares config as JSON, which will be managed by
            this class.
        lock_path : str, optional
            [description], by default "/etc/smbdrop/shares.lock"
        """
        self._share_lock = fasteners.InterProcessLock(lock_path)
        self._shares_config_path = shares_config
        self._config_manager = config_manager

    def _add_share(self, share: Share) -> Config:
        # NOTE: only call this while holding the self._share_lock!
        config = self._read_config()
        config['shares'][share["share_id"]] = share
        self._write_config(config)
        return config

    def _remove_share(self, share_id: str) -> Config:
        # NOTE: only call while holding the self._share_lock!
        config = self._read_config()
        if share_id not in config['shares']:
            raise ValueError(f"share id {share_id} not found!")
        del config['shares'][share_id]
        self._write_config(config)
        return config

    def _write_config(self, config: Config) -> None:
        # NOTE: only call while holding the self._share_lock!
        os.umask(0o077)  # mask out all bits for group and other
        path = os.path.join(
            os.path.dirname(self._shares_config_path),
            ".shares.json~tmp"
        )
        with open(path, "w") as f:
            f.write(json.dumps(config))
        os.rename(src=path, dst=self._shares_config_path)

    def _read_config(self) -> Config:
        # NOTE: when doing a read/modify/update-cycle,
        # only call while holding the self._share_lock!
        if not os.path.exists(self._shares_config_path):
            # we want to create an empty, new share config if we don't already have one
            return {
                'shares': {},
                'users': {},
            }
        with open(self._shares_config_path, "r") as f:
            loaded_config: Config = json.load(f)
            return loaded_config

    def _update_samba(self, shares: Shares) -> None:
        # delegate to SambaConfigManager
        self._config_manager.update_shares(shares)

    def _get_create_user(self, uid: int, share_id: str) -> Tuple[str, str]:
        # NOTE: only call while holding the self._share_lock!
        config = self._read_config()
        if str(uid) in config['users']:
            u = config['users'][str(uid)]
            return u['username'], u['password']
        else:
            password = secrets.token_urlsafe(32)
            username =  f"share{share_id}".replace("-", "")[:32]
            self._add_user(username, uid, password)
            config['users'][str(uid)] = User(
                username=username,
                password=password,
                uid=uid,
            )
            self._write_config(config)
            return username, password
    
    def _add_user(self, username: str, uid: int, password: str):
        subprocess.check_call([
            "/usr/sbin/useradd",
            "--no-create-home",
            "--no-user-group",
            username,
            # allow multiple users with the same UID (multiple ingest for
            # the same user at the same time):
            "--non-unique",
            "--uid", str(uid),
            # "--gid", str(gid),  # FIXME: might need to create groups in the container?!?!
            "--home-dir", '/tmp/',
        ])
        # -s read password from stdin
        # -a add user
        proc = subprocess.Popen(
            ["/usr/bin/smbpasswd", "-s", "-a", username],
            shell=False,
            stdin=subprocess.PIPE
        )
        # smbpasswd wants the password twice:
        proc.communicate(f"{password}\n{password}\n".encode("ascii"))
        proc.wait()
        if proc.returncode != 0:
            raise RuntimeError("failed to set temporary credentials")

    def _remove_user(self, uid: int) -> Config:
        # NOTE: only call while holding the self._share_lock!
        config = self._read_config()
        if str(uid) not in config['users']:
            raise ValueError(f"user id {uid} not found!")
        username = config['users'][str(uid)]['username']
        subprocess.check_call([
            "/usr/bin/smbpasswd",
            "-x",  # delete user
            username,
        ])
        subprocess.check_call([
            "/usr/sbin/userdel",
            username,
        ])
        del config['users'][str(uid)]
        self._write_config(config)
        return config
    
    def _consistency_check(self, config: Config) -> bool:
        share_uids = set(s['uid'] for s in config['shares'].values())
        user_uids = set(u['uid'] for u in config['users'].values())
        if share_uids != user_uids:
            RuntimeError(f"Mismatch between UIDs used in shares {share_uids} and UIDs assigned to users {user_uids}.")

        for u in config['users'].values():
            try:
                pwres = pwd.getpwnam(u['username'])
                if u['uid'] != pwres.pw_uid:
                    raise RuntimeError(
                        f"UID mismatch for {u['username']}: System UID {pwres.pw_uid} vs database UID {u['uid']}."
                    )
            except KeyError:
                raise RuntimeError(f"User {u['username']} not found.")
            try:
                pwres = pwd.getpwuid(u['uid'])
                if u['username'] != pwres.pw_name:
                    raise RuntimeError(
                        f"User name mismatch for {u['uid']}: System {pwres.pw_name} vs database {u['username']}."
                    )
            except KeyError:
                raise RuntimeError(f"User {u['username']} not found.")

        for s in config['shares'].values():
            user_entry = config['users'][str(s['uid'])]
            if user_entry['username'] != s['username']:
                raise RuntimeError(f"User name mismatch for {s['uid']}: User entry {user_entry['username']} vs share {s['username']}.")
        return True

    def create_share(self, dest_path: str, uid: int, gid: int) -> Share:
        """
        Create a new samba share in the give configuration file
        and return its id.
        """
        with self._share_lock:
            share_id = str(uuid.uuid4())
            username, password = self._get_create_user(uid, share_id)
            share: Share = {
                "share_id": share_id,
                "share_name": f"share-{share_id}",
                "dest_path": dest_path,
                "uid": uid,
                "gid": gid,
                "created_timestamp": int(time.time()),
                "username": username,
                "password": password,
            }
            
            config = self._add_share(share)
            assert self._consistency_check(config)
            self._update_samba(config['shares'])
        return share

    def delete_share(self, share_id: str) -> Config:
        with self._share_lock:
            config = self._read_config()
            if share_id not in config['shares']:
                raise ValueError(f"share id {share_id} not found!")
            share: Share = config['shares'][share_id]
            use_count = 0
            # Check if there are other shares for the same user
            for other_share in config['shares'].values():
                if other_share['username'] == share['username']:
                    use_count += 1
            
            self._remove_share(share_id)
            # The share to delete is the last one of that user
            if use_count == 1:
                self._remove_user(share['uid'])
            elif use_count < 1:
                raise RuntimeError('User database inconsistency')
            config = self._read_config()
            assert self._consistency_check(config)
            self._update_samba(config['shares'])
            return config

    def update(self) -> Config:
        with self._share_lock:
            config = self._read_config()
            for user in config['users'].values():
                try:
                    pwd.getpwnam(user['username'])
                except KeyError:
                    self._add_user(
                        username=user['username'],
                        uid=user['uid'],
                        password=user['password'],
                    )
            assert self._consistency_check(config)
            self._update_samba(config['shares'])
            return config

    def list(self) -> Shares:
        config = self._read_config()
        return config['shares']
