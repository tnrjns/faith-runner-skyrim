//! Mirror's Edge's first-person body for faith_move, engine independent:
//! - [`Driver`] picks and blends the game's own animations from the movement state;
//! - [`Rig`] places the body and the camera each frame, the way the game does;
//! - [`sound`] decides which of Mirror's Edge's sounds play when.
//!
//! The animations are read from the player's own Mirror's Edge install (`me_assets`).

pub mod driver;
#[cfg(feature = "retarget")]
pub mod retarget;
pub mod rig;
pub mod sound;

pub use driver::{BodyAnim, Driver};
#[cfg(feature = "retarget")]
pub use retarget::{BoneRest, HostFrame, Placement, Retarget, Xform};
pub use rig::{Rig, RigFrame};

#[cfg(test)]
mod tests;
#[cfg(all(test, feature = "retarget"))]
mod retarget_tests;
