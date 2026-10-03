//! faith_move: first-person parkour movement in the spirit of Mirror's Edge.
//!
//! Engine-agnostic: feed it input and a [`World`] of boxes, read back the
//! player position, state, events and a camera [`View`].

pub mod camera;
pub mod controller;
pub mod greybox;
pub mod locomotion;
pub mod look;
pub mod moves;
pub mod rooftops;
pub mod springboard;
pub mod tuning;
pub mod vault;
pub mod world;

pub use camera::{CameraFx, CameraFxSettings, Shot, SpeedBlur};
pub use look::{LookLimiter, SwanNeck};
pub use controller::{AgainstWall, Controller, Event, Input, Melee, MeleeKind, Shimmy, State, Traverse, TraverseKind, TurnCurves, TurnKind, View};
pub use tuning::Tuning;
pub use vault::{Vault, VaultType, VAULT_TYPES};
pub use world::{Aabb, Body, BoxWorld, Fixture, MeshWorld, SweepHit, World};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod course_tests;
#[cfg(test)]
mod rooftops_tests;
#[cfg(test)]
mod moves_tests;
#[cfg(test)]
mod springboard_tests;
#[cfg(test)]
mod mesh_tests;
#[cfg(test)]
mod rough_shapes_tests;
