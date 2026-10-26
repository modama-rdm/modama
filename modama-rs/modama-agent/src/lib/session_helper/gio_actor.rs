use std::{
    sync::{Arc, mpsc::SyncSender},
    time::Duration,
};

use gio::{
    Mount,
    prelude::{FileExt as _, MountExt, MountOperationExt, VolumeMonitorExt},
};
use glib::MainContext;
use modama_common::{
    internal_api_types::{MountedShare, SessionHelperConfig},
    messages::Share,
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use tokio::sync::mpsc::{UnboundedReceiver, error::TryRecvError};

pub enum GioActorCommand {
    Mount {
        reply: SyncSender<()>,
        share: Share,
    },
    Umount {
        reply: SyncSender<()>,
        mounted_share: MountedShare,
    },
    ListMounts {
        reply: SyncSender<Vec<MountedShare>>,
    },
    Shutdown,
}

fn find_mount_by_mounted_share(share: MountedShare, context: &MainContext) -> Mount {
    while context.pending() {
        context.iteration(false);
    }
    let mounts = gio::VolumeMonitor::get().mounts();
    todo!();
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(32);

fn run<O>(context: &MainContext, fut: impl Future<Output = O>) -> O {
    context
        .block_on(glib::future_with_timeout(DEFAULT_TIMEOUT, fut))
        .unwrap()
}

fn umount(share: MountedShare, context: &MainContext) {
    let mount = find_mount_by_mounted_share(share, context);
    if mount.can_unmount() {
        run(
            context,
            mount.unmount_with_operation_future(
                gio::MountUnmountFlags::NONE,
                gio::MountOperation::NONE,
            ),
        )
        .unwrap();
    }
}

fn mount(share: &Share, context: &MainContext, config: &SessionHelperConfig) -> MountedShare {
    let share_name = utf8_percent_encode(&share.share_name, NON_ALPHANUMERIC);
    let uri = if config.smb_port == 445 {
        format!("smb://{}/{}", config.smb_hostname, share_name)
    } else {
        format!(
            "smb://{}:{}/{}",
            config.smb_hostname, config.smb_port, share_name
        )
    };
    let file = gio::File::for_uri(&uri);

    let config = config.clone();
    let share = share.clone();
    let op = gio::MountOperation::new();
    op.set_password_save(gio::PasswordSave::Never);
    op.connect_ask_password(
        move |operation, _message, _default_user, _default_domain, flags| {
            if flags.contains(gio::AskPasswordFlags::NEED_USERNAME) {
                operation.set_username(Some(&share.username));
            }
            if flags.contains(gio::AskPasswordFlags::NEED_PASSWORD) {
                operation.set_password(Some(&share.password));
            }
            if flags.contains(gio::AskPasswordFlags::NEED_DOMAIN) {
                operation.set_domain(Some(&config.smb_domain));
            }
            operation.set_password_save(gio::PasswordSave::Never);
            operation.reply(gio::MountOperationResult::Handled);
        },
    );

    run(
        context,
        file.mount_enclosing_volume_future(gio::MountMountFlags::NONE, Some(&op)),
    );

    todo!();
}

pub fn gio_actor(
    mut commands: UnboundedReceiver<GioActorCommand>,
    startup_reply: SyncSender<Arc<MainContext>>,
    config: SessionHelperConfig,
) {
    let context = Arc::new(MainContext::new());

    // send the MainContext to the caller, so they can wake us up when new work arrives
    startup_reply.send(Arc::clone(&context)).unwrap();

    loop {
        match commands.try_recv() {
            Ok(GioActorCommand::Umount {
                reply,
                mounted_share,
            }) => {
                umount(mounted_share, &context);
                reply.send(()).unwrap(); // do we care about this error?
            }
            Ok(GioActorCommand::Shutdown {}) => break,
            Ok(GioActorCommand::Mount { reply, share }) => todo!(),
            Ok(GioActorCommand::ListMounts { reply }) => todo!(),
            Err(TryRecvError::Empty) => {
                context.iteration(true);
            }
            Err(TryRecvError::Disconnected) => {
                break;
            }
        }
        while context.pending() {
            context.iteration(false);
        }
    }
}
