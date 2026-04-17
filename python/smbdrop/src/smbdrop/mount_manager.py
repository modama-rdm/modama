import os
from typing import Set, List
import logging
import subprocess

from modama_common.fs import set_immutable

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
            if char == "\\":
                # start of an octal sequence, read the next three characters:
                num_as_str = next(char_iter) + next(char_iter) + next(char_iter)
                result.append(chr(int(num_as_str, 8)))
            else:
                result.append(char)
        except StopIteration:
            return "".join(result)


class MountError(Exception):
    pass


class MountStateError(MountError):
    def __init__(self, msg, failed_mounts: List["BindMount"]) -> None:
        super().__init__(msg)
        self.failed_mounts = failed_mounts


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
        bind_mount_prefix: str = "/run/smbdrop/shares/",
    ):
        self._bind_mount_prefix = bind_mount_prefix
        if not os.path.exists(bind_mount_prefix):
            os.makedirs(bind_mount_prefix)

    def set_state(self, state: Set[BindMount]):
        """
        Ensure the bind mounts that should exist are created, and those that
        exist but shouldn't are removed.

        Raises `MountStateError` if one or more mounts fail
        """
        current_mounts = self.get_current_mounts()

        # which of the current mounts aren't in the desirable state?
        to_umount = current_mounts - state

        # which of the desirable mounts aren't currently there?
        to_mount = state - current_mounts

        log.debug(
            "current_mounts=%d, to_umount=%s, to_mount=%s",
            current_mounts,
            to_umount,
            to_mount,
        )

        # first umount, then mount:
        for m in to_umount:
            # will ignore errors, for example if the directory is in use:
            self.umount(m)

        failed_mounts = []
        for m in to_mount:
            try:
                self.mount(m)
            except MountError:
                log.exception("failed to execute bind mount %s, continuing", m)
                failed_mounts.append(m)
                continue
        if len(failed_mounts) > 0:
            failed_str = ",".join([repr(m) for m in failed_mounts])
            raise MountStateError(
                f"one or more mounts failed to execute: {failed_str}",
                failed_mounts=failed_mounts,
            )

    def umount(self, m: BindMount):
        try:
            subprocess.check_call(["umount", m.dest])
            os.rmdir(m.dest)
        except subprocess.CalledProcessError:
            log.error(f"can't unmount {m.dest} - will try again on next `set_state`")

    def mount(self, m: BindMount):
        # 0) Pre-flight checks: make sure the source is a directory, and it
        #    stays this way. We do this here by setting the immutable flag, and
        #    keeping it until after the mount finished successfully:
        src_path = os.path.normpath(m.src)
        if src_path.endswith("/"):
            # if we do `chattr +i /some/symlink/`,  we actually _follow_ the symlink,
            # and set the link target immutable. Can't have that...
            raise MountError("Trailing slashes should be stripped out")
        set_immutable(src_path, True)
        try:
            if os.path.islink(src_path) or not os.path.isdir(src_path):
                raise MountError(
                    f"{src_path} is a symlink or not a directory, aborting"
                )
            # 1) Create mountpoint, if it doesn't exist:
            if not os.path.exists(m.dest):
                os.mkdir(m.dest)
            # 2) Create bind mount
            subprocess.check_call(["mount", "-obind", src_path, m.dest])
        except subprocess.CalledProcessError as e:
            raise MountError(f"Failed to mount {m.src} as {m.dest}: {e}")
        finally:
            set_immutable(src_path, False)

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
            parts = line.split(" ", 5)
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

    print(MountManager(bind_mount_prefix="/mnt/smbdrop/").get_current_mounts())
