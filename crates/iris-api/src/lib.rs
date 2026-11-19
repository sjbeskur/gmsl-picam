//! Compatibility exports. New protocol code should depend on photon-protocol.
pub use photon_protocol::*;
/// Browser gateway protection against simple cross-origin mutations; not authentication.
pub const CONTROL_HEADER: &str = "x-iris-control";
