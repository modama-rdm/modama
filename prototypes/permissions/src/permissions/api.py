import os
import pwd
import grp
import fcntl
import array
import re
import subprocess
import stat
import shutil
from typing import NamedTuple, List

import pysss  # python3-sss
import posix1e  # pylibacl, needs libacl1-dev, https://pylibacl.k1024.org/


BASE_PATH = '/data/'
ADHOC_PATH = os.path.join(BASE_PATH, 'adhoc')
SHARING_PATH = os.path.join(BASE_PATH, 'sharing')

ADHOC_GROUP = 'testerc'

# From https://www.geeklab.info/2021/04/chattr-and-lsattr-in-python/
FS_IOC_GETFLAGS = 0x80086601
FS_IOC_SETFLAGS = 0x40086602
FS_IMMUTABLE_FL = 0x010

with open('/etc/group', 'r') as groupfile:
    system_groups = set(line.split(':')[0] for line in groupfile)


class ValidationError(ValueError):
    '''
    Something ist wrong with the user input or files related to the request.

    Details provided in error message.
    '''
    pass


class ACLError(IOError):
    '''
    Checking an ACL failed for some reason

    See https://pylibacl.k1024.org/module.html#posix1e.ACL.check
    '''
    pass


# From https://unix.stackexchange.com/questions/11477/what-are-the-allowed-group-names-for-groupadd
# See https://github.com/shadow-maint/shadow/blob/master/libmisc/chkname.c
# Go with most conservative approach
def check_valid_group(groupname: str):
    '''
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
    '''
    if groupname in system_groups:
        raise ValidationError("Is a system group.")
    if len(groupname) > 16:
        raise ValidationError(f"Too long: Maximum length is 16, actual length is {len(groupname)}.")
    if len(groupname) < 3:
        raise ValidationError(f"Too short: Minimum length is 3, actual length is {len(groupname)}.")
    if not re.fullmatch('[a-z]', groupname[0]):
        raise ValidationError(f"First character must be in a-z, is {groupname[0]}.")
    if not re.fullmatch('[-a-z_0-9]*', groupname):
        raise ValidationError("Can only use characters a-z, 0-9, '_' and '-'.")


def check_valid_dataset(name: str):
    # Restriction on some file systems, avoid "funny business" from user input
    if len(name) > 255:
        raise ValidationError(f"Too long: Maximum length is 255, actual length is {len(name)}.")
    if len(name) < 1:
        raise ValidationError(f"Too short: Minimum length is 1, actual length is {len(name)}.")
    # Restrictive positive list to avoid "funny business" from user input
    if not re.fullmatch('[-a-zA-Z0-9_.]*', name):
        raise ValidationError("Dataset name can only contain characters a-z, A-Z, 0-9, '_', '-' and '.'.")


def check_project_name_taken(name):
    if os.path.exists(os.path.join(BASE_PATH, name)):
        raise ValidationError("File or folder exists in base folder.")
    if os.path.exists(os.path.join(ADHOC_PATH, name)):
        raise ValidationError("File or folder exists in adhoc folder.")
    if os.path.exists(os.path.join(SHARING_PATH, name)):
        raise ValidationError("File or exists in sharing folder.")


def is_group(name):
    try:
        grp.getgrnam(name)
        return True
    except KeyError:
        return False


def check_can_create(name):
    '''
    Check if the user is permitted to create ad-hoc projects
    '''
    usersgroup = grp.getgrnam(ADHOC_GROUP)
    try:
        info = pwd.getpwnam(name)
        groups = os.getgrouplist(name, info.pw_gid)
        if usersgroup.gr_gid not in groups:
            raise ValidationError(f'User "{name}" is not in the group "{ADHOC_GROUP}".')
    except KeyError:
        raise ValidationError(f'User "{name}" not found.')


# Based on https://www.geeklab.info/2021/04/chattr-and-lsattr-in-python/
def is_immutable(file_path):
    '''
    Raises :class:`OSError` or subclasses thereof
    if :func:`os.open` or :func:`fcntl.ioctl` fail, for whatever reason.

    See also ioctl_iflags(2)
    '''
    fd = None
    try:
        fd = os.open(file_path, os.O_RDONLY)
        flags = array.array('L', [0])
        fcntl.ioctl(fd, FS_IOC_GETFLAGS, flags, True)
        return bool(flags[0] & FS_IMMUTABLE_FL)
    finally:
        if fd is not None:
            os.close(fd)


# Based on https://www.geeklab.info/2021/04/chattr-and-lsattr-in-python/
def set_immutable(file_path, immutable):
    '''
    Raises :class:`OSError` or subclasses thereof
    if :func:`os.open` or :func:`fcntl.ioctl` fail, for whatever reason.

    See also ioctl_iflags(2)
    '''
    fd = None
    try:
        fd = os.open(file_path, os.O_RDONLY)
        flags = array.array('L', [0])
        fcntl.ioctl(fd, FS_IOC_GETFLAGS, flags, True)
        if immutable:
            flags[0] = flags[0] | FS_IMMUTABLE_FL
        else:
            flags[0] = flags[0] & ~FS_IMMUTABLE_FL
        fcntl.ioctl(fd, FS_IOC_SETFLAGS, flags, True)
    finally:
        if fd is not None:
            os.close(fd)


class Dataset(NamedTuple):
    name: str  # the relative path of the dataset in its project
    immutable: bool


class AdhocProject:
    def __init__(self, name):
        if not self.is_adhoc(name):
            raise ValidationError('Not an ad-hoc project.')
        self._name = name

    @property
    def name(self):
        return self._name

    @property
    def path(self):
        return os.path.join(ADHOC_PATH, self.name)

    @classmethod
    def create(cls, name: str, owner: str, members: List[str]):
        '''
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
        '''
        check_can_create(owner)
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
            shutil.chown(folder, owner, ADHOC_GROUP)

            mode  = stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR  # u+rwx
            mode |= stat.S_IRGRP | stat.S_IXGRP               # g+rx  -> explicitly state the should-state, don't rely on the umask
            mode |= stat.S_IROTH | stat.S_IXOTH               # o+rx
            mode |= stat.S_ISVTX                              # sticky / "restricted deletion" bit
            mode |= stat.S_ISGID

            os.chmod(folder, mode)

            # Initialize ACL since it is difficult and brittle to do that
            # with pylibacl
            subprocess.run(
                ['setfacl', '-m', 'o:rX,d:o:rX', folder],
                capture_output=True, check=True
            )
            project = cls(name)
            project.add_members(members + [owner])
        except Exception as e:
            os.rmdir(folder)
            raise e
        return project

    @staticmethod
    def list_all():
        result = []
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
        return pwd.getpwuid(os.stat(self.path).st_uid).pw_name

    @property
    def members(self) -> List[str]:
        members = []
        # Members have a default ACL for the ad-hoc project's folder
        acl = posix1e.ACL(filedef=self.path)
        for a in acl:
            if a.tag_type is posix1e.ACL_USER:
                members.append(pwd.getpwuid(a.qualifier).pw_name)
        return list(set(members))

    def add_members(self, usernames: List[str]) -> None:
        '''
        Exceptions can come from posix1e, os.walk,
        :func:`get_immutable`and :func:`set_immutable`.

        os.walk doesn't follow symlinks by default. The option set for clarity.
        Also, the function only modifies permissions for directories,
        and regular users can't create hard links to directories.
        If any of the limitations were lifted, a user could
        add a link to another file and have its permissions
        manipulated!
        '''
        def add(entries: List[posix1e.Entry], new_uids: List[int], filename: str) -> None:
            acl = posix1e.ACL(file=filename)
            # Delete any existing entries for the new user
            for entry in acl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in new_uids:
                    acl.delete_entry(entry)
            for entry in entries:
                acl.append(entry)
            acl.calc_mask()
            check_res = acl.check()
            if check_res is not False:
                raise ACLError(f"Invalid ACL encountered: {str(check_res)}")
            acl.applyto(filename)

            defacl = posix1e.ACL(filedef=filename)
            # Delete any existing entries for the new user
            for entry in defacl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in new_uids:
                    defacl.delete_entry(entry)
            for entry in entries:
                defacl.append(entry)
            defacl.calc_mask()
            defcheck_res = defacl.check()
            if defcheck_res is not False:
                raise ACLError(f"Invalid ACL encountered: {str(defcheck_res)}")
            defacl.applyto(filename, posix1e.ACL_TYPE_DEFAULT)

        source = posix1e.ACL(file=self.path)
        new_uids = set(pwd.getpwnam(username).pw_uid for username in usernames)
        new_acl_entries = []
        for uid in new_uids:
            new = source.append()
            new.tag_type = posix1e.ACL_USER
            new.qualifier = uid
            new.permset.read = True
            new.permset.write = True
            new.permset.execute = True
            new_acl_entries.append(new)
        add(new_acl_entries, new_uids, self.path)

        # Go twice to make race condition between
        # creation with default acl and walking less likely
        # FIXME repeat until ACLs remain constant
        # How to test ACL equivalency?
        for repeat in range(2):
            for root, dirs, files in os.walk(self.path, followlinks=False):
                for directory in dirs:
                    target = os.path.join(root, directory)
                    immutable = is_immutable(target)
                    try:
                        if immutable:
                            set_immutable(target, False)
                        add(new_acl_entries, new_uids, target)
                    finally:
                        if immutable:
                            set_immutable(target, True)

    def remove_members(self, usernames):
        '''
        See :meth:`add_members()` for safety considerations!
        '''
        def remove(uids, filename):
            acl = posix1e.ACL(file=filename)
            for entry in acl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in uids:
                    acl.delete_entry(entry)
            check_res = acl.check()
            if check_res is not False:
                raise ACLError(f"Invalid ACL encountered: {str(check_res)}")
            acl.applyto(filename)
            defacl = posix1e.ACL(filedef=filename)
            for entry in defacl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in uids:
                    defacl.delete_entry(entry)
            defcheck_res = defacl.check()
            if defcheck_res is not False:
                raise ACLError(f"Invalid ACL encountered: {str(defcheck_res)}")
            defacl.applyto(filename, posix1e.ACL_TYPE_DEFAULT)

        uids = set(pwd.getpwnam(username).pw_uid for username in usernames)
        remove(uids, self.path)
        # See above add_members() for safety considerations
        # Go twice to make race condition between
        # creation with default acl and walking less likely
        # FIXME repeat until ACLs remain constant
        # How to test ACL equivalency?
        for repeat in range(2):
            for root, dirs, files in os.walk(self.path):
                # Don't manipulate files, only directories. Files should have
                # their ACLs dropped by watchdog after creation anyway,
                # they would be susceptible to shenanigans with hard links,
                # and it saves us a lot of file system operations.
                for directory in dirs:
                    target = os.path.join(root, directory)
                    immutable = is_immutable(target)
                    try:
                        if immutable:
                            set_immutable(target, False)
                        remove(uids, target)
                    finally:
                        if immutable:
                            set_immutable(target, True)

    @property
    def datasets(self):
        result = []
        for filename in os.listdir(self.path):
            raw = os.path.join(self.path, filename, 'raw')
            if os.path.isdir(raw):
                result.append(Dataset(name=filename, immutable=is_immutable(raw)))
        return result

    def add_dataset(self, name, user):
        '''
        Permissions, ACLs etc will be set by default ACLs and the watchdog.
        '''
        check_valid_dataset(name)
        check_can_create(user)
        target_base_path = os.path.join(self.path, name)
        target_raw_path = os.path.join(target_base_path, 'raw')
        # Run as the specified user to make sure
        # file system permissions are enforced
        # Don't use mkdir -p so that an error is triggered if directory exists

        subprocess.run(
            ['sudo', '-u', user, 'mkdir', target_base_path, target_raw_path],
            capture_output=True, check=True, text=True
        )

    def seal_dataset(self, name):
        '''
        Recursively set the :code:`raw` subfolder of a dataset and its contents immutable

        FIXME: Sensitive to TOCTOU, seee https://iffgit.fz-juelich.de/weber/data-management-system/-/merge_requests/1#note_27618

        Throws `ValidationError` if the :code:`raw` subfolder doesn't exist or is a link.
        Symlinks are ignored.

        This is intended to be called after an ingest session ends.
        '''
        target_base_path = os.path.join(self.path, name)
        target_raw_path = os.path.join(target_base_path, 'raw')
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
    def diskusage(self):
        result = subprocess.run(
            ['du', '--total', '--summarize', '--bytes', self.path],
            capture_output=True, check=True, text=True
        )
        lines = result.stdout.split('\n')
        last_line = lines[-2]  # Last one is empty string
        size = last_line.split('\t')[0]
        return int(size)

    def serialize(self, fast: bool = False):
        """
        Parameters
        ----------
        fast
            Don't perform potentially expensive operations, like ones that
            stat all files in the project
        """
        serialized = {
            "name": self.name,
            "members": self.members,
        }
        if not fast:
            serialized["usage_bytes"] = self.diskusage
        return serialized



if __name__ == '__main__':
    # info = pwd.getpwnam('weber')
    # groups = os.getgrouplist('weber', info.pw_gid)
    # print("native", groups)
    # print("pysss", pysss.getgrouplist('weber'))

    otherproj = AdhocProject('kidney')
    print(otherproj.diskusage)
    print(otherproj.members)
    otherproj.add_members(['testjanghyun', 'testalex'])
    print(otherproj.owner)

    myproj = AdhocProject.create('pythoncreated', 'testdieter', ['testalex', 'testpenghan'])
    assert set(myproj.members) == set(('testdieter', 'testalex', 'testpenghan'))
    myproj.add_members(['testjanghyun', 'testthibaud'])
    myproj.add_members(['testjanghyun', 'testthibaud'])
    assert set(myproj.members) == set(('testdieter', 'testalex', 'testpenghan', 'testjanghyun', 'testthibaud'))
    myproj.remove_members(['testpenghan', 'testalex'])
    assert set(myproj.members) == set(('testdieter', 'testjanghyun', 'testthibaud'))
    myproj.add_dataset('helloworld', 'testjanghyun')

    print(AdhocProject.list_all())
    subprocess.run(
        ['sudo', '-u', 'testdieter', 'touch', '/data/adhoc/pythoncreated/helloworld/raw/image1.dm4'],
        capture_output=True, check=True, text=True
    )
    print(myproj.datasets)
    myproj.seal_dataset('helloworld')
    assert is_immutable('/data/adhoc/pythoncreated/helloworld/raw/image1.dm4')
