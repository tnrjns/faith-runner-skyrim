//! A greybox rooftop course that exercises every move, in order.
//!
//! Pure data: boxes tagged with a look (so a renderer can colour them) and
//! checkpoints. The course runs toward -Z from the spawn.
//!
//! ```text
//!  Roof A (y 0)      gap   Roof B1 (y -1)   wallrun gap   Roof B2 (y -1)   climb   Roof C (y 2.3)   drop   Roof D (y -3.7)
//!  rail, pipe, crate  3m                     over the pit                   3.3m    kick bonus       5m     stairs, finish
//! ```

use glam::Vec3;

use crate::world::{Aabb, BoxWorld, Fixture};

/// How a box should be drawn. `Runner` is the red "runner vision" highlight
/// Mirror's Edge uses for things you can interact with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Roof,
    Wall,
    Runner,
    Prop,
    Finish,
    /// Scenery far away; has collision but you'll never reach it.
    Skyline,
}

#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub name: &'static str,
    /// Walking into this region makes it the respawn point.
    pub region: Aabb,
    pub spawn: Vec3,
    pub yaw: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Level {
    pub name: &'static str,
    /// Reaching this region stops the time trial.
    pub finish: Option<Aabb>,
    pub solids: Vec<(Aabb, Look)>,
    /// Painted-on markings: drawn, but never collided with.
    pub decor: Vec<(Aabb, Look)>,
    pub checkpoints: Vec<Checkpoint>,
    /// Ziplines, swing poles, balance beams.
    pub fixtures: Vec<Fixture>,
}

/// What a box sounds like underfoot / under hand (Mirror's Edge's
/// footstep material groups).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Concrete,
    /// Girders, beams, low metal ledges.
    Metal,
    /// Rails and pipes thin enough to wrap a hand around.
    MetalPipe,
    /// Vents, ducts and AC units.
    Airduct,
    // The game's other footstep sets (A_Material_Footstep), for surfaces a host game names.
    Wood,
    /// Grating and gantries.
    MetalGantry,
    MetalLadder,
    /// Chain-link (handsteps).
    MetalFence,
    Cardboard,
    Water,
    Glass,
}

impl Surface {
    pub fn of(b: &Aabb, look: Look) -> Surface {
        let s = b.size();
        match look {
            Look::Prop => Surface::Airduct,
            // Tall red boxes are runner-vision walls; short ones are rails,
            // pipes and girders.
            Look::Runner if s.y > 2.5 => Surface::Concrete,
            Look::Runner if s.x.min(s.z).min(s.y) < 0.5 => Surface::MetalPipe,
            Look::Runner => Surface::Metal,
            _ => Surface::Concrete,
        }
    }
}

impl Level {
    /// Surface of the solid nearest `p`, if one is within `radius`.
    pub fn surface_near(&self, p: Vec3, radius: f32) -> Option<Surface> {
        self.solids
            .iter()
            .map(|(b, look)| (p.clamp(b.min, b.max).distance(p), b, *look))
            .filter(|(d, _, _)| *d <= radius)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, b, look)| Surface::of(b, look))
    }

    pub fn world(&self) -> BoxWorld {
        BoxWorld { boxes: self.solids.iter().map(|(b, _)| *b).collect(), fixtures: self.fixtures.clone(), ..Default::default() }
    }

    pub(crate) fn paint(&mut self, look: Look, min: [f32; 3], max: [f32; 3]) {
        self.decor.push((Aabb::new(Vec3::from(min), Vec3::from(max)), look));
    }

    pub(crate) fn add(&mut self, look: Look, min: [f32; 3], max: [f32; 3]) {
        self.solids.push((Aabb::new(Vec3::from(min), Vec3::from(max)), look));
    }

    pub(crate) fn checkpoint(&mut self, name: &'static str, min: [f32; 3], max: [f32; 3], spawn: [f32; 3]) {
        self.checkpoints.push(Checkpoint {
            name,
            region: Aabb::new(Vec3::from(min), Vec3::from(max)),
            spawn: Vec3::from(spawn),
            yaw: 0.0,
        });
    }

    pub fn in_finish(&self, p: Vec3) -> bool {
        self.finish.is_some_and(|f| inside(&f, p))
    }

    /// Which checkpoint (index) contains `p`, if any.
    pub fn checkpoint_at(&self, p: Vec3) -> Option<usize> {
        self.checkpoints.iter().position(|c| {
            p.x >= c.region.min.x
                && p.x <= c.region.max.x
                && p.y >= c.region.min.y
                && p.y <= c.region.max.y
                && p.z >= c.region.min.z
                && p.z <= c.region.max.z
        })
    }
}

pub(crate) fn inside(b: &Aabb, p: Vec3) -> bool {
    p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y && p.z >= b.min.z && p.z <= b.max.z
}

pub mod z {
    //! Key z positions along the course, shared with tests.
    pub const ROOF_A_END: f32 = -32.0;
    pub const RAIL: f32 = -8.0;
    pub const PIPE: f32 = -14.0;
    pub const CRATE: f32 = -20.0;
    pub const ROOF_B1_START: f32 = -35.0;
    pub const ROOF_B1_END: f32 = -44.0;
    pub const ROOF_B2_START: f32 = -52.0;
    pub const CLIMB_WALL: f32 = -72.0;
    pub const KICK_WALL: f32 = -100.0;
    pub const KICK_PLATFORM: f32 = -96.5;
    pub const ROOF_D_START: f32 = -100.0;
}

pub const ROOF_B_Y: f32 = -1.0;
pub const ROOF_C_Y: f32 = 2.3;
pub const ROOF_D_Y: f32 = -3.7;
pub const WALLRUN_X: f32 = 5.2;

pub fn greybox() -> Level {
    use Look::*;
    let mut l = Level { name: "Training", ..Level::default() };
    l.finish = Some(Aabb::new(Vec3::new(4.0, ROOF_D_Y + 1.6, -130.0), Vec3::new(10.0, ROOF_D_Y + 5.0, -125.0)));

    // ---- Roof A: vault, slide, mantle -----------------------------------
    l.add(Roof, [-6.0, -6.0, z::ROOF_A_END], [6.0, 0.0, 4.0]);
    l.add(Wall, [-6.0, 0.0, 3.5], [6.0, 4.0, 4.0]); // back wall behind spawn
    // Parapets along the sides (too tall to step, low enough to vault).
    l.add(Wall, [-6.0, 0.0, z::ROOF_A_END], [-5.7, 0.9, 3.5]);
    l.add(Wall, [5.7, 0.0, z::ROOF_A_END], [6.0, 0.9, 3.5]);
    // Vault rail.
    l.add(Runner, [-3.0, 0.0, z::RAIL - 0.2], [3.0, 1.0, z::RAIL]);
    l.add(Prop, [-5.7, 0.0, z::RAIL - 0.2], [-3.0, 2.2, z::RAIL]);
    l.add(Prop, [3.0, 0.0, z::RAIL - 0.2], [5.7, 2.2, z::RAIL]);
    // Pipe you slide under, on two posts.
    l.add(Runner, [-5.7, 1.3, z::PIPE - 0.4], [5.7, 1.6, z::PIPE]);
    l.add(Prop, [-5.7, 1.6, z::PIPE - 0.4], [5.7, 3.0, z::PIPE]);
    // Crate to mantle onto, and a lower step off the back.
    l.add(Runner, [-2.5, 0.0, z::CRATE - 3.0], [2.5, 1.1, z::CRATE]);
    l.add(Prop, [-5.7, 0.0, z::CRATE - 3.0], [-2.5, 2.4, z::CRATE]);
    l.add(Prop, [2.5, 0.0, z::CRATE - 3.0], [5.7, 2.4, z::CRATE]);
    // Red edge at the jump.
    l.paint(Runner, [-5.7, -0.02, z::ROOF_A_END], [5.7, 0.004, z::ROOF_A_END + 0.4]);

    // ---- Roof B1: land the gap, line up the wallrun ---------------------
    l.add(Roof, [-6.0, -7.0, z::ROOF_B1_END], [6.0, ROOF_B_Y, z::ROOF_B1_START]);
    l.add(Wall, [-6.0, ROOF_B_Y, z::ROOF_B1_END], [-5.7, ROOF_B_Y + 0.9, z::ROOF_B1_START]);
    // Wallrun wall on the right, spanning the pit between B1 and B2.
    l.add(Runner, [WALLRUN_X, -8.0, -60.0], [WALLRUN_X + 0.6, 5.0, -38.0]);
    // Guide strip on the floor toward the wall.
    l.paint(Runner, [2.5, ROOF_B_Y - 0.02, -43.0], [WALLRUN_X, ROOF_B_Y + 0.004, -42.6]);

    // ---- Roof B2: land after the wallrun, climb the wall ----------------
    l.add(Roof, [-6.0, -7.0, z::CLIMB_WALL], [WALLRUN_X, ROOF_B_Y, z::ROOF_B2_START]);
    l.add(Wall, [-6.0, ROOF_B_Y, z::CLIMB_WALL], [-5.7, ROOF_B_Y + 0.9, z::ROOF_B2_START]);

    // ---- Roof C: up top. Kick bonus on the left, drop on the right -----
    // The block itself is the climb wall (face at z = -72).
    l.add(Wall, [-6.0, -7.0, z::KICK_WALL], [6.0, ROOF_C_Y, z::CLIMB_WALL]);
    l.paint(Runner, [-2.0, ROOF_C_Y - 0.25, z::CLIMB_WALL - 0.01], [2.0, ROOF_C_Y + 0.004, z::CLIMB_WALL + 0.004]);
    // A ladder and a drainpipe up it too, either side of the climb.
    l.fixtures.push(Fixture::Ladder(crate::climb::Ladder { base: Vec3::new(4.2, ROOF_B_Y, z::CLIMB_WALL), top: ROOF_C_Y, normal: Vec3::Z, pipe: false, exit: true }));
    l.fixtures.push(Fixture::Ladder(crate::climb::Ladder { base: Vec3::new(-4.2, ROOF_B_Y, z::CLIMB_WALL), top: ROOF_C_Y, normal: Vec3::Z, pipe: true, exit: true }));
    // Tall wall to climb, turn and kick off (left half only).
    l.add(Runner, [-6.0, ROOF_C_Y, z::KICK_WALL - 4.0], [0.0, 16.0, z::KICK_WALL]);
    // Catwalk you kick back onto.
    l.add(Runner, [-5.0, 6.6, z::KICK_PLATFORM], [-1.0, 7.4, z::KICK_PLATFORM + 0.01]);
    l.add(Prop, [-5.0, 6.6, -89.0], [-1.0, 7.4, z::KICK_PLATFORM]);
    l.add(Finish, [-3.5, 7.4, -91.0], [-2.5, 9.4, -90.0]);
    // Divider between the kick zone and the drop.
    l.add(Wall, [0.0, ROOF_C_Y, z::KICK_WALL - 4.0], [0.3, ROOF_C_Y + 1.1, -94.0]);

    // ---- Roof D: 6 m drop (roll it), stairs, finish ---------------------
    l.add(Roof, [0.3, -9.0, -140.0], [14.0, ROOF_D_Y, z::ROOF_D_START]);
    for i in 0..8 {
        let z0 = -112.0 - i as f32 * 0.45;
        l.add(Prop, [4.0, ROOF_D_Y, -130.0], [10.0, ROOF_D_Y + 0.22 * (i + 1) as f32, z0]);
    }
    l.add(Finish, [6.5, ROOF_D_Y + 1.76, -128.0], [7.5, ROOF_D_Y + 5.0, -127.0]);
    // Two swing bars beside the stairs, one after the other: jump off the first at the second.
    for z in [-115.0, -118.5] {
        l.fixtures.push(Fixture::SwingPole { a: Vec3::new(10.8, ROOF_D_Y + 2.6, z), b: Vec3::new(13.6, ROOF_D_Y + 2.6, z) });
    }

    // ---- Skyline: tall white blocks all around -----------------------
    let sky = [
        ([-40.0, -40.0, -30.0], [-18.0, 30.0, -10.0]),
        ([-36.0, -40.0, -80.0], [-16.0, 45.0, -50.0]),
        ([-45.0, -40.0, -150.0], [-20.0, 60.0, -110.0]),
        ([22.0, -40.0, -40.0], [40.0, 25.0, -15.0]),
        ([26.0, -40.0, -95.0], [48.0, 55.0, -60.0]),
        ([24.0, -40.0, -170.0], [52.0, 40.0, -140.0]),
        ([-20.0, -40.0, -200.0], [15.0, 70.0, -180.0]),
        ([-30.0, -40.0, 20.0], [30.0, 20.0, 40.0]),
    ];
    for (a, b) in sky {
        l.add(Skyline, a, b);
    }

    // ---- Checkpoints -----------------------------------------------
    l.checkpoint("Roof A", [-6.0, -1.0, -6.0], [6.0, 3.0, 3.5], [0.0, 0.0, 0.0]);
    l.checkpoint("Roof B1", [-6.0, -2.0, z::ROOF_B1_END], [6.0, 2.0, z::ROOF_B1_START], [0.0, ROOF_B_Y, -36.5]);
    l.checkpoint("Roof B2", [-6.0, -2.0, z::CLIMB_WALL], [WALLRUN_X, 2.0, z::ROOF_B2_START], [0.0, ROOF_B_Y, -56.0]);
    l.checkpoint("Roof C", [-6.0, 1.5, -94.0], [6.0, 6.0, z::CLIMB_WALL], [2.0, ROOF_C_Y, -76.0]);
    l.checkpoint("Roof D", [0.3, -4.0, -140.0], [14.0, 0.0, z::ROOF_D_START], [6.0, ROOF_D_Y, -104.0]);
    l
}

#[cfg(test)]
mod surface_tests {
    use super::*;

    #[test]
    fn surfaces_by_box() {
        let l = crate::rooftops::rooftops();
        // Standing on the plain roof.
        assert_eq!(l.surface_near(Vec3::new(0.0, 0.0, -2.0), 0.2), Some(Surface::Concrete));
        // On top of the runner pipe.
        let pipe = crate::rooftops::z::PIPE - 0.17;
        assert_eq!(l.surface_near(Vec3::new(0.0, 1.56, pipe), 0.2), Some(Surface::MetalPipe));
        // On a (red, vault-over) vent box: sheet metal.
        assert_eq!(l.surface_near(Vec3::new(0.0, crate::rooftops::R6_Y + 0.81, crate::rooftops::z::VENT_A - 0.25), 0.2), Some(Surface::Metal));
        // On top of the big duct.
        assert_eq!(l.surface_near(Vec3::new(0.0, crate::rooftops::R6_Y + 3.51, crate::rooftops::z::DUCT - 0.25), 0.2), Some(Surface::Airduct));
        // Mid-air.
        assert_eq!(l.surface_near(Vec3::new(0.0, 30.0, 0.0), 0.2), None);
    }
}
