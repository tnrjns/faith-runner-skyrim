//! Mirror's Edge's grab transfer (TdMove_GrabTransfer), up: hanging from a ledge, jump while
//! pushing up and, with another ledge within reach above, she reaches up to it.
//!
//! - **When** (TdMove_Grab's MA_Jump: the transfer is tried before the pull-up; CanDoMove,
//!   CheckContextMove with the hint up): a ledge on the wall above, no more than
//!   AllowedZTransferDistance (140 uu) up, with the way there clear (bFitForGrab).
//! - **How** (StartMove / OnTimer / PlayTransferAnimation): straight there at max(distance /
//!   0.55, 200) uu/s, `hangtransferup` (in 0.1, out 0.1), and hanging from the new ledge
//!   (SetMove(Grab), not IntoGrab: no grab impact).
//!
//! The left / right transfers (round to the next wall: CheckContextMove's native search over
//! the level's grab markers) aren't here.

use glam::Vec3;

use crate::controller::{Controller, Event, State};
use crate::world::{find_ledge, trace_wall, World};

const fn uu(v: f32) -> f32 {
    v / 100.0
}

impl Controller {
    pub(crate) fn try_grab_transfer(&mut self, n: Vec3, ledge_y: f32, world: &dyn World) -> bool {
        let tu = &self.tuning;
        let body = self.body();
        let hands = tu.hang_hands_above_feet;
        // The wall above this ledge (it may be set back from it), and a ledge on it in reach.
        let above = Vec3::new(self.feet.x, ledge_y + 0.05, self.feet.z);
        let back = trace_wall(world, body, above, -n, 1.0, body.height * 0.5).map_or(0.0, |w| w.gap);
        let at = self.feet - n * back;
        let Some(top) = find_ledge(world, body, at, n, hands + 0.3, hands + uu(140.0), tu.crouch_height) else { return false };
        let to = Vec3::new(at.x, top - hands, at.z);
        // bFitForGrab: the way up is clear.
        let half = body.aabb(Vec3::ZERO).size() * 0.5;
        if world.sweep(half * 0.95, self.feet + Vec3::Y * half.y + n * 0.02, to - self.feet).is_some() {
            return false;
        }
        let dist = (to - self.feet).length();
        let dur = dist / (dist / 0.55).max(uu(200.0));
        self.state = State::GrabTransfer { t: 0.0, from: self.feet, to, dur, normal: n, ledge_y: top };
        self.events.push(Event::GrabTransfer);
        true
    }

    pub(crate) fn grab_transfer(&mut self, dt: f32, t: f32, from: Vec3, to: Vec3, dur: f32, normal: Vec3, ledge_y: f32) {
        let t = t + dt;
        let k = if dur > 1e-4 { (t / dur).min(1.0) } else { 1.0 };
        self.feet = from.lerp(to, k);
        self.vel = Vec3::ZERO;
        if k >= 1.0 {
            self.state = State::LedgeHang { normal, ledge_y, turned: false };
            self.hang_time = 0.0;
            self.shimmy = None;
        } else {
            self.state = State::GrabTransfer { t, from, to, dur, normal, ledge_y };
        }
    }
}
