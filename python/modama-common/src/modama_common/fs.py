import os
import fcntl
import array


# From https://www.geeklab.info/2021/04/chattr-and-lsattr-in-python/
FS_IOC_GETFLAGS = 0x80086601
FS_IOC_SETFLAGS = 0x40086602
FS_IMMUTABLE_FL = 0x010


# Based on https://www.geeklab.info/2021/04/chattr-and-lsattr-in-python/
def is_immutable(file_path: str):
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
def set_immutable(file_path: str, immutable: bool):
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
