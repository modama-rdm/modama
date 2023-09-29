import os
from typing import Set
import logging
import subprocess

log = logging.getLogger(__name__)


def unescape_octal(line: str):
    r"""
    Unescape octal escape sequences like \040 into their original characters
    (not terribly optimized, but isn't called often)

    Ignores incomplete sequences at the end of the line.
    """
    char_iter = iter(line)
    result = []
    while True:
        try:
            char = next(char_iter)
            if char == '\\':
                # start of an octal sequence, read the next three characters:
                num_as_str = next(char_iter) + next(char_iter) + next(char_iter)
                result.append(chr(int(num_as_str, 8)))
            else:
                result.append(char)
        except StopIteration:
            return "".join(result)


class BindMount:
    def __init__(self, src: str, dest: str):
        self.src = src
        self.dest = dest

    def __repr__(self):
        return "<BindMount %s -> %s>" % (self.src, self.dest)


class MountManager:
    """
    This class takes care of bind mounting the shared directories. It's given
    a set of mounts that should exist, and adds/removes as needed to reach the
    desired state.

    Mountpoints will also be dynamically created/removed by the manager.
    """

    def __init__(
        self,
        bind_mount_prefix: str = '/run/smbdrop/shares/',
    ):
        self._bind_mount_prefix = bind_mount_prefix
        if not os.path.exists(bind_mount_prefix):
            os.makedirs(bind_mount_prefix)

    def set_state(self, state: Set[BindMount]):
        current_mounts = self.get_current_mounts()

        # which of the current mounts aren't in the desirable state?
        to_umount = current_mounts - state

        # which of the desirable mounts aren't currently there?
        to_mount = state - current_mounts

        log.debug(
            "current_mounts=%d, to_umount=%s, to_mount=%s",
            current_mounts, to_umount, to_mount
        )

        # first umount, then mount:
        for m in to_umount:
            self.umount(m)

        for m in to_mount:
            self.mount(m)

    def umount(self, m: BindMount):
        try:
            subprocess.check_call(["umount", m.dest])
            os.rmdir(m.dest)
        except subprocess.CalledProcessError:
            log.error(f"can't unmount {m.dest} - will try again on next `set_state`")

    def mount(self, m: BindMount):
        # 1) create mountpoint, if it doesn't exist:
        if not os.path.exists(m.dest):
            os.mkdir(m.dest)
        # 2) create bind mount
        subprocess.check_call(["mount", "-obind", m.src, m.dest])

    def get_current_mounts(self) -> Set[BindMount]:
        """
        Returns the set of currently active bind mounts which are managed under
        the bind mount prefix.
        """
        with open("/proc/self/mountinfo") as f:
            lines = f.readlines()

        # /proc/<pid>/mountinfo lines look like this:
        # 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
        #
        # for bind mounts, fields 4 and 5 are the "source" and "destination" of
        # the mount.

        # spaces, tabs, newlines, backslashes, and hash signs in the mount
        # point are octal-escaped, so we need to un-escape them here.

        result = set()

        for line in lines:
            # limit here, as we don't need to parse most of the fields
            parts = line.split(' ', 5)
            root = unescape_octal(parts[3])
            mount_point = unescape_octal(parts[4])
            if mount_point.startswith(self._bind_mount_prefix):
                result.add(BindMount(root, mount_point))

        return result


if __name__ == "__main__":
    # poor man's test suite here:
    assert unescape_octal("asdf") == "asdf"
    assert unescape_octal(r"as\040df") == "as df"
    assert unescape_octal(r"as\043df") == "as#df"

    print(MountManager(bind_mount_prefix='/mnt/smbdrop/').get_current_mounts())
