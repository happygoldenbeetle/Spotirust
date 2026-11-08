//! One HTTP agent for every Spotify request. It must send an honest User-Agent:
//! Spotify rejects ureq's default (`ureq/2.x`) on private endpoints with 403
//! "Client/request not allowed". Never impersonate Spotify's own clients here.

use std::sync::OnceLock;

pub const USER_AGENT: &str = concat!("Spotirust/", env!("CARGO_PKG_VERSION"));

pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| ureq::AgentBuilder::new().user_agent(USER_AGENT).build())
}
