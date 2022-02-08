import os
import pwd
import grp
import fcntl
import array
import re
import subprocess

import posix1e  # pylibacl


BASE = '/data/'
ADHOC = os.path.join(BASE, 'adhoc')
SHARING = os.path.join(BASE, 'sharing')

ADHOC_GROUP = 'testerc'

# From https://www.geeklab.info/2021/04/chattr-and-lsattr-in-python/
FS_IOC_GETFLAGS = 0x80086601
FS_IOC_SETFLAGS = 0x40086602
FS_IMMUTABLE_FL = 0x010

with open('/etc/group', 'r') as groupfile:
    system_groups = set(line.split(':')[0] for line in groupfile)


# From https://unix.stackexchange.com/questions/11477/what-are-the-allowed-group-names-for-groupadd
# See https://github.com/shadow-maint/shadow/blob/master/libmisc/chkname.c
# Go with most conservative approach
def check_valid_group(groupname: str):
    if groupname in system_groups:
        raise ValueError("Is a system group.")
    if len(groupname) > 16:
        raise ValueError(f"Too long: Maximum length is 16, actual length is {len(groupname)}.")
    if len(groupname) < 3:
        raise ValueError(f"Too short: Minimum length is 3, actual length is {len(groupname)}.")
    if not re.fullmatch('[a-z]', groupname[0]):
        raise ValueError(f"First character must be in a-z, is {groupname[0]}.")
    if not re.fullmatch('[-a-z_0-9]*', groupname):
        raise ValueError("Can only use characters a-z, 0-9, '_' and '-'.")


def check_valid_dataset(name: str):
    # Restriction on some file systems, avoid "funny business" from user input
    if len(name) > 255:
        raise ValueError(f"Too long: Maximum length is 255, actual length is {len(name)}.")
    # Restrictive positive list to avoid "funny business" from user input
    if not re.fullmatch('[-a-zA-Z0-9_.]*', name):
        raise ValueError("Dataset name can only contain characters a-z, A-Z, 0-9, '_', '-' and '.'.")


def check_taken(name):
    if os.path.exists(os.path.join(BASE, name)):
        raise ValueError("File or folder exists in base folder.")
    if os.path.exists(os.path.join(ADHOC, name)):
        raise ValueError("File or folder exists in adhoc folder.")
    if os.path.exists(os.path.join(SHARING, name)):
        raise ValueError("File or exists in sharing folder.")


def is_group(name):
    try:
        grp.getgrnam(name)
        return True
    except KeyError:
        return False


def check_valid_user(name):
    usersgroup = grp.getgrnam(ADHOC_GROUP)
    try:
        info = pwd.getpwnam(name)
        groups = os.getgrouplist(name, info.pw_gid)
        if usersgroup.gr_gid not in groups:
            raise ValueError(f'User "{name}" is not in the group "{ADHOC_GROUP}".')
    except KeyError:
        raise ValueError(f'User "{name}" not found.')


# Based on https://www.geeklab.info/2021/04/chattr-and-lsattr-in-python/
def is_immutable(file_path):
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


class AdhocProject:
    def __init__(self, name):
        if not self.is_adhoc(name):
            raise ValueError('Not an ad-hoc project.')
        self._name = name

    @property
    def name(self):
        return self._name

    @property
    def path(self):
        return os.path.join(ADHOC, self.name)

    @classmethod
    def create(cls, name: str, owner: str, members: List[str]):
        check_valid_user(owner)
        check_valid_group(name)
        check_taken(name)
        if is_group(name):
            raise ValueError("Conflicts with existing group.")
        folder = os.path.join(ADHOC, name)
        os.mkdir(folder)
        subprocess.run(
            ['chown', f'{owner}:{ADHOC_GROUP}', folder],
            capture_output=True, check=True
        )
        subprocess.run(
            ['chmod', 'u+rwx,go-w,g+s,+t', folder],
            capture_output=True, check=True
        )
        # Initialize ACL since it is difficult and brittle to do that
        # with pylibacl
        subprocess.run(
            ['setfacl', '-m', 'o:rX,d:o:rX', folder],
            capture_output=True, check=True
        )
        project = cls(name)
        project.add_members(members + [owner])
        return project

    @staticmethod
    def list_all():
        result = []
        files = os.listdir(ADHOC)
        for f in files:
            if os.path.isdir(os.path.join(ADHOC, f)):
                result.append(f)
        return result

    @classmethod
    def is_adhoc(cls, project_name: str) -> bool:
        path = os.path.join(ADHOC, adhoc_project)
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
        def add(entries: List[posix1e.Entry], file: str) -> None:
            acl = posix1e.ACL(file=file)
            for entry in entries:
                acl.append(entry)
            acl.calc_mask()
            acl.applyto(file)

            defacl = posix1e.ACL(filedef=file)
            for entry in entries:
                defacl.append(entry)
            defacl.calc_mask()
            defacl.applyto(file, posix1e.ACL_TYPE_DEFAULT)

        source = posix1e.ACL(file=self.path)
        news = []
        for username in usernames:
            new = source.append()
            new.tag_type = posix1e.ACL_USER
            new.qualifier = pwd.getpwnam(username).pw_uid
            new.permset.read = True
            new.permset.write = True
            new.permset.execute = True
            news.append(new)
        add(news, self.path)
        # os.walk doesn't follow symlinks by default, option set for clarity.
        # Also, the code below only modifies permissions for directories,
        # and regular users can't create hard links to directories.
        # If any of the limitations were lifted, a user could
        # add a link to another file and have its permissions
        # manipulated!
        for root, dirs, files in os.walk(self.path, followlinks=False):
            for dir in dirs:
                target = os.path.join(root, dir)
                immutable = is_immutable(target)
                try:
                    if immutable:
                        set_immutable(target, False)
                    add(news, target)
                finally:
                    if immutable:
                        set_immutable(target, True)

    def remove_members(self, usernames):
        def remove(uids, file):
            acl = posix1e.ACL(file=file)
            for entry in acl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in uids:
                    acl.delete_entry(entry)
            acl.applyto(file)
            defacl = posix1e.ACL(filedef=file)
            for entry in defacl:
                if entry.tag_type == posix1e.ACL_USER and entry.qualifier in uids:
                    defacl.delete_entry(entry)
            defacl.applyto(file, posix1e.ACL_TYPE_DEFAULT)

        uids = set(pwd.getpwnam(username).pw_uid for username in usernames)
        remove(uids, self.path)
        # See above add_members() for safety considerations
        for root, dirs, files in os.walk(self.path):
            # Don't manipulate files, only directories. Files should have
            # their ACLs dropped by watchdog after creation anyway,
            # they would be susceptible to shenanigans with hard links,
            # and it saves us a lot of file system operations.
            for dir in dirs:
                target = os.path.join(root, dir)
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
        for file in os.listdir(self.path):
            raw = os.path.join(self.path, file, 'raw')
            if os.path.isdir(raw):
                result.append((file, is_immutable(raw)))
        return result

    def add_dataset(self, name, user):
        check_valid_dataset(name)
        check_valid_user(user)
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
        target_base_path = os.path.join(self.path, name)
        target_raw_path = os.path.join(target_base_path, 'raw')
        # The check for realpath is important to prevent mocking a dataset
        # using symbolic links that points to files beyond file system
        # boundaries. Otherwise this could lock them!
        # That can trigger a DoS if log files are targeted, for example.
        if os.path.realpath(target_raw_path) != target_raw_path:
            raise ValueError(
                "Real path and specified path are different, "
                "could be caused by symlink."
            )
        if not os.path.isdir(target_raw_path):
            raise ValueError("Raw folder does not exist.")
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


if __name__ == '__main__':
    otherproj = AdhocProject('kidney')
    print(otherproj.diskusage)
    print(otherproj.members)
    otherproj.add_members(['testjanghyun', 'testalex'])
    print(otherproj.owner)

    myproj = AdhocProject.create('pythoncreated', 'testdieter', ['testalex', 'testpenghan'])
    assert set(myproj.members) == set(('testdieter', 'testalex', 'testpenghan'))
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
