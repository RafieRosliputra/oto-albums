//! oto-core: domain model, units, geometry, templates, layout and crop solvers, commands.
#![forbid(unsafe_code)]

pub mod builtin;
pub mod crop;
pub mod document;
pub mod expr;
pub mod geometry;
pub mod lab;
pub mod media;
pub mod model;
pub mod solver;
pub mod spec;
pub mod template;
pub mod units;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
