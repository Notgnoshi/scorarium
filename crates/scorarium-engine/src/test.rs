use scorarium_client::fake::FakeTransport;
use scorarium_client::{Client, UserAgent};

use crate::EngineOptions;

/// Get a test configuration of [EngineOptions] that replays recorded API responses instead of
/// making real requests.
pub(crate) fn options() -> EngineOptions {
    let user_agent = UserAgent::new(Some("Notgnoshi@gmail.com"));
    let transport = FakeTransport::new(&user_agent).expect("failed to open API fixtures");
    let client = Client::builder(user_agent.clone())
        .transport(transport)
        .build()
        .unwrap();
    EngineOptions::new(user_agent).unwrap().client(client)
}
