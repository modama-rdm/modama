Known Issues
============

Uncertainties of permission handling
------------------------------------

ACLs, other file system permissions and the tools to manipulate them interact in a complex way. The code still requires more tests, and we may take some time to cover all scenarios that can occur in real-world use. As a specific example, the effect of a user's UMASK and the UMASK of the process the API is used from should be tested.

Generally, the system should be documented well since it is rather specific and uses pretty much the full range of Linux file system capabilities.

Changing membership of ad-hoc groups
------------------------------------

Ad-hoc groups are currently based on ACLs that are set for the project folder and sub-folders. The ACLs for new files and folders are managed by a combination of default ACLs and a watchdog daemon. Changing membership requires walking the directory tree and changing all ACLs. That means an ambiguous situation can arise if project membership is changed while directories are created, since they may inherit old ACLs without being included in the walk that sets the new ACLs.

As a fix, the change could be applied repeatedly until a consistent situation is reached, i.e. no change was necessary between two subsequent runs. That would require implementing or finding a suitable comparison method for ACLs.

TOCTOU
------

Currently, checks are performed on paths. That opens the system to TOCTOU vulnerabilities. The code should be rewritten to use open file handles, including for directories. That means the underlying file object can't be switched out while a routine is running.

Error handling
--------------

The system involves complex interactions with the operating system that can trigger a wide range of errors. Their handling should be improved, following the needs of future downstream code.

File system character encoding
------------------------------

The file system may contain file names that are not valid Unicode strings. That may upset some routines and require fixes. We should test it with various operating systems, including older Windows versions, to make sure everything plays together nicely. See https://iffgit.fz-juelich.de/weber/data-management-system/-/merge_requests/1#note_27267

Protection against hard link shenanigans
----------------------------------------

We should make sure :code:`/proc/sys/fs/protected_hardlinks` is available and set to 1 to protect against "cross-talk" between projects that could subvert file system permissions and ownership. https://iffgit.fz-juelich.de/weber/data-management-system/-/merge_requests/1#note_27268

Protection against soft link shenanigans
----------------------------------------

One should make sure that all operations don't follow soft links so that they are limited to the same file system and can't be redirected.
