# SMBDrop

This folder contains the low-level services that allow dynamic creation
of SMB shares, where users can drop their data for a specific data set / session.
In particular, they are only concerned with creating and removing the samba shares,
and no higher level concerns, like selection of the destination path based on information
given by the user, or session lifetimes etc.
