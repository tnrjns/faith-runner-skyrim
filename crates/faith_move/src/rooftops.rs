//! "Rooftops": a longer run across a block of buildings, with choices.
//!
//! Runs toward −Z. Every roof is a building rising from the street 30 m
//! below (falling off is fatal and respawns you at the last checkpoint).
//!
//! ```text
//!  R1 Start (y 0)      AC units + rail, skylight, pipe   gap 4 m, drop 1.5 m
//!  R2 Billboards (-1.5) low wall | rail, then the billboard: wallrun over an 8 m gap
//!                       (or balance across the girder on the left)
//!  R3 Tanks (-1.5)     water tank, then climb the 3.3 m wall (or crate + grab on the left)
//!  R4 Upper (1.8)      AC slalom, then a 12 m girder over the drop
//!  R5 Ledge (1.8)      6 m drop: roll it
//!  R6 Plaza (-4.2)     slide the duct, hop the vents, climb to the finish roof
//!  R7 Finish (-0.9)    the orange marker
//! ```
//!
//! Routes are checked by `course_tests.rs`.

use glam::Vec3;

use crate::greybox::{Level, Look};
use crate::world::Aabb;

pub const STREET_Y: f32 = -30.0;
pub const R1_Y: f32 = 0.0;
pub const R2_Y: f32 = -1.5;
pub const R3_Y: f32 = -1.5;
pub const R4_Y: f32 = 1.8;
pub const R6_Y: f32 = -4.2;
pub const R7_Y: f32 = -0.9;

/// Key z positions (the run goes toward −Z).
pub mod z {
    pub const R1_END: f32 = -28.0;
    pub const RAIL: f32 = -6.0;
    pub const SKYLIGHT: f32 = -14.0;
    pub const PIPE: f32 = -22.0;
    pub const R2_START: f32 = -32.0;
    pub const LOW_WALL: f32 = -40.0;
    pub const R2_END: f32 = -58.0;
    pub const R3_START: f32 = -66.0;
    pub const CLIMB: f32 = -92.0;
    pub const SLALOM_A: f32 = -104.0;
    pub const SLALOM_B: f32 = -110.0;
    pub const R4_END: f32 = -118.0;
    pub const R5_START: f32 = -130.0;
    pub const R5_END: f32 = -142.0;
    pub const DUCT: f32 = -152.0;
    pub const VENT_A: f32 = -158.0;
    pub const VENT_B: f32 = -163.0;
    pub const FINISH_WALL: f32 = -172.0;
    pub const FINISH: f32 = -186.0;
}

/// x of the billboard face you wallrun along (right side).
pub const BILLBOARD_X: f32 = 8.0;

pub fn rooftops() -> Level {
    use Look::*;
    let mut l = Level { name: "Rooftops", ..Level::default() };
    let building = |l: &mut Level, x0: f32, x1: f32, z0: f32, z1: f32, top: f32| {
        l.add(Roof, [x0, STREET_Y, z1.min(z0)], [x1, top, z0.max(z1)]);
    };
    let parapets = |l: &mut Level, x0: f32, x1: f32, z0: f32, z1: f32, top: f32| {
        l.add(Wall, [x0, top, z1], [x0 + 0.3, top + 0.9, z0]);
        l.add(Wall, [x1 - 0.3, top, z1], [x1, top + 0.9, z0]);
    };

    // ---- R1: start ------------------------------------------------------
    building(&mut l, -8.0, 8.0, 6.0, z::R1_END, R1_Y);
    parapets(&mut l, -8.0, 8.0, 6.0, z::R1_END, R1_Y);
    l.add(Wall, [-8.0, R1_Y, 5.5], [8.0, R1_Y + 3.5, 6.0]);
    // AC units with a rail between them: vault the rail.
    l.add(Prop, [-7.7, R1_Y, z::RAIL - 1.0], [-2.0, R1_Y + 1.7, z::RAIL + 1.0]);
    l.add(Prop, [2.0, R1_Y, z::RAIL - 1.0], [7.7, R1_Y + 1.7, z::RAIL + 1.0]);
    l.add(Runner, [-2.0, R1_Y, z::RAIL - 0.12], [2.0, R1_Y + 1.0, z::RAIL + 0.12]);
    // Skylight frame: mantle onto it and run across, or go round.
    l.add(Runner, [-3.5, R1_Y, z::SKYLIGHT - 3.0], [3.5, R1_Y + 1.1, z::SKYLIGHT]);
    l.add(Prop, [-3.3, R1_Y + 1.1, z::SKYLIGHT - 2.8], [3.3, R1_Y + 1.15, z::SKYLIGHT - 0.2]);
    // Pipe across the roof: slide under it (or coil-jump over).
    l.add(Runner, [-7.7, R1_Y + 1.3, z::PIPE - 0.35], [7.7, R1_Y + 1.6, z::PIPE]);
    l.add(Prop, [-7.7, R1_Y, z::PIPE - 0.35], [-7.2, R1_Y + 1.3, z::PIPE]);
    l.add(Prop, [7.2, R1_Y, z::PIPE - 0.35], [7.7, R1_Y + 1.3, z::PIPE]);
    l.paint(Runner, [-7.7, R1_Y - 0.02, z::R1_END], [7.7, R1_Y + 0.004, z::R1_END + 0.4]);

    // ---- R2: billboards -------------------------------------------------
    building(&mut l, -8.0, BILLBOARD_X, z::R2_START, z::R2_END, R2_Y);
    l.add(Wall, [-8.0, R2_Y, z::R2_END], [-7.7, R2_Y + 0.9, z::R2_START]);
    // Low wall on the left (vault), open on the right.
    l.add(Runner, [-7.7, R2_Y, z::LOW_WALL - 0.4], [0.0, R2_Y + 1.05, z::LOW_WALL]);
    // Billboard on the right, spanning the gap: wallrun across.
    l.add(Runner, [BILLBOARD_X, -12.0, -72.0], [BILLBOARD_X + 0.4, R2_Y + 4.5, -46.0]);
    l.add(Wall, [BILLBOARD_X + 0.4, -12.0, -71.0], [BILLBOARD_X + 0.6, R2_Y + 6.5, -47.0]);
    l.paint(Runner, [3.0, R2_Y - 0.02, -50.4], [BILLBOARD_X, R2_Y + 0.004, -50.0]);
    // Girder across the gap on the left: walk it.
    l.add(Runner, [-6.4, R2_Y - 0.4, z::R3_START - 0.5], [-5.6, R2_Y, z::R2_END + 0.5]);

    // ---- R3: tanks --------------------------------------------------------
    building(&mut l, -8.0, BILLBOARD_X, z::R3_START, z::CLIMB, R3_Y);
    l.add(Wall, [-8.0, R3_Y, z::CLIMB], [-7.7, R3_Y + 0.9, z::R3_START]);
    // Water tank (scenery you go round) on stilts-high base.
    l.add(Prop, [3.5, R3_Y, -82.0], [7.5, R3_Y + 3.2, -74.0]);
    // Crate against the climb wall on the left: mantle it, then grab the roof.
    l.add(Prop, [-7.7, R3_Y, z::CLIMB], [-4.5, R3_Y + 1.3, z::CLIMB + 3.0]);

    // ---- R4: upper roof (its front face is the climb wall) ---------------
    building(&mut l, -8.0, 8.0, z::CLIMB, z::R4_END, R4_Y);
    l.paint(Runner, [-2.0, R4_Y - 0.25, z::CLIMB - 0.01], [2.0, R4_Y + 0.004, z::CLIMB + 0.004]);
    parapets(&mut l, -8.0, 8.0, z::CLIMB, z::R4_END, R4_Y);
    // AC slalom: boxes alternate sides, a lane stays open down the middle.
    l.add(Prop, [-7.7, R4_Y, z::SLALOM_A - 0.8], [-1.1, R4_Y + 0.9, z::SLALOM_A]);
    l.add(Prop, [1.1, R4_Y, z::SLALOM_B - 0.8], [7.7, R4_Y + 0.9, z::SLALOM_B]);
    // Girder over the drop to R5.
    l.add(Runner, [-0.4, R4_Y - 0.5, z::R5_START], [0.4, R4_Y, z::R4_END]);

    // ---- R5: ledge, then the drop ------------------------------------
    building(&mut l, -8.0, 8.0, z::R5_START, z::R5_END, R4_Y);
    parapets(&mut l, -8.0, 8.0, z::R5_START, z::R5_END, R4_Y);
    l.paint(Runner, [-7.7, R4_Y - 0.02, z::R5_END], [7.7, R4_Y + 0.004, z::R5_END + 0.4]);

    // ---- R6: plaza roof ------------------------------------------------
    building(&mut l, -12.0, 12.0, z::R5_END, z::FINISH_WALL, R6_Y);
    // Duct: slide under (solid above it, so no going over).
    l.add(Runner, [-12.0, R6_Y + 1.3, z::DUCT - 0.5], [12.0, R6_Y + 1.6, z::DUCT]);
    l.add(Prop, [-12.0, R6_Y + 1.6, z::DUCT - 0.5], [12.0, R6_Y + 3.5, z::DUCT]);
    // Vents to hop over.
    l.add(Runner, [-4.0, R6_Y, z::VENT_A - 0.5], [4.0, R6_Y + 0.8, z::VENT_A]);
    l.add(Runner, [-4.0, R6_Y, z::VENT_B - 0.5], [4.0, R6_Y + 0.9, z::VENT_B]);
    // Side walls of the plaza.
    l.add(Wall, [-12.0, R6_Y, z::FINISH_WALL], [-11.7, R6_Y + 1.0, z::R5_END]);
    l.add(Wall, [11.7, R6_Y, z::FINISH_WALL], [12.0, R6_Y + 1.0, z::R5_END]);

    // ---- R7: finish roof (its front face is the 3.3 m climb) -------------
    building(&mut l, -12.0, 12.0, z::FINISH_WALL, -200.0, R7_Y);
    l.paint(Runner, [-3.0, R7_Y - 0.25, z::FINISH_WALL - 0.01], [3.0, R7_Y + 0.004, z::FINISH_WALL + 0.004]);
    l.add(Finish, [-0.5, R7_Y, z::FINISH - 0.5], [0.5, R7_Y + 3.0, z::FINISH + 0.5]);
    l.finish = Some(Aabb::new(
        Vec3::new(-6.0, R7_Y - 0.5, z::FINISH - 2.0),
        Vec3::new(6.0, R7_Y + 3.0, z::FINISH + 2.0),
    ));

    // ---- city around it ---------------------------------------------------
    l.add(Skyline, [-200.0, STREET_Y - 1.0, -320.0], [200.0, STREET_Y, 120.0]);
    let towers = [
        ([-34.0, -24.0, 10.0], [-14.0, 40.0, -20.0]),
        ([-40.0, -24.0, -40.0], [-16.0, 26.0, -80.0]),
        ([-38.0, -24.0, -100.0], [-18.0, 55.0, -140.0]),
        ([-44.0, -24.0, -160.0], [-20.0, 30.0, -210.0]),
        ([16.0, -24.0, 8.0], [34.0, 22.0, -30.0]),
        ([20.0, -24.0, -50.0], [42.0, 48.0, -95.0]),
        ([18.0, -24.0, -110.0], [40.0, 34.0, -150.0]),
        ([22.0, -24.0, -170.0], [48.0, 62.0, -215.0]),
        ([-20.0, -24.0, -230.0], [24.0, 70.0, -260.0]),
        ([-30.0, -24.0, 40.0], [30.0, 18.0, 60.0]),
    ];
    for (a, b) in towers {
        let (a, b) = (Vec3::from(a), Vec3::from(b));
        l.add(Skyline, a.min(b).to_array(), a.max(b).to_array());
    }

    // ---- checkpoints -------------------------------------------------------
    l.checkpoint("R1 Start", [-8.0, -1.0, -4.0], [8.0, 3.0, 5.5], [0.0, R1_Y, 3.0]);
    l.checkpoint("R2 Billboards", [-8.0, R2_Y - 1.0, z::R2_END], [BILLBOARD_X, R2_Y + 3.0, z::R2_START], [0.0, R2_Y, -34.0]);
    l.checkpoint("R3 Tanks", [-8.0, R3_Y - 1.0, z::CLIMB], [BILLBOARD_X, R3_Y + 3.0, z::R3_START], [0.0, R3_Y, -70.0]);
    l.checkpoint("R4 Upper", [-8.0, R4_Y - 1.0, z::R4_END], [8.0, R4_Y + 3.0, z::CLIMB], [0.0, R4_Y, -95.0]);
    l.checkpoint("R6 Plaza", [-12.0, R6_Y - 1.0, z::FINISH_WALL], [12.0, R6_Y + 3.0, z::R5_END], [0.0, R6_Y, -146.0]);
    l.checkpoint("R7 Finish", [-12.0, R7_Y - 1.0, -200.0], [12.0, R7_Y + 4.0, z::FINISH_WALL], [0.0, R7_Y, -175.0]);
    l
}
