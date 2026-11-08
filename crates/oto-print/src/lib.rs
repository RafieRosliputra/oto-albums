//! oto-print: preflight, PDF/X-4 export, raster export, spec sheets, lab profiles, validation.
#![forbid(unsafe_code)]
// Render and layout helpers take many plain geometric parameters by design (see DECISIONS.md).
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod compose;
pub mod export;
pub mod labs;
pub mod pdf;
pub mod preflight;
pub mod validate;
