import grp
import os
import pwd
import re
import shutil
import stat
import subprocess
from collections.abc import Collection
from typing import NamedTuple, final, TypedDict

import pamela
import posix1e  # pylibacl, needs libacl1-dev, https://pylibacl.k1024.org/
from modama_common.fs import is_immutable, set_immutable
from permissions_client.schema import ADHOC, SerializedProject

BASE_PATH = "/data/"

ADHOC_PATH = os.path.join(BASE_PATH, ADHOC)
SHARING_PATH = os.path.join(BASE_PATH, "sharing")

ADHOC_GROUP = "testerc"

with open("/etc/group", "r") as groupfile:
    system_groups = set(line.split(":")[0] for line in groupfile)


class ValidationError(ValueError):
    """
    Something ist wrong with the user input or files related to the request.

    Details provided in error message.
    """

    pass


class ACLError(IOError):
    """
    Checking an ACL failed for some reason

    See https://pylibacl.k1024.org/module.html#posix1e.ACL.check
    """

    pass


FS_ERRORS = (ACLError, PermissionError, FileNotFoundError, OSError)


class AccumulatedMemberError(RuntimeError):
    """
    Accumulated errors belonging to classes in FS_ERRORS from
    traversing a project's directory tree when adding or removing members.

    The code will not bail out, but just collect all errors. Since we run as root,
    errors SHOULD not happen, but for defense in depth and keeping a consistent state
    we try to change members as much as possible instead of bailing out early.

    With this exception we can raise a collection with all errors that occurred.
    """

    pass


# From https://unix.stackexchange.com/questions/11477/what-are-the-allowed-group-names-for-groupadd
# See https://github.com/shadow-maint/shadow/blob/master/libmisc/chkname.c
# Go with most conservative approach
def check_valid_group(groupname: str):
    """
    Check if :code:`groupname` can become a new Unix group

    The concept for projects includes "proper" projects that have their own Unix
    group. To keep an overview over many active projects, it will be beneficial
    to have identical folder and group names.

    Furthermore, ad-hoc projects can be created by users on-demand and control
    access via per-user ACLs. That allows creating projects immediately, without
    having to wait for the IFF admins to create the group.

    For large projects with many members it will be easier to manage access via
    group membership. Likely, many "proper" projects will start their life as
    ad-hoc projects and will be migrated to a group. Therefore we should already
    make sure that a new group with the project name can be created later.

    Criteria:
    * Does not collide with system groups. Probably we should curate
    that as a list later?
    * Length and characters compatible with most Unix systems
    * Not too short to have some kind of meaning
    """
    if groupname in system_groups:
        raise ValidationError("Is a system group.")
    if len(groupname) > 32:
        raise ValidationError(
            f"Too long: Maximum length is 32, actual length is {len(groupname)}."
        )
    if len(groupname) < 3:
        raise ValidationError(
            f"Too short: Minimum length is 3, actual length is {len(groupname)}."
        )
    if not re.fullmatch("[a-z]", groupname[0]):
        raise ValidationError(f"First character must be in a-z, is {groupname[0]}.")
    if not re.fullmatch("[-a-z_0-9]*", groupname):
        raise ValidationError("Can only use characters a-z, 0-9, '_' and '-'.")


def check_valid_dataset(name: str):
    # Restriction on some file systems, avoid "funny business" from user input
    if len(name) > 255:
        raise ValidationError(
            f"Too long: Maximum length is 255, actual length is {len(name)}."
        )
    if len(name) < 1:
        raise ValidationError(
            f"Too short: Minimum length is 1, actual length is {len(name)}."
        )
    # Restrictive positive list to avoid "funny business" from user input
    if not re.fullmatch("[-a-zA-Z0-9_. ]*", name):
        raise ValidationError(
            "Dataset name can only contain characters a-z, A-Z, 0-9, '_', '-', ' ' and '.'."
        )


def check_project_name_taken(name: str):
    if os.path.exists(os.path.join(BASE_PATH, name)):
        raise ValidationError("File or folder exists in base folder.")
    if os.path.exists(os.path.join(ADHOC_PATH, name)):
        raise ValidationError("File or folder exists in adhoc folder.")
    if os.path.exists(os.path.join(SHARING_PATH, name)):
        raise ValidationError("File or exists in sharing folder.")


def is_group(name: str):
    try:
        _ = grp.getgrnam(name)
        return True
    except KeyError:
        return False


def authenticate(username: str, password: str) -> pamela.PamHandle | None:
    return pamela.authenticate(username, password)


class _UidType(TypedDict):
    uid: int


def uid(username: str) -> _UidType:
    return {"uid": pwd.getpwnam(username).pw_uid}


def uid_to_username(uid: int) -> str:
    try:
        return pwd.getpwuid(uid).pw_name
    except KeyError:  # example: `KeyError: 'getpwuid(): uid not found: XXX'`
        return str(uid)


def list_users(adhoc_group: str) -> list[str]:
    # FIXME this only works if adhoc_group is not
    # a user's primary group. Workaround on systems managed
    # with IPA (LDAP, SSSD etc) TBD.
    return grp.getgrnam(adhoc_group).gr_mem


def check_can_create(name: str, adhoc_group: str):
    """
    Check if the user is permitted to create ad-hoc projects
    """
    usersgroup = grp.getgrnam(adhoc_group)
    try:
        info = pwd.getpwnam(name)
        groups = os.getgrouplist(name, info.pw_gid)
        if usersgroup.gr_gid not in groups:
            raise ValidationError(f'User "{name}" is not in the group "{adhoc_group}".')
    except KeyError:
        raise ValidationError(f'User "{name}" not found.')


class Dataset(NamedTuple):
    name: str  # the relative path of the dataset in its project
    immutable: bool
    ctime: float
    mtime: float


@final
class AdhocProject:
    def __init__(self, name: str, adhoc_group: str | None = None):
        if adhoc_group is None:
            adhoc_group = ADHOC_GROUP
        self.adhoc_group = adhoc_group
        if not self.is_adhoc(name):
            raise ValidationError(f'"{name}" is not an ad-hoc project.')
        self._name = name

    @property
    def name(self):
        return self._name

    @property
    def path(self):
        return os.path.join(ADHOC_PATH, self.name)

    @property
    def relpath(self):
        return os.path.join(ADHOC, self.name)

    @property
    def gid(self):
        return os.stat(self.path).st_gid

    @property
    def mtime(self):
        return os.path.getmtime(self.path)

    @property
    def ctime(self):
        return os.path.getctime(self.path)

    @classmethod
    def create(
        cls, name: str, owner: str, members: list[str], adhoc_group: str | None = None
    ):
        """
        Create a new ad-hoc project folder

        This creates an empty folder in the
        folder for ad-hoc projects after checking if boundary conditions are met.

        The folder is owned by the specified owner and the group for ad-hoc projects.
        The owner and the members receive ACLs and default ACLs that allow creating
        new files and directories. Project membership is defined by having an ACL
        of tag_type :code:`posix1e.ACL_USER`
        for this directory. Project ownership is defined through file ownerhip of the project folder.

        Group and Other permissions are set for read-only. Sticky bit and setgid are set as well.

        The watchdog influences ACLs and permissions for newly added files, see there for specifics.

        Possible exceptions: Boundary conditions not met -> ValidationError
        Permission issues -> OSError and the like
        Subprocess call failed -> subprocess.CalledProcessError
        """
        if adhoc_group is None:
            adhoc_group = ADHOC_GROUP
        check_can_create(owner, adhoc_group)
        check_valid_group(name)
        check_project_name_taken(name)
        if is_group(name):
            # Best effort check: Avoid collision with
            # existing groups to try keeping a 1:1 match
            # between project name and group name, just
            # for "keeping order"
            raise ValidationError("Conflicts with existing group.")
        folder = os.path.join(ADHOC_PATH, name)
        os.mkdir(folder)
        try:
            shutil.chown(folder, owner, adhoc_group)

            mode = stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR  # u+rwx
            mode |= (
                stat.S_IRGRP | stat.S_IXGRP
            )  # g+rx  -> explicitly state the should-state, don't rely on the umask
            mode |= stat.S_IROTH | stat.S_IXOTH  # o+rx
            mode |= stat.S_ISVTX  # sticky / "restricted deletion" bit
            mode |= stat.S_ISGID

            os.chmod(folder, mode)

            # Initialize ACL since it is difficult and brittle to do that
            # with pylibacl
            _ = subprocess.run(
                ["setfacl", "-m", "o:rX,d:o:rX", folder],
                capture_output=True,
                check=True,
            )
            project = cls(name, adhoc_group=adhoc_group)
            project.add_members(members + [owner], owner)
        except Exception as e:
            os.rmdir(folder)
            raise e
        return project

    def check_can_modify(self, username: str):
        if self.owner != username:
            raise ValidationError(
                f'User "{username}" cannot modify project {self.name}: Not an owner.'
            )

    def check_can_seal(self, dataset: str, username: str):
        if self.owner == username:
            return
        # FIXME or check write permission to dataset path?
        if username in self.members:
            return
        raise ValidationError(
            f'User "{username}" cannot seal dataset {dataset} in project {self.name}: Not an owner or member.'
        )

    @staticmethod
    def list_all() -> list[str]:
        result: list[str] = []
        files = os.listdir(ADHOC_PATH)
        for f in files:
            if os.path.isdir(os.path.join(ADHOC_PATH, f)):
                result.append(f)
        return result

    @classmethod
    def is_adhoc(cls, project_name: str) -> bool:
        path = os.path.join(ADHOC_PATH, project_name)
        # Be permissive with the kind of paths that work,
        # but check that there are no shenanigans with special characters
        # or links.
        return os.path.isdir(path) and os.path.realpath(path) == path

    @property
    def owner(self) -> str:
        uid = os.stat(self.path).st_uid
        return uid_to_username(uid)

    @property
    def members(self) -> list[str]:
        members = []
        # Members have a default ACL for the ad-hoc project's folder
        acl = posix1e.ACL(filedef=self.path)
        for a in acl:
            if a.tag_type is posix1e.ACL_USER:
                members.append(uid_to_username(a.qualifier))
        return list(set(members))

    @staticmethod
    def get_initial_acl(filename, default=False):
        if default:
            acl = posix1e.ACL(filedef=filename)
        else:
            acl = posix1e.ACL(file=filename)
        initial_res = acl.check()
        # If invalid because empty, initialize with a blank ACL
        # corresponding to the file permissions
        if initial_res is not False:
            # Empty ACL and no other error
            if initial_res[0] == posix1e.ACL_MISS_ERROR and initial_res[1] == 0:
                st = os.stat(filename)
                acl = posix1e.ACL(mode=st.st_mode)
                if acl.check() is not False:
                    raise ACLError(
                        f"Attempt to generate initial ACL {str(acl)} failed for {filename}."
                    )
            else:
                raise ACLError(
                    f"Invalid pre-existing ACL encountered: ACL {acl} error {str(initial_res)} for {filename}"
                )
        return acl

    def add_members(self, usernames: list[str], user: str) -> None:
        """Add users listed in `usernames` to this project.


        Exceptions can come from posix1e, os.walk,
        :func:`get_immutable`and :func:`set_immutable`. Most will be added
        to a collection and raised as :class:`AccumulatedMemberError` in the end.

        os.walk doesn't follow symlinks by default. The option set for clarity.
        Also, the function only modifies permissions for directories,
        and regular users can't create hard links to directories.
        If any of the limitations were lifted, a user could
        add a link to another file and have its permissions
        manipulated!

        TOCTOU vulnerabilities are probably still present.

        Parameters
        ----------
        usernames
            The users to add to this project

        user
            The user who wants to perform the action

        """

        def add(
            entries: list[posix1e.Entry], new_uids: Collection[int], filename: str
        ) -> None:
            acl = self.get_initial_acl(filename, default=False)
            # Delete any existing entries for the new user
            for entry in acl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in new_uids:
                    acl.delete_entry(entry)
            for entry in entries:
                acl.append(entry)
            acl.calc_mask()
            check_res = acl.check()
            if check_res is not False:
                raise ACLError(
                    f"Invalid ACL encountered: ACL {acl} error {str(check_res)} for {filename}"
                )
            acl.applyto(filename)

            defacl = self.get_initial_acl(filename, default=True)
            # Delete any existing entries for the new user
            for entry in defacl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in new_uids:
                    defacl.delete_entry(entry)
            for entry in entries:
                defacl.append(entry)
            defacl.calc_mask()
            defcheck_res = defacl.check()
            if defcheck_res is not False:
                raise ACLError(
                    f"Invalid ACL encountered: ACL {defacl} error {str(defcheck_res)} for {filename}"
                )
            defacl.applyto(filename, posix1e.ACL_TYPE_DEFAULT)

        self.check_can_modify(user)
        source = posix1e.ACL(file=self.path)
        new_uids = set(pwd.getpwnam(username).pw_uid for username in usernames)
        new_acl_entries = []
        # List of (path, exception) tuples
        errors = []
        for uid in new_uids:
            new = source.append()
            new.tag_type = posix1e.ACL_USER
            new.qualifier = uid
            new.permset.read = True
            new.permset.write = True
            new.permset.execute = True
            new_acl_entries.append(new)

        # Bail if this throws an exception
        add(new_acl_entries, new_uids, self.path)

        # Go twice to make race condition between
        # creation with default acl and walking less likely
        # FIXME repeat until ACLs remain constant
        # How to test ACL equivalency?
        for repeat in range(2):
            for root, dirs, files in os.walk(self.path, followlinks=False):
                for directory in dirs:
                    target = os.path.join(root, directory)
                    try:
                        if os.path.islink(target):
                            continue
                        immutable = is_immutable(target)
                        try:
                            if immutable:
                                set_immutable(target, False)
                            add(new_acl_entries, new_uids, target)
                        finally:
                            if immutable:
                                set_immutable(target, True)
                    except FS_ERRORS as e:
                        errors.append((target, e))
        if errors:
            raise AccumulatedMemberError("Encountered errors when walking path", errors)

    def remove_members(self, usernames: list[str], user: str) -> None:
        """
        See :meth:`add_members()` for safety considerations!
        """

        def remove(uids: Collection[int], filename):
            acl = self.get_initial_acl(filename, default=False)
            for entry in acl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in uids:
                    acl.delete_entry(entry)
            check_res = acl.check()
            if check_res is not False:
                raise ACLError(
                    f"Invalid ACL encountered: ACL {acl} error {str(check_res)} for {filename}"
                )
            acl.applyto(filename)
            defacl = self.get_initial_acl(filename, default=True)
            for entry in defacl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in uids:
                    defacl.delete_entry(entry)
            defcheck_res = defacl.check()
            if defcheck_res is not False:
                raise ACLError(
                    f"Invalid ACL encountered: ACL {defacl} error {str(defcheck_res)} for {filename}"
                )
            defacl.applyto(filename, posix1e.ACL_TYPE_DEFAULT)

        self.check_can_modify(user)
        uids = set(pwd.getpwnam(username).pw_uid for username in usernames)
        # List of (path, exception) tuples
        errors = []
        try:
            remove(uids, self.path)
        except FS_ERRORS as e:
            errors.append((self.path, e))
        # See above add_members() for safety considerations
        # Go twice to make race condition between
        # creation with default acl and walking less likely
        # FIXME repeat until ACLs remain constant
        # How to test ACL equivalency?
        for repeat in range(2):
            for root, dirs, files in os.walk(self.path, followlinks=False):
                # Don't manipulate files, only directories. Files should have
                # their ACLs dropped by watchdog after creation anyway,
                # they would be susceptible to shenanigans with hard links,
                # and it saves us a lot of file system operations.
                for directory in dirs:
                    target = os.path.join(root, directory)
                    try:
                        # Will return False if it can't access target, won't throw Exception
                        if os.path.islink(target):
                            continue
                        immutable = is_immutable(target)
                        try:
                            if immutable:
                                set_immutable(target, False)
                            remove(uids, target)
                        finally:
                            if immutable:
                                set_immutable(target, True)
                    except FS_ERRORS as e:
                        errors.append((target, e))
        if errors:
            raise AccumulatedMemberError("Encountered errors when walking path", errors)

    @property
    def datasets(self) -> list[Dataset]:
        result: list[Dataset] = []
        for filename in os.listdir(self.path):
            root = os.path.join(self.path, filename)
            raw = os.path.join(root, "raw")
            if os.path.isdir(raw):
                result.append(
                    Dataset(
                        name=filename,
                        immutable=is_immutable(raw),
                        ctime=min(os.path.getctime(root), os.path.getctime(raw)),
                        mtime=max(os.path.getmtime(root), os.path.getmtime(raw)),
                    )
                )
        return result

    def add_dataset(self, name: str, user: str):
        """
        Permissions, ACLs etc will be set by default ACLs and the watchdog.
        """
        check_valid_dataset(name)
        check_can_create(user, self.adhoc_group)
        target_base_path = os.path.join(self.path, name)
        target_raw_path = os.path.join(target_base_path, "raw")
        # Run as the specified user to make sure
        # file system permissions are enforced
        # Don't use mkdir -p so that an error is triggered if directory exists

        _ = subprocess.run(
            ["sudo", "-u", user, "mkdir", target_base_path, target_raw_path],
            capture_output=True,
            check=True,
            text=True,
        )

    def seal_dataset(self, name: str, user: str):
        """
        Recursively set the :code:`raw` subfolder of a dataset and its contents immutable

        FIXME: Sensitive to TOCTOU, seee https://iffgit.fz-juelich.de/weber/data-management-system/-/merge_requests/1#note_27618

        Throws `ValidationError` if the :code:`raw` subfolder doesn't exist or is a link.
        Symlinks are ignored.

        This is intended to be called after an ingest session ends.
        """
        self.check_can_seal(name, user)
        target_base_path = os.path.join(self.path, name)
        target_raw_path = os.path.join(target_base_path, "raw")
        # The check for realpath is important to prevent mocking a dataset
        # using symbolic links that points to files beyond file system
        # boundaries. Otherwise this could lock them!
        # That can trigger a DoS if log files are targeted, for example.
        if os.path.realpath(target_raw_path) != target_raw_path:
            raise ValidationError(
                "Real path and specified path are different, "
                "could be caused by symlink."
            )
        if not os.path.isdir(target_raw_path):
            raise ValidationError("Raw folder does not exist.")
        set_immutable(target_raw_path, True)
        # Any hard links to files will also be set immutable.
        # FIXME That allows users to lock other's files by linking them
        # in a dataset and then sealing the dataset. Is there a way to
        # prevent that? Error out if the link count is not 1?
        # Compare file and project ownership?
        # For sharing of files in private projects there's a legitimate
        # reason to create hard links, so this may not work.
        # Also, there might be legitimate reasons to have other's
        # files in a project.
        # This can only trigger a mild, reversible DoS as long as only files
        # within the data management can be targeted, not data loss,
        # privilege escalation or data manipulation from my understanding.
        # For that reason it is left as-is for now.
        for root, dirs, files in os.walk(target_raw_path, followlinks=False):
            for dir in dirs + files:
                target = os.path.join(root, dir)
                # Skip soft links, they would trigger
                # an error.
                if not os.path.islink(target):
                    set_immutable(target, True)

    @property
    def diskusage(self) -> int:
        result = subprocess.run(
            ["du", "--total", "--summarize", "--bytes", self.path],
            capture_output=True,
            check=True,
            text=True,
        )
        lines = result.stdout.split("\n")
        last_line = lines[-2]  # Last one is empty string
        size = last_line.split("\t")[0]
        return int(size)

    def serialize(self, fast: bool = False) -> SerializedProject:
        """
        Parameters
        ----------
        fast
            Don't perform potentially expensive operations, like ones that
            stat all files in the project
        """
        usage = None
        if not fast:
            usage = self.diskusage
        serialized: SerializedProject = {
            "relpath": self.relpath,
            "name": self.name,
            "owner": self.owner,
            "members": self.members,
            "ctime": self.ctime,
            "mtime": self.mtime,
            "gid": self.gid,
            "usage_bytes": usage,
        }
        return serialized


if __name__ == "__main__":
    # info = pwd.getpwnam('weber')
    # groups = os.getgrouplist('weber', info.pw_gid)
    # print("native", groups)
    # print("pysss", pysss.getgrouplist('weber'))

    otherproj = AdhocProject("kidney")
    print(otherproj.diskusage)
    print(otherproj.members)
    otherproj.add_members(["testjanghyun", "testalex"])
    print(otherproj.owner)

    myproj = AdhocProject.create(
        "pythoncreated", "testdieter", ["testalex", "testpenghan"]
    )
    assert set(myproj.members) == set(("testdieter", "testalex", "testpenghan"))
    myproj.add_members(["testjanghyun", "testthibaud"])
    myproj.add_members(["testjanghyun", "testthibaud"])
    assert set(myproj.members) == set(
        ("testdieter", "testalex", "testpenghan", "testjanghyun", "testthibaud")
    )
    myproj.remove_members(["testpenghan", "testalex"])
    assert set(myproj.members) == set(("testdieter", "testjanghyun", "testthibaud"))
    myproj.add_dataset("helloworld", "testjanghyun")

    print(AdhocProject.list_all())
    subprocess.run(
        [
            "sudo",
            "-u",
            "testdieter",
            "touch",
            "/data/adhoc/pythoncreated/helloworld/raw/image1.dm4",
        ],
        capture_output=True,
        check=True,
        text=True,
    )
    print(myproj.datasets)
    myproj.seal_dataset("helloworld")
    assert is_immutable("/data/adhoc/pythoncreated/helloworld/raw/image1.dm4")
