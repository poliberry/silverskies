//! Silver Skies domain core — pure, dependency-light, fully unit-testable.
//!
//! Nothing in here does I/O. Networking lives in `ss-net`, persistence in
//! `ss-store`, audio in `ss-audio`, map rendering in `ss-map`, and the egui
//! application in the `silverskies` binary.

pub mod ai;
pub mod alerts;
pub mod cells;
pub mod command;
pub mod geo;
pub mod opera;
pub mod placefile;
pub mod radar3d;
pub mod same;
pub mod sounding;
pub mod storms;
pub mod theme;
pub mod types;
pub mod version;
pub mod weather;

pub use types::*;
