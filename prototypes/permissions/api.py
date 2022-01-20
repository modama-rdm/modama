import os
import pwd
import fcntl
import array

import posix1e  # pylibacl


BASE = '/data/'
ADHOC = os.path.join(BASE, 'adhoc')

# From https://www.geeklab.info/2021/04/chattr-and-lsattr-in-python/
FS_IOC_GETFLAGS = 0x80086601
FS_IOC_SETFLAGS = 0x40086602
FS_IMMUTABLE_FL = 0x010


def list_adhoc():
    files = os.listdir(ADHOC)
    for f in files:
        if os.path.isdir(os.path.join(ADHOC, f)):
            yield f


def list_members(adhoc_project):
    path = os.path.join(ADHOC, adhoc_project)
    assert os.path.isdir(path)
    acl = posix1e.ACL(filedef=path)
    for a in acl:
        if a.tag_type is posix1e.ACL_USER:
            yield pwd.getpwuid(a.qualifier)


def add_member(username, adhoc_project):
    def add(entry, file):
        acl = posix1e.ACL(file=file)
        acl.append(new)
        acl.applyto(file)
        defacl = posix1e.ACL(filedef=file)
        defacl.append(new)
        defacl.applyto(file, posix1e.ACL_TYPE_DEFAULT)

    path = os.path.join(ADHOC, adhoc_project)
    assert os.path.isdir(path)
    source = posix1e.ACL(file=path)
    new = source.append()
    new.tag_type = posix1e.ACL_USER
    new.qualifier = pwd.getpwnam(username).pw_uid
    new.permset.read = True
    new.permset.write = True
    new.permset.execute = True
    add(new, path)
    for root, dirs, files in os.walk(os.path.join(ADHOC, adhoc_project)):
        for dir in dirs:
            target = os.path.join(root, dir)
            immutable = is_immutable(target)
            try:
                if immutable:
                    set_immutable(target, False)
                add(new, target)
            finally:
                if immutable:
                    set_immutable(target, True)


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


def unseal(path):
    immutables = []
    for root, dirs, files in os.walk(path):
        for dir in dirs:
            target = os.path.join(root, dir)
            if is_immutable(target):
                immutables.append(target)
                set_immutable(target, False)

        for file in files:
            target = os.path.join(root, file)
            if is_immutable(target):
                immutables.append(target)
                set_immutable(target, False)
    return immutables


def seal(immutables):
    for target in immutables:
        set_immutable(target, True)


if __name__ == '__main__':
    for project in list_adhoc():
        has_janghyun = False
        new_members = list(list_members(project))
        print(new_members)
        for m in new_members:
            if m.pw_name == 'testjanghyun':
                has_janghyun = True
        assert not has_janghyun

        add_member('testjanghyun', project)

        new_members = list(list_members(project))
        for m in new_members:
            if m.pw_name == 'testjanghyun':
                has_janghyun = True
        print(new_members)
        assert has_janghyun
