use log::info;
use modama_agent::platform::internal::{
    find_free_drive, get_mapped_drives, net_use_delete, perform_net_use, NetUseParams,
};
use modama_common::logging::init_logging;
use widestring::U16CString;

fn main() {
    init_logging();

    let drive = find_free_drive().unwrap();
    info!("mounting at {drive}");

    let net_use = NetUseParams {
        remote_name: U16CString::from_str_truncate(
            "\\\\moellenstedt.iff.kfa-juelich.de\\share-c6cda885-8e95-4919-a2b8-6e9e3909aa64",
        ),
        local_name: U16CString::from_str_truncate(&drive),
        domain: U16CString::from_str_truncate("ER-C-INGEST"),
        username: U16CString::from_str_truncate("sharef76b764d16a043ea805918305ce"),
        password: U16CString::from_str_truncate("kubeFWCBMvw1AVTADZsM4wEXxECwBxgAAo8_9cAZ_5U"),
    };

    get_mapped_drives().unwrap();

    if let Err(e) = perform_net_use(net_use) {
        panic!("error `net use`ing: {}", e.message());
    }

    info!("{:#?}", get_mapped_drives().unwrap());

    net_use_delete(&drive, false).unwrap();

    info!("{:#?}", get_mapped_drives().unwrap());
}
