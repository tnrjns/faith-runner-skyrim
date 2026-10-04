//! Mirror's Edge's rump slide (TdMove_RumpSlide): ground too steep to stand on (its normal
//! between 0.6 and the walkable 0.7) and she slides down it on her backside.
//!
//! - **When** (CanDoMove): the floor under her is steeper than she can walk on but not a wall
//!   (UncontrolledSlideNormal.Z 0.6 up to MinSlideFloorZ).
//! - **How** (StartMove / StartSliding): her speed turned down the slope, InitialSpeedLoss 0.75
//!   of it; falling physics at GravityModifier 0.5, so the slope carries her; SideControl 350
//!   uu/s² of steering across it; MaxSlideSpeed 1000 uu/s. She turns to face down the slope over
//!   AnimBlendTime (0.5 s), and the look is held within 5000 of it (MinLook/MaxLookConstraint).
//!   `crouchslideintoend45` (in 0.15, out 0.2), then the AnimTree's `crouchslideend45` loop.
//! - **Until** she's on ground flat enough again: the move raises WalkableFloorZ to
//!   MinSlideFloorZ (0.9) while it lasts. RedoMoveTime 1 s.
//!
//! Not the game's: the slope is checked against the ground's own fall (the normal from a box can
//! be a step's edge), and a slide that's stopped dead for half a second ends.

use glam::Vec3;

use crate::controller::{horiz, Controller, Event, State};
use crate::world::{move_axis, slide_move, Body, World, WALKABLE};

const fn uu(v: f32) -> f32 {
    v / 100.0
}

/// MinSlideFloorZ: the slide goes on until the floor is at least this flat.
const MIN_SLIDE_FLOOR: f32 = 0.9;

fn yaw_of(v: Vec3) -> f32 {
    (-v.x).atan2(-v.z)
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Down the slope with normal `n`.
fn downhill(n: Vec3) -> Vec3 {
    (Vec3::NEG_Y - n * n.dot(Vec3::NEG_Y)).normalize_or_zero()
}

impl Controller {
    /// The floor right under her: its normal, if she's on something.
    pub(crate) fn floor_normal(&self, world: &dyn World) -> Option<Vec3> {
        let body = Body { half_width: self.tuning.half_width * 0.5, height: 0.2 };
        let start = self.feet + Vec3::Y * 0.1;
        let hit = world.sweep(body.aabb(Vec3::ZERO).size() * 0.5, start + Vec3::Y * 0.1, Vec3::new(0.0, -0.3, 0.0))?;
        Some(hit.normal)
    }

    /// The floor's normal comes from a small box swept down, which can catch a step's edge and
    /// read it as a steep slope (Mirror's Edge's cylinder on its smooth collision doesn't). The
    /// ground has to fall away along it as that slope says, on both sides of her, which stairs don't.
    fn slope_is_real(&self, n: Vec3, world: &dyn World) -> bool {
        let d = horiz(downhill(n)).normalize_or_zero();
        let tan = (1.0 - n.y * n.y).max(0.0).sqrt() / n.y.max(1e-3);
        // The first surface down a thin trace (any slope: tops_below only sees walkable ones).
        let top = |p: Vec3| {
            let start = Vec3::new(p.x, self.feet.y + 0.4, p.z);
            world.sweep(Vec3::splat(0.005), start, Vec3::new(0.0, -1.0, 0.0)).map(|h| start.y - h.t)
        };
        let (Some(hi), Some(mid), Some(lo)) = (top(self.feet - d * 0.15), top(self.feet), top(self.feet + d * 0.15)) else { return false };
        // Falling away on both sides of her feet, each about as the slope says (a stair is flat
        // on one side of its edge and a whole step on the other).
        let want = 0.15 * tan;
        let ok = |fall: f32| fall > want * 0.5 && fall < want * 1.5 + 0.03;
        ok(hi - mid) && ok(mid - lo)
    }

    /// TdMove_RumpSlide.CanDoMove and StartMove.
    pub(crate) fn try_rump_slide(&mut self, world: &dyn World) -> bool {
        if self.rump_redo > 0.0 {
            return false;
        }
        let Some(n) = self.floor_normal(world) else { return false };
        if !(0.6..WALKABLE).contains(&n.y) {
            return false;
        }
        let down = downhill(n);
        if !self.slope_is_real(n, world) {
            return false;
        }
        self.vel = down * self.vel.length() * 0.75;
        self.crouched = false;
        self.state = State::RumpSlide { t: 0.0, air: 0.0, face: yaw_of(horiz(down)) };
        self.events.push(Event::RumpSlide);
        true
    }

    pub(crate) fn rump_slide(&mut self, dt: f32, t: f32, air: f32, face: f32, strafe: f32, world: &dyn World) {
        let tu = self.tuning.clone();
        let t = t + dt;
        let n = self.floor_normal(world);
        // On flat enough ground again: walking (WalkableFloorZ back to normal).
        if let Some(n) = n {
            if n.y >= MIN_SLIDE_FLOOR {
                self.rump_redo = 1.0;
                self.state = State::Ground;
                return;
            }
        }
        // Held fast by odd geometry (not a slope after all): walking again.
        if t > 0.5 && self.vel.length() < 0.05 {
            self.rump_redo = 1.0;
            self.state = State::Ground;
            return;
        }
        let air = if n.is_some() { 0.0 } else { air + dt };
        if air > 0.5 {
            // Off the slope into the air.
            self.rump_redo = 1.0;
            self.state = State::Air;
            self.air_time = 0.0;
            return;
        }
        let face = n.map_or(face, |n| {
            let d = horiz(downhill(n));
            if d.length_squared() > 1e-6 { yaw_of(d) } else { face }
        });
        // Falling physics at half gravity, steering across the slope.
        self.vel.y -= tu.gravity * 0.5 * dt;
        let across = Vec3::new(face.cos(), 0.0, -face.sin());
        self.vel += across * strafe * uu(350.0) * dt;
        self.vel = self.vel.clamp_length_max(uu(1000.0));
        // PHYS_Falling against the slope: across, then down onto it; meeting it turns the
        // velocity along it (what falling onto a slope too steep to stand on does).
        let body = self.body();
        let d = self.vel * dt;
        let mut p = self.feet;
        for wall in slide_move(world, body, &mut p, d) {
            let into = self.vel.dot(wall);
            if into < 0.0 {
                self.vel -= wall * into;
            }
        }
        let hy = move_axis(world, body, &mut p, 1, d.y);
        if hy.blocked {
            let n = if hy.normal.y > 0.1 { hy.normal } else { Vec3::Y };
            let into = self.vel.dot(n);
            if into < 0.0 {
                self.vel -= n * into;
            }
        }
        self.feet = p;
        // Facing down the slope over AnimBlendTime.
        self.yaw += wrap(face - self.yaw) * (dt / 0.5).min(1.0);
        self.state = State::RumpSlide { t, air, face };
    }
}
