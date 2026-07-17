//! Ship status objects for each map type.
//!
//! Each map has its own ship status struct that wraps the base
//! `InnerShipStatus` and provides map-specific system definitions.

pub mod ship_status;
pub mod skeld;
pub mod mira;
pub mod polus;
pub mod dleks;
pub mod airship;
pub mod fungle;

pub use ship_status::{InnerShipStatus, ShipSystem, SystemTypes};
pub use skeld::InnerSkeldShipStatus;
pub use mira::InnerMiraShipStatus;
pub use polus::InnerPolusShipStatus;
pub use dleks::InnerDleksShipStatus;
pub use airship::InnerAirshipStatus;
pub use fungle::InnerFungleShipStatus;
