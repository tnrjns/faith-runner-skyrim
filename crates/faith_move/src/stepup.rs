//! Mirror's Edge's auto step-up (TdMove_AutoStepUp): walking into something knee-high, she steps
//! up onto it with `autostepuprightleg` rather than stopping against it.
//!
//! The game ships the move switched off (its CanDoMove returns false before any check) and
//! leans on its levels' collision ramps instead; a host whose stairs have no ramps (other games'
//! cities) can turn it on (`Tuning::auto_step_up`). It's the move as written:
//! - **When** (CanDoMove): moving towards a ledge whose top is StepUpHighMinHeight (8) to
//!   StepUpHighMaxHeight (48) uu above her feet, no more than 100 uu and 0.35 s away (at
//!   max(150, speed) uu/s), not rising nor falling faster than 600 uu/s, and room to stand 40 uu
//!   past its edge, with the way there clear. Steps the walk climbs by itself (up to its step
//!   height) are left to it; this takes the taller ones.
//! - **How** (StartMove / StopMove): `autostepuprightleg` at 0.8 (in 0.15, out 0.25); she goes
//!   straight to the end at max(200 uu/s, half her speed), then carries on at half the speed she
//!   came in with.

use glam::Vec3;

use crate::controller::{forward, horiz, Controller, Event, State};
use crate::world::{column_top, trace_wall, Body, World};

const fn uu(v: f32) -> f32 {
    v / 100.0
}

impl Controller {
    /// TdMove_AutoStepUp.CanDoMove and StartMove.
    pub(crate) fn try_auto_step_up(&mut self, world: &dyn World) -> bool {
        let tu = &self.tuning;
        let fwd = forward(self.yaw);
        let body = self.body();
        let speed = horiz(self.vel).length();
        if self.vel.y < -uu(600.0) || self.vel.y > 0.0 || speed < 0.05 {
            return false;
        }
        // The face in front, low down (above what the walk steps up by itself).
        let reach = uu(100.0);
        let Some(hit) = trace_wall(world, body, self.feet, fwd, reach, tu.step_height * 0.5) else { return false };
        // Moving towards it: Velocity . MoveNormal <= 0.
        if self.vel.dot(hit.n) > 0.0 {
            return false;
        }
        let inside = hit.at - hit.n * 0.02;
        let max = tu.auto_step_up_max;
        let top = column_top(world, inside, self.feet.y, self.feet.y + max + 0.05);
        let height = top - self.feet.y;
        if height <= tu.step_height || height > max {
            return false;
        }
        let to_ledge = horiz(hit.at - self.feet).length();
        if to_ledge > uu(100.0) || to_ledge / speed.max(uu(150.0)) > 0.35 {
            return false;
        }
        // EndPosition: 40 uu past the edge, on top; room to stand there and on the way.
        let along = -hit.n;
        let edge = Vec3::new(hit.at.x, top, hit.at.z);
        let end = edge + along * uu(40.0) + Vec3::Y * 0.02;
        let stand = Body { half_width: tu.half_width, height: tu.stand_height };
        if !world.is_free(&stand.aabb(end)) {
            return false;
        }
        let half = stand.aabb(Vec3::ZERO).size() * 0.5;
        let lift = Vec3::new(self.feet.x, end.y, self.feet.z);
        if world.sweep(half, lift + Vec3::Y * half.y, end - lift).is_some() {
            return false;
        }
        let saved = self.vel * 0.5;
        let rate = uu(200.0).max(horiz(saved).length());
        let dur = (end - self.feet).length() / rate;
        self.state = State::StepUp { t: 0.0, from: self.feet, to: end, dur, saved };
        self.events.push(Event::StepUp);
        true
    }

    /// Straight to the end (SetPreciseLocation, no collision: bDisableCollision), then
    /// walking again at half the speed she came in with (StopMove).
    pub(crate) fn step_up(&mut self, dt: f32, t: f32, from: Vec3, to: Vec3, dur: f32, saved: Vec3) {
        let t = t + dt;
        let k = if dur > 1e-4 { (t / dur).min(1.0) } else { 1.0 };
        self.feet = from.lerp(to, k);
        self.vel = (to - from) / dur.max(1e-4);
        if k >= 1.0 {
            self.vel = Vec3::new(saved.x, 0.0, saved.z);
            self.state = State::Ground;
        } else {
            self.state = State::StepUp { t, from, to, dur, saved };
        }
    }
}
