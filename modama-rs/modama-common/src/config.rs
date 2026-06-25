use serde::{Deserialize, Serialize};

pub fn default_event_url() -> String {
    "wss://localhost:5052".to_owned()
}

pub fn default_token_service_url() -> String {
    "https://localhost:5053".to_owned()
}

pub fn default_mgmt_api_url() -> String {
    "http://localhost:5001".to_owned()
}

pub fn default_smb_host() -> String {
    "moellenstedt.iff.kfa-juelich.de".to_owned()
}

pub fn default_smb_port() -> u32 {
    445
}

pub fn default_smb_domain() -> String {
    "ER-C-INGEST".to_owned()
}

pub fn default_listen_local_api() -> String {
    "localhost:3003".to_owned()
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AgentConfigFileServices {
    #[serde(default = "default_event_url")]
    pub event_url: String,

    #[serde(default = "default_token_service_url")]
    pub token_service_url: String,

    #[serde(default = "default_mgmt_api_url")]
    pub mgmt_api_url: String,

    #[serde(default = "default_smb_host")]
    pub smb_hostname: String,

    #[serde(default = "default_smb_port")]
    pub smb_port: u32,

    #[serde(default = "default_smb_domain")]
    pub smb_domain: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfigFileCa {
    pub ca_cert: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfigFileClientCert {
    pub chain: String,
    pub private_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfigFile {
    pub services: AgentConfigFileServices,

    #[serde(default = "default_listen_local_api")]
    pub listen_local_api: String,

    pub ca: AgentConfigFileCa,

    pub client_cert: Option<AgentConfigFileClientCert>,
}
