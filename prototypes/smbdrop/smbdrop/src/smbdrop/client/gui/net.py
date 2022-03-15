import win32net


def get_drives():
    # adapted from http://timgolden.me.uk/python/win32_how_do_i/show_mapped_drives.html
    result = []
    resume = 0
    while 1:
        (drives, total, resume) = win32net.NetUseEnum (None, 0, resume)
        for drive in drives:
            if drive['local']:
                _, _, server, share = drive['remote'].split('\\')
                result.append((drive['local'], server, share))
        if not resume:
            break
    return result
