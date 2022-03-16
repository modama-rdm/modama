import sys
from typing import List, NamedTuple, Type


class Mount(NamedTuple):
    share: str
    dest: str


class MountManager:
    def __init__(self, smb_host: str, workgroup: str):
        self._smb_host = smb_host
        self._workgroup = workgroup

    def mount_smb(self, share_name: str, username: str, password: str) -> str:
        raise NotImplementedError()

    def list_mounts(self) -> List[Mount]:
        raise NotImplementedError()

    def umount(self, share_name: str, username: str) -> None:
        raise NotImplementedError()

    def open(self, share_name: str, username: str) -> None:
        raise NotImplementedError()


def get_mm_class() -> Type[MountManager]:
    if sys.platform.startswith("nt"):
        from smbdrop.client.gui.mount.nt import NTMountManager as MountManagerC
    elif sys.platform.startswith("linux"):
        from smbdrop.client.gui.mount.gio import GIOMountManager as MountManagerC
    return MountManagerC
