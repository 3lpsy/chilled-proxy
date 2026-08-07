//! Inbound authentication. The identity middleware only attaches identity;
//! rejection is the per-tier guards' job, always a JSON 401, never a redirect.

mod identity;
pub(crate) mod password;
pub(crate) mod session;
#[cfg(test)]
mod tests;

pub(crate) use identity::{identity, require, require_auth, require_read, MaybeIdentity};
pub(crate) use session::{create_session, forwarded_https};
