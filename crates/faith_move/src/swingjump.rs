//! Mirror's Edge's swing-to-swing jump (TdMove_SwingJump): jumping off a swing bar with
//! another one ahead, she's carried across to it.
//!
//! - **Which bar** (TdMove_Swing.JumpOff: CheckForTargetVolume, native 0x1206770): a trace
//!   along the swing from 100 uu past the bar to 100 + max(0, 500 x SwingVelocity /
//!   MaxSwingVelocity) uu, for another bar's TdSwingVolume. Here the bars have no volumes: a
//!   bar within grab reach of that line counts.
//! - **How** (StartMove): GravityModifier 0.73 for 0.75 s; PHYS_Flying straight at a point
//!   120 uu short of the bar (along the way to it) and 20 uu down, at the 2D distance over
//!   0.9 s; at 0.9 s, falling (OnTimer). `SwingOff` (1.0, in 0.2, out 0.2). The bar's volume
//!   takes her into the swing when she reaches it (TdSwingVolume.PawnUpdate).
//!
//! The target is the closest point of that bar to the line (a volume's Location is its middle).

use glam::Vec3;

use crate::controller::{Controller, Event, State};
use crate::world::{closest_on_segment, slide_move, Fixture, World};

const fn uu(v: f32) -> f32 {
    v / 100.0
}

/// TdMove_SwingJump: TargetVolumeOffset (-120, 0, -20), GravityModifier(Timer), the flight.
const OFFSET_BACK: f32 = uu(120.0);
const OFFSET_DOWN: f32 = uu(20.0);
pub(crate) const GRAVITY: f32 = 0.73;
pub(crate) const GRAVITY_TIME: f32 = 0.75;
const FLIGHT: f32 = 0.9;

impl Controller {
    /// CheckForTargetVolume: another bar along the swing, within reach of this speed.
    pub(crate) fn swing_target(&self, a: Vec3, b: Vec3, at: Vec3, dir: Vec3, rate: f32, world: &dyn World) -> Option<Vec3> {
        let tu = &self.tuning;
        let reach = uu(100.0) + (uu(500.0) * rate / tu.swing_max_rate).max(0.0);
        let steps = ((reach - uu(100.0)) / 0.1).ceil().max(1.0) as usize;
        for k in 0..=steps {
            let p = at + dir * (uu(100.0) + (reach - uu(100.0)) * k as f32 / steps as f32);
            for f in world.fixtures() {
                let Fixture::SwingPole { a: fa, b: fb } = *f else { continue };
                if fa == a && fb == b {
                    continue;
                }
                let (cp, _) = closest_on_segment(fa, fb, p);
                if cp.distance(p) <= tu.grab_reach {
                    return Some(cp);
                }
            }
        }
        None
    }

    /// TdMove_SwingJump.StartMove, with the target bar's point.
    pub(crate) fn start_swing_jump(&mut self, target: Vec3) {
        let centre = self.centre();
        let to_bar = target - centre;
        let goal = target - to_bar.normalize_or_zero() * OFFSET_BACK - Vec3::Y * OFFSET_DOWN;
        let d2 = Vec3::new(to_bar.x, 0.0, to_bar.z).length();
        let speed = d2 / FLIGHT;
        let to = goal - Vec3::Y * self.height() * 0.5;
        self.low_grav = GRAVITY_TIME;
        self.low_grav_k = GRAVITY;
        self.jumped_since_ground = true;
        self.jump_buffer = 0.0;
        self.fixture_cooldown = 0.6;
        self.vel = (to - self.feet).normalize_or_zero() * speed;
        self.state = State::SwingJump { t: 0.0, to, speed };
        self.events.push(Event::SwingToSwing);
    }

    /// SetPreciseLocation (PHYS_Flying) at the target until the 0.9 s timer, then falling.
    pub(crate) fn swing_jump(&mut self, dt: f32, t: f32, to: Vec3, speed: f32, world: &dyn World) {
        let t = t + dt;
        let left = to - self.feet;
        let step = (speed * dt).min(left.length());
        let d = left.normalize_or_zero() * step;
        if step > 1e-6 {
            self.vel = left.normalize_or_zero() * speed;
        }
        let mut p = self.feet;
        slide_move(world, self.body(), &mut p, d);
        crate::world::move_axis(world, self.body(), &mut p, 1, d.y);
        self.feet = p;
        if t >= FLIGHT {
            self.state = State::Air;
            self.air_time = 0.0;
            return;
        }
        self.state = State::SwingJump { t, to, speed };
        self.try_grab_fixture(world);
    }
}
