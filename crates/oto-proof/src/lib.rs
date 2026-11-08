//! oto-proof: local proofs (v1) and the web-proof API contract (v1.1, no server in v1).
#![forbid(unsafe_code)]
// Render and layout helpers take many plain geometric parameters by design (see DECISIONS.md).
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod api;
pub mod local;
