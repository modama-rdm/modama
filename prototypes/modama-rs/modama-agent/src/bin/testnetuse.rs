use modama_agent::platform::{perform_net_use, perform_net_use_alt, NetUseParams};
use widestring::U16CString;

fn main() {
    let remote_name = U16CString::from_str(
        "\\\\moellenstedt.iff.kfa-juelich.de\\share-c6cda885-8e95-4919-a2b8-6e9e3909aa64",
    )
    .unwrap();
    let local_name = U16CString::from_str("Y:").unwrap();
    let username = U16CString::from_str("sharef76b764d16a043ea805918305ce").unwrap();
    let domain = U16CString::from_str("ER-C-INGEST").unwrap();
    let password = U16CString::from_str("kubeFWCBMvw1AVTADZsM4wEXxECwBxgAAo8_9cAZ_5U").unwrap();

    perform_net_use_alt(NetUseParams {
        remote_name,
        local_name,
        domain,
        username,
        password,
    }).unwrap();
}
