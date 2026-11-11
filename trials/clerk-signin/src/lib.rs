//! Trial 3: Clerk sign-in from a native client.
//!
//! - [`verify`] is the server-side check that will move into `dirt-api`.
//! - [`flow`] is the client side: discovery, PKCE and the loopback callback.

pub mod flow;
pub mod verify;
