use modama_common::internal_api_types::{SessionHelperConfig, Shares};

#[derive(Debug, Clone)]
pub enum HelperConnectionState {
    Connecting,
    Connected,
}

#[derive(Debug, Clone)]
pub struct HelperState {
    pub desired_state: Option<Shares>,
    pub config: Option<SessionHelperConfig>,
    pub connection_state: HelperConnectionState,
}
