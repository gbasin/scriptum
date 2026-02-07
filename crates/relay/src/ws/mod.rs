mod handler;
pub mod protocol;
mod session;

pub(crate) use handler::router;
#[cfg(test)]
pub(crate) use session::CreateSyncSessionResponse;
pub(crate) use session::{DocSyncStore, SyncSessionStore, WorkspaceMembershipStore};

#[cfg(test)]
pub(crate) use handler::{
    handle_awareness_update, handle_hello_message, handle_subscribe_message,
    handle_yjs_update_message,
};
#[cfg(test)]
pub(crate) use session::{
    SessionTokenValidation, HEARTBEAT_INTERVAL_MS, HEARTBEAT_TIMEOUT_MS, MAX_FRAME_BYTES,
};

#[cfg(test)]
mod tests;
