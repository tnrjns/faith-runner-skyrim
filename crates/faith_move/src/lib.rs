//! faith_move: first-person parkour movement in the spirit of Mirror's Edge.
//!
//! Engine-agnostic: feed it input and a [`World`] of boxes, read back the
//! player position, state, events and a camera [`View`].

pub mod camera;
pub mod climb;
pub mod controller;
pub mod fixtures;
pub mod grabtransfer;
pub mod greybox;
pub mod locomotion;
pub mod melee;
pub mod look;
pub mod moves;
pub mod rooftops;
pub mod reaction;
pub mod rumpslide;
pub mod springboard;
pub mod stepup;
pub mod swingjump;
pub mod takedown;
pub mod airbarge;
pub mod tuning;
pub mod vault;
pub mod vertigo;
pub mod world;

pub use camera::{CameraFx, CameraFxSettings, Shot, SpeedBlur};
pub use look::{LookLimiter, SwanNeck};
pub use controller::{AgainstWall, ClimbStart, ClimbStep, Ladder, Controller, Event, Input, Melee, MeleeKind, MeleePhase, Shimmy, State, Traverse, TraverseKind, TurnCurves, TurnKind, View};
pub use melee::Target;
pub use takedown::TAKEDOWN_ANIMS;
pub use tuning::{MeleeClips, Tuning};
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
#[cfg(test)]
mod melee_tests;
