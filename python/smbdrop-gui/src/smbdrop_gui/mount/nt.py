from typing import List
import subprocess
from smbdrop_gui.mount import Mount, MountManager

import win32net


def get_drives() -> List[Mount]:
    # adapted from http://timgolden.me.uk/python/win32_how_do_i/show_mapped_drives.html
    result = []
    resume = 0
    while 1:
        (drives, total, resume) = win32net.NetUseEnum(None, 0, resume)
        for drive in drives:
            if drive["local"]:
                _, _, server, share = drive["remote"].split("\\")
                result.append(Mount(dest=drive, share=share))
                # result.append((drive['local'], server, share))
        if not resume:
            break
    return result


class NTMountManager(MountManager):
    def mount_smb(self, share_name: str, username: str, password: str) -> Mount:
        # FIXME: selection of share drive?
        # print(f"net use z: \\\\192.168.178.27\\{share['share_name']} {share['password']} /USER:ER-C-INGEST\\{share['username']}")
        subprocess.run(
            [
                "net",
                "use",
                "/persistent:no",
                "z:",
                f"\\\\{self._smb_host}\\{share_name}",
                password,
                f"/USER:{self._workgroup}\\{username}",
            ],
            capture_output=False,
            check=True,
        )
        return Mount(dest="z:", share=share_name)

    def umount(self, share_name: str, username: str) -> None:
        subprocess.run(
            ["net", "use", "z:", "/delete", "/yes"], capture_output=False, check=True
        )

    def open(self, share_name: str, username: str) -> None:
        subprocess.run(["explorer", "z:"])

    def list_mounts(self) -> List[Mount]:
        all_mounts = get_drives()
        return [m for m in all_mounts if m.share.startswith("")]
