//! The "Springboard" map: a small test range for TdMove_SpringBoard. Five
//! lanes side by side, all running toward -Z from the same start line. Keys
//! 1–5 put you at the start of a lane, and R takes you back to it.
//!
//! A springboard in Mirror's Edge takes two things in a row: a low step
//! (64 cm, give or take 20) and, with its front 112 cm (±20) behind the
//! step's, something 80–148 cm high. Jump while you'd reach the step within a
//! second (up to ~7 m away at a sprint): Faith runs in, plants a foot on the
//! step, then on the top, and launches.
//!
//! 1. Up: 64 cm step, 1.2 m block, then a deck 3.5 m up, too high to jump to.
//! 2. Lowest: 50 cm step, 85 cm block, onto a 3 m deck.
//! 3. Highest: 80 cm step, 1.45 m block, onto a 3.7 m deck.
//! 4. Pit: 64 cm step, 1.2 m block, then across a 6 m pit.
//! 5. Not springboards, in grey: a thin rail you vault, a 1.2 m block with no
//!    step (you climb onto it), and a deep one you mantle onto and stand on.

use glam::Vec3;

use crate::greybox::{Level, Look};

/// Where each lane runs, along X.
pub const LANE_X: [f32; 5] = [-12.0, -6.0, 0.0, 6.0, 12.0];
/// Start line for every lane.
pub const START_Z: f32 = 20.0;
/// The front face of each lane's tall block.
pub const BLOCK_Z: f32 = 0.0;
/// The front face of each lane's step: 112 cm before the block's.
pub const STEP_Z: f32 = BLOCK_Z + 1.12;
pub const BLOCK_DEPTH: f32 = 1.5;

pub const UP_STEP: f32 = 0.64;
pub const UP_BLOCK: f32 = 1.2;
pub const UP_DECK: f32 = 3.5;
pub const LOW_STEP: f32 = 0.5;
pub const LOW_BLOCK: f32 = 0.85;
pub const LOW_DECK: f32 = 3.0;
pub const HIGH_STEP: f32 = 0.8;
pub const HIGH_BLOCK: f32 = 1.45;
pub const HIGH_DECK: f32 = 3.7;
pub const PIT_STEP: f32 = 0.64;
pub const PIT_BLOCK: f32 = 1.2;
pub const PIT_FLOOR: f32 = -4.0;

pub mod z {
    /// Where the decks (lanes 1–3) start: a 2.5 m gap behind each block.
    pub const DECK_START: f32 = -4.0;
    pub const DECK_END: f32 = -16.0;
    /// Lane 4's pit, from the back of its block.
    pub const PIT_START: f32 = -1.5;
    pub const PIT_END: f32 = -7.5;
    /// Lane 5's three near misses.
    pub const RAIL: f32 = 0.0;
    pub const LONE: f32 = -8.0;
    pub const DEEP: f32 = -16.0;
    /// Back wall.
    pub const END: f32 = -24.0;
}

pub fn springboard() -> Level {
    use Look::*;
    let mut l = Level { name: "Springboard", ..Level::default() };
    let lane = |i: usize| (LANE_X[i] - 2.0, LANE_X[i] + 2.0);

    // ---- the range: one floor, walled in --------------------------------
    // Everywhere except lane 4's pit.
    let (px0, px1) = lane(3);
    l.add(Roof, [-16.0, -1.0, z::PIT_START], [16.0, 0.0, 24.0]);
    l.add(Roof, [-16.0, -1.0, z::END], [16.0, 0.0, z::PIT_END]);
    l.add(Roof, [-16.0, -1.0, z::PIT_END], [px0, 0.0, z::PIT_START]);
    l.add(Roof, [px1, -1.0, z::PIT_END], [16.0, 0.0, z::PIT_START]);
    l.add(Roof, [px0, PIT_FLOOR - 1.0, z::PIT_END], [px1, PIT_FLOOR, z::PIT_START]);
    // Pit walls (it's a pit, not a hole in a thin slab).
    l.add(Wall, [px0, PIT_FLOOR, z::PIT_END], [px0 + 0.01, 0.0, z::PIT_START]);
    l.add(Wall, [px1 - 0.01, PIT_FLOOR, z::PIT_END], [px1, 0.0, z::PIT_START]);
    l.add(Wall, [px0, PIT_FLOOR, z::PIT_START - 0.01], [px1, -1.0, z::PIT_START]);
    l.add(Wall, [px0, PIT_FLOOR, z::PIT_END], [px1, -1.0, z::PIT_END + 0.01]);
    for (a, b) in [
        ([-16.5, 0.0, z::END], [-16.0, 3.0, 24.0]),
        ([16.0, 0.0, z::END], [16.5, 3.0, 24.0]),
        ([-16.0, 0.0, 24.0], [16.0, 3.0, 24.5]),
        ([-16.0, 0.0, z::END - 0.5], [16.0, 3.0, z::END]),
    ] {
        l.add(Wall, a, b);
    }

    // ---- lanes 1–4: step + block in red, then somewhere it takes you -----
    let springboards = [(0, UP_STEP, UP_BLOCK), (1, LOW_STEP, LOW_BLOCK), (2, HIGH_STEP, HIGH_BLOCK), (3, PIT_STEP, PIT_BLOCK)];
    for (i, step, block) in springboards {
        let (x0, x1) = lane(i);
        l.add(Runner, [x0, 0.0, BLOCK_Z - BLOCK_DEPTH], [x1, block, BLOCK_Z]);
        l.add(Runner, [x0, 0.0, BLOCK_Z], [x1, step, STEP_Z]);
        // A painted run-up stripe: jump anywhere on it (or hold jump) at a sprint.
        l.paint(Runner, [x0 + 1.0, -0.02, STEP_Z], [x1 - 1.0, 0.004, STEP_Z + 7.0]);
    }
    for (i, deck) in [(0, UP_DECK), (1, LOW_DECK), (2, HIGH_DECK)] {
        let (x0, x1) = lane(i);
        l.add(Roof, [x0 - 0.5, 0.0, z::DECK_END], [x1 + 0.5, deck, z::DECK_START]);
        // A red lip on the deck's edge, so you can see what you're aiming for.
        l.paint(Runner, [x0 - 0.5, deck - 0.25, z::DECK_START + 0.004], [x1 + 0.5, deck, z::DECK_START + 0.01]);
    }

    // ---- lane 5: near misses, in grey -------------------------------------
    let (x0, x1) = lane(4);
    // Waist-high and thin: vault it.
    l.add(Wall, [x0, 0.0, z::RAIL - 0.4], [x1, 0.9, z::RAIL]);
    // Springboard height, but no step in front: you just climb onto it.
    l.add(Wall, [x0, 0.0, z::LONE - BLOCK_DEPTH], [x1, 1.2, z::LONE]);
    // Same again, 3.5 m deep: mantle on and stand on it.
    l.add(Wall, [x0, 0.0, z::DEEP - 3.5], [x1, 1.2, z::DEEP]);

    // ---- skyline, so it's not floating in a void ---------------------------
    l.add(Skyline, [-200.0, -31.0, -300.0], [200.0, -30.0, 200.0]);
    for (a, b) in [
        ([-60.0, -30.0, -20.0], [-30.0, 30.0, 20.0]),
        ([30.0, -30.0, -40.0], [60.0, 40.0, 10.0]),
        ([-40.0, -30.0, -90.0], [40.0, 50.0, -60.0]),
    ] {
        let (a, b) = (Vec3::from(a), Vec3::from(b));
        l.add(Skyline, a.min(b).to_array(), a.max(b).to_array());
    }

    // ---- checkpoints: one per lane --------------------------------------------
    // The first one's region is the whole range, so walking between lanes never
    // changes your checkpoint (and the time trial never starts: there's no finish).
    // Pick a lane with 1–5; R puts you back at its start.
    let names = ["1 Up onto the deck", "2 Lowest", "3 Highest", "4 Over the pit", "5 Not springboards"];
    for (i, name) in names.into_iter().enumerate() {
        let x = LANE_X[i];
        let (min, max) = if i == 0 { ([-17.0, -10.0, -25.0], [17.0, 20.0, 25.0]) } else { ([x - 2.0, 0.0, 19.0], [x + 2.0, 2.0, 21.0]) };
        l.checkpoint(name, min, max, [x, 0.0, START_Z]);
    }
    l
}
