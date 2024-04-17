use modama_agent::platform::{find_free_drive, get_mapped_drives, perform_net_use, NetUseParams};
use widestring::U16CString;

fn main() {
    let drive = find_free_drive().unwrap();
    eprintln!("mounting at {drive}");

    let net_use = NetUseParams {
        remote_name: U16CString::from_str(
            "\\\\moellenstedt.iff.kfa-juelich.de\\share-c6cda885-8e95-4919-a2b8-6e9e3909aa64",
        )
        .unwrap(),
        local_name: U16CString::from_str(&drive).unwrap(),
        domain: U16CString::from_str("ER-C-INGEST").unwrap(),
        username: U16CString::from_str("sharef76b764d16a043ea805918305ce").unwrap(),
        password: U16CString::from_str("kubeFWCBMvw1AVTADZsM4wEXxECwBxgAAo8_9cAZ_5U").unwrap(),
    };

    get_mapped_drives().unwrap();

    if let Err(e) = perform_net_use(net_use) {
        panic!("error `net use`ing: {}", e.message());
    }

    get_mapped_drives().unwrap();
}
