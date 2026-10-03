//! The "Moves" map: a short course built around Mirror's Edge's special
//! moves, one after another, running toward -Z:
//!
//! 1. M1: sprint at the low step in front of a chest-high block and
//!    **springboard** off them up onto a roof too high to jump to.
//! 2. M2: a **balance beam** over a drop (left/right to keep your balance).
//! 3. M3: jump to a **swing pole**, swing, and jump off across the next gap.
//! 4. M4: jump up to a **zipline** and ride it down to the last roof.
//! 5. M5: the finish (and room to try **kicks and punches**: F / right mouse). A wall across
//!    the roof with a **door**: kick it (F) standing, or run at it and barge through (F).
//!    Mind the **barbed wire** on the right: it trips you.
//!
//! Off the right side of M3, 5.5 m down, a lower roof with a **mattress**: drop onto it for a
//! soft landing instead of a hard one.

use glam::Vec3;

use crate::greybox::{Level, Look};
use crate::world::{Aabb, Fixture};

pub const STREET_Y: f32 = -30.0;
pub const M1_Y: f32 = 0.0;
pub const M2_Y: f32 = 3.5;
pub const M3_Y: f32 = 3.5;
pub const M4_Y: f32 = 2.0;
pub const M5_Y: f32 = -4.0;
/// Top of the springboard block.
pub const BLOCK_TOP: f32 = 1.2;
/// The low step in front of it (TdMove_SpringBoard: 64 cm, its front 112 cm before the block's).
pub const STEP_TOP: f32 = 0.64;
/// Swing bar height.
pub const POLE_Y: f32 = M3_Y + 2.9;

pub mod z {
    pub const STEP_START: f32 = -27.38;
    pub const BLOCK_START: f32 = -28.5;
    pub const M1_END: f32 = -30.0;
    pub const M2_START: f32 = -32.5;
    pub const M2_END: f32 = -60.0;
    pub const M3_START: f32 = -68.0;
    pub const M3_END: f32 = -84.0;
    pub const POLE: f32 = -86.5;
    pub const M4_START: f32 = -92.0;
    pub const M4_END: f32 = -106.0;
    pub const ZIP_START: f32 = -103.5;
    pub const ZIP_END: f32 = -140.0;
    pub const M5_START: f32 = -134.0;
    pub const M5_END: f32 = -165.0;
    pub const FINISH: f32 = -158.0;
    /// The wall with the door, across M5.
    pub const DOOR: f32 = -146.0;
    pub const WIRE: f32 = -142.5;
}

/// The soft-landing roof off M3's right side, and the mattress on it.
pub const SOFT_ROOF_Y: f32 = M3_Y - 6.0;
pub const PAD_TOP: f32 = SOFT_ROOF_Y + 0.5;

/// The zipline's two ends.
pub const ZIP_A: Vec3 = Vec3::new(0.0, M4_Y + 2.8, z::ZIP_START);
pub const ZIP_B: Vec3 = Vec3::new(0.0, M5_Y + 2.6, z::ZIP_END);

pub fn moves() -> Level {
    use Look::*;
    let mut l = Level { name: "Moves", ..Level::default() };
    let building = |l: &mut Level, x0: f32, x1: f32, z0: f32, z1: f32, top: f32| {
        l.add(Roof, [x0, STREET_Y, z1.min(z0)], [x1, top, z0.max(z1)]);
    };
    let sides = |l: &mut Level, x0: f32, x1: f32, z0: f32, z1: f32, top: f32| {
        l.add(Wall, [x0, top, z1.min(z0)], [x0 + 0.3, top + 0.9, z0.max(z1)]);
        l.add(Wall, [x1 - 0.3, top, z1.min(z0)], [x1, top + 0.9, z0.max(z1)]);
    };

    // ---- M1: run-up, then the springboard block at the edge -------------
    building(&mut l, -6.0, 6.0, 6.0, z::M1_END, M1_Y);
    sides(&mut l, -6.0, 6.0, 6.0, z::M1_END, M1_Y);
    l.add(Wall, [-6.0, M1_Y, 5.5], [6.0, M1_Y + 3.5, 6.0]);
    l.add(Runner, [-2.0, M1_Y, z::M1_END], [2.0, BLOCK_TOP, z::BLOCK_START]);
    l.add(Runner, [-2.0, M1_Y, z::BLOCK_START], [2.0, STEP_TOP, z::STEP_START]);
    l.paint(Runner, [-2.0, M1_Y - 0.02, z::STEP_START], [2.0, M1_Y + 0.004, z::STEP_START + 6.0]);
    // A lip under M2's edge to show it's out of reach from the ground.
    l.paint(Runner, [-6.0, M2_Y - 0.25, z::M2_START + 0.004], [6.0, M2_Y, z::M2_START + 0.01]);

    // ---- M2: landing roof, beam over the drop at the far end ------------
    building(&mut l, -6.0, 6.0, z::M2_START, z::M2_END, M2_Y);
    sides(&mut l, -6.0, 6.0, z::M2_START - 1.0, z::M2_END, M2_Y);
    l.add(Runner, [-0.12, M2_Y - 0.3, z::M3_START], [0.12, M2_Y, z::M2_END]);
    l.fixtures.push(Fixture::Beam { a: Vec3::new(0.0, M2_Y, z::M2_END), b: Vec3::new(0.0, M2_Y, z::M3_START) });
    // Walls either side of the beam's start so you have to take it.
    l.add(Wall, [-6.0, M2_Y, z::M2_END + 0.3], [-0.6, M2_Y + 1.2, z::M2_END]);
    l.add(Wall, [0.6, M2_Y, z::M2_END + 0.3], [6.0, M2_Y + 1.2, z::M2_END]);

    // ---- M3: swing pole over the gap to the lower M4 --------------------
    building(&mut l, -6.0, 6.0, z::M3_START, z::M3_END, M3_Y);
    sides(&mut l, -6.0, 6.0, z::M3_START, z::M3_END, M3_Y);
    l.paint(Runner, [-2.5, M3_Y - 0.02, z::M3_END + 0.4], [2.5, M3_Y + 0.004, z::M3_END]);
    l.fixtures.push(Fixture::SwingPole { a: Vec3::new(-2.5, POLE_Y, z::POLE), b: Vec3::new(2.5, POLE_Y, z::POLE) });
    // The bar's supports.
    l.add(Prop, [-2.8, STREET_Y, z::POLE - 0.15], [-2.5, POLE_Y + 0.1, z::POLE + 0.15]);
    l.add(Prop, [2.5, STREET_Y, z::POLE - 0.15], [2.8, POLE_Y + 0.1, z::POLE + 0.15]);

    // ---- M4: zipline start ----------------------------------------------
    building(&mut l, -6.0, 6.0, z::M4_START, z::M4_END, M4_Y);
    sides(&mut l, -6.0, 6.0, z::M4_START, z::M4_END, M4_Y);
    l.fixtures.push(Fixture::ZipLine { a: ZIP_A, b: ZIP_B });
    // Masts the cable hangs from (off to the side, with an arm over the line).
    l.add(Prop, [0.7, M4_Y, z::ZIP_START - 0.15], [1.0, ZIP_A.y + 0.5, z::ZIP_START + 0.15]);
    l.paint(Prop, [-0.1, ZIP_A.y + 0.3, z::ZIP_START - 0.08], [1.0, ZIP_A.y + 0.45, z::ZIP_START + 0.08]);
    l.add(Prop, [0.7, M5_Y, z::ZIP_END - 0.15], [1.0, ZIP_B.y + 0.5, z::ZIP_END + 0.15]);
    l.paint(Prop, [-0.1, ZIP_B.y + 0.3, z::ZIP_END - 0.08], [1.0, ZIP_B.y + 0.45, z::ZIP_END + 0.08]);

    // ---- M5: finish -------------------------------------------------------
    building(&mut l, -8.0, 8.0, z::M5_START, z::M5_END, M5_Y);
    sides(&mut l, -8.0, 8.0, z::M5_START, z::M5_END, M5_Y);
    l.add(Wall, [-8.0, M5_Y, z::M5_END + 0.3], [8.0, M5_Y + 3.0, z::M5_END]);
    l.add(Finish, [-0.5, M5_Y, z::FINISH - 0.5], [0.5, M5_Y + 3.0, z::FINISH + 0.5]);
    l.finish = Some(Aabb::new(
        Vec3::new(-6.0, M5_Y - 0.5, z::FINISH - 2.0),
        Vec3::new(6.0, M5_Y + 3.0, z::FINISH + 2.0),
    ));

    // ---- M5: a wall with a door to barge, barbed wire to the right --------
    l.add(Wall, [-8.0, M5_Y, z::DOOR - 0.3], [-0.7, M5_Y + 3.0, z::DOOR]);
    l.add(Wall, [0.7, M5_Y, z::DOOR - 0.3], [8.0, M5_Y + 3.0, z::DOOR]);
    l.add(Wall, [-0.7, M5_Y + 2.2, z::DOOR - 0.3], [0.7, M5_Y + 3.0, z::DOOR]);
    l.fixtures.push(Fixture::Door {
        b: Aabb::new(Vec3::new(-0.7, M5_Y, z::DOOR - 0.25), Vec3::new(0.7, M5_Y + 2.2, z::DOOR - 0.05)),
        n: Vec3::Z,
    });
    l.fixtures.push(Fixture::BarbedWire {
        b: Aabb::new(Vec3::new(2.0, M5_Y - 0.1, z::WIRE - 0.4), Vec3::new(6.0, M5_Y + 0.6, z::WIRE)),
    });

    // ---- the soft landing: a lower roof off M3's right side, with a mattress
    building(&mut l, 6.5, 14.0, z::M3_START, z::M3_END, SOFT_ROOF_Y);
    l.add(Prop, [8.0, SOFT_ROOF_Y, -80.0], [12.0, PAD_TOP, -72.0]);
    l.fixtures.push(Fixture::SoftPad { b: Aabb::new(Vec3::new(8.0, SOFT_ROOF_Y, -80.0), Vec3::new(12.0, PAD_TOP, -72.0)) });

    // ---- city -------------------------------------------------------------
    l.add(Skyline, [-200.0, STREET_Y - 1.0, -300.0], [200.0, STREET_Y, 120.0]);
    for (a, b) in [
        ([-30.0, -24.0, 0.0], [-12.0, 36.0, -40.0]),
        ([-36.0, -24.0, -60.0], [-14.0, 24.0, -110.0]),
        ([-34.0, -24.0, -130.0], [-16.0, 50.0, -180.0]),
        ([14.0, -24.0, 4.0], [32.0, 26.0, -50.0]),
        ([16.0, -24.0, -70.0], [40.0, 44.0, -120.0]),
        ([16.0, -24.0, -140.0], [42.0, 30.0, -190.0]),
        ([-24.0, -24.0, -200.0], [24.0, 60.0, -230.0]),
    ] {
        let (a, b) = (Vec3::from(a), Vec3::from(b));
        l.add(Skyline, a.min(b).to_array(), a.max(b).to_array());
    }

    // ---- checkpoints -------------------------------------------------------
    l.checkpoint("M1 Springboard", [-6.0, -1.0, -10.0], [6.0, 4.0, 5.5], [0.0, M1_Y, 3.0]);
    l.checkpoint("M2 Balance", [-6.0, M2_Y - 1.0, z::M2_END], [6.0, M2_Y + 3.0, z::M2_START], [0.0, M2_Y, -50.0]);
    l.checkpoint("M3 Swing", [-6.0, M3_Y - 1.0, z::M3_END], [6.0, M3_Y + 3.0, z::M3_START], [0.0, M3_Y, -72.0]);
    l.checkpoint("M4 Zipline", [-6.0, M4_Y - 1.0, z::M4_END], [6.0, M4_Y + 3.0, z::M4_START], [0.0, M4_Y, -95.0]);
    l.checkpoint("M5 Finish", [-8.0, M5_Y - 1.0, z::M5_END], [8.0, M5_Y + 4.0, z::M5_START], [0.0, M5_Y, -140.0]);
    l
}
