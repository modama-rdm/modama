from typing import List
import subprocess
from . import Mount, MountManager


"""
Prototype-ish GIO/gvfs-based SMB mounter for Linux. Calls out to the command
line `gio` tool to mount samba shares. Could be rewritten using Python bindings,
but they are (AFAIK) not pip-installable.
"""


class GIOMountManager(MountManager):
    def _get_uri(self, share_name: str, username: str) -> str:
        return f"smb://{username}@{self._smb_host}/{share_name}"

    def mount_smb(self, share_name: str, username: str, password: str) -> str:
        """
        Call out to `gio` to mount smb share. I/O looks like this:

            $ gio mount smb://sharec79ef3b6d55047b196e9aa3b3fe@localhost/share-c79ef3b6-d550-47b1-96e9-aa3b3feca861
            Authentication Required
            Enter password for share “share-c79ef3b6-d550-47b1-96e9-aa3b3feca861” on “localhost”:
            Domain [WORKGROUP]: ER-C-INGEST
            Password:

        Parameters
        ----------
        share_name : str
            _description_
        username : str
            _description_
        password : str
            _description_

        Returns
        -------
        str
            _description_
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

    def umount(self, share_name: str, username: str) -> None:
        uri = self._get_uri(share_name, username)
        subprocess.run(["gio", "mount", "-s", uri], capture_output=True, check=True)

    def open(self, share_name: str, username: str) -> None:
        uri = self._get_uri(share_name, username)
        subprocess.run(["xdg-open", uri], capture_output=True, check=True)

    def list_mounts(self) -> List[Mount]:
        # TODO
        return super().list_mounts()
