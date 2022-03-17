from typing import List, Optional
import subprocess
from smbdrop_gui.mount import Mount, MountManager


"""
Prototype-ish GIO/gvfs-based SMB mounter for Linux. Calls out to the command
line `gio` tool to mount samba shares. Could be rewritten using Python bindings,
but they are (AFAIK) not pip-installable.
"""


class GIOMountManager(MountManager):
    def _get_uri(self, share_name: str, username: str) -> str:
        return f"smb://{username}@{self._smb_host}/{share_name}"

    def mount_smb(self, share_name: str, username: str, password: str) -> Mount:
        """
        Call out to `gio` to mount smb share. I/O looks like this:

            $ gio mount smb://sharec79ef3b6d55047b196e9aa3b3fe@localhost/share-c79ef3b6-d550-47b1-96e9-aa3b3feca861
            Authentication Required
            Enter password for share “share-c79ef3b6-d550-47b1-96e9-aa3b3feca861” on “localhost”:
            Domain [WORKGROUP]: ER-C-INGEST
            Password:
        """
        uri = self._get_uri(share_name, username)
        proc = subprocess.Popen(["gio", "mount", uri], stdin=subprocess.PIPE)
        input_text = f"{self._workgroup}\n{password}\n".encode("utf-8")
        try:
            outs, errs = proc.communicate(input=input_text, timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
        ret = proc.wait()
        assert ret == 0, "mount should succeed"
        return Mount(dest=uri, share=share_name)

    def umount(self, share_name: str, username: str) -> None:
        uri = self._get_uri(share_name, username)
        subprocess.run(["gio", "mount", "-u", uri], capture_output=True, check=True)

    def open(self, share_name: str, username: str) -> None:
        uri = self._get_uri(share_name, username)
        subprocess.run(["xdg-open", uri], capture_output=True, check=True)

    def list_mounts(self) -> List[Mount]:
        gio_mount_ls = subprocess.run(
            ["gio", "mount", "-li"],
            capture_output=True,
            check=True,
        )
        mounts = []
        for line in gio_mount_ls.stdout.decode("utf-8").splitlines():
            if line.startswith(" "):
                continue
            m = mount_from_line(line)
            if m is None:
                continue
            # filter out our ingest shares:
            if m.dest.startswith("smb://") and m.share.startswith("share-"):
                mounts.append(m)
        return mounts


def mount_from_line(line: str) -> Optional[Mount]:
    if not line.startswith("Mount"):
        return None  # not an SMB mount
    id_, rest = line.split(":", 1)  # drop Mount(X) prefix
    parts = rest.strip().split(" ")
    if len(parts) != 5:
        return None
    share_name, _, hostname, _, uri = parts
    return Mount(share=share_name, dest=uri)


if __name__ == "__main__":
    # sample output of `gio mount -li`:
    test_input = \
"""
Drive(0): Samsung SSD 860 EVO 1TB
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  ids:
   unix-device: '/dev/sda'
  themed icons:  [drive-harddisk-solidstate]  [drive-harddisk]  [drive]  [drive-harddisk-solidstate-symbolic]  [drive-harddisk-symbolic]  [drive-symbolic]
  symbolic themed icons:  [drive-harddisk-solidstate-symbolic]  [drive-harddisk-symbolic]  [drive-symbolic]  [drive-harddisk-solidstate]  [drive-harddisk]  [drive]
  is_removable=0
  is_media_removable=0
  has_media=1
  is_media_check_automatic=1
  can_poll_for_media=0
  can_eject=0
  can_start=0
  can_stop=0
  start_stop_type=shutdown
  sort_key=00coldplug/00fixed/sd____a
Drive(1): LITEONIT LCS-256M6S
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  ids:
   unix-device: '/dev/sdb'
  themed icons:  [drive-harddisk-solidstate]  [drive-harddisk]  [drive]  [drive-harddisk-solidstate-symbolic]  [drive-harddisk-symbolic]  [drive-symbolic]
  symbolic themed icons:  [drive-harddisk-solidstate-symbolic]  [drive-harddisk-symbolic]  [drive-symbolic]  [drive-harddisk-solidstate]  [drive-harddisk]  [drive]
  is_removable=0
  is_media_removable=0
  has_media=1
  is_media_check_automatic=1
  can_poll_for_media=0
  can_eject=0
  can_start=0
  can_stop=0
  start_stop_type=shutdown
  sort_key=00coldplug/00fixed/sd____b
Drive(2): MATSHITA DVD-RAM SW830
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  ids:
   unix-device: '/dev/sr0'
  themed icons:  [drive-optical]  [drive]  [drive-optical-symbolic]  [drive-symbolic]
  symbolic themed icons:  [drive-optical-symbolic]  [drive-symbolic]  [drive-optical]  [drive]
  is_removable=1
  is_media_removable=1
  has_media=0
  is_media_check_automatic=1
  can_poll_for_media=0
  can_eject=1
  can_start=0
  can_stop=0
  start_stop_type=shutdown
  sort_key=00coldplug/11removable/sr0
Mount(0): share-c79ef3b6-d550-47b1-96e9-aa3b3feca861 on localhost -> smb://sharec79ef3b6d55047b196e9aa3b3fe@localhost/share-c79ef3b6-d550-47b1-96e9-aa3b3feca861/
  Type: GDaemonMount
  default_location=smb://sharec79ef3b6d55047b196e9aa3b3fe@localhost/share-c79ef3b6-d550-47b1-96e9-aa3b3feca861/
  themed icons:  [folder-remote]  [folder]  [folder-remote-symbolic]  [folder-symbolic]
  symbolic themed icons:  [folder-remote-symbolic]  [folder-symbolic]  [folder-remote]  [folder]
  can_unmount=1
  can_eject=0
  is_shadowed=0
"""
    mm = GIOMountManager("localhost", "ER-C-INGEST")
    mounts = mm.list_mounts()
    print(mounts)
