//! Mirror's Edge's disarm (TdMOVE_Disarm) as a takedown on one of the host's people: the
//! takedown key with someone in front of her.
//!
//! - **Who** (CanDoMove): the melee target (GetMeleeTarget), within TdMove_MeleeBase's
//!   TargetingMaxDistance (the game's own disarm window is closer still), on the same level
//!   (70 uu) and with nothing in between.
//! - **Which** (ChooseDisarmType): facing away from her, SnatchBack; facing her, a front snatch
//!   (SnatchFwd, SnatchFwd2, SnatchFwd3, as for a patrol cop's light weapon). The game compares
//!   the two pawns' facings; here it's their facing against the way to them, so what plays
//!   matches where they actually stand.
//! - **Where** (StartMove / AlignPawn): she slides to DisarmOffset (125.899) short of them, at
//!   max(400, speed) uu/s, facing them. Their side of it (the enemy's clip) is placed facing her
//!   for all four: that's where the two clips' hands meet (from behind too: the clip turns the
//!   body itself: measured with the two clips side by side). If she can't get there they're brought to
//!   her instead (HandlePlayerUnableToMove).
//! - **How long**: the clip (OnCustomAnimEnd); look and move input are ignored throughout
//!   (DisableLookTime / DisableMovementTime -1), the view swings onto them (UpdateMeleeAutoLockOn)
//!   and levels (ResetCameraLook 0.2).
//!
//! The game only allows it in the enemy's disarm window (the AI's QueryDisarmState); here
//! any target will do. What happens to them is the host's call (`Event::TakedownDone`).

use glam::Vec3;

use crate::controller::{forward, horiz, Controller, Event, State};
use crate::melee::pick;
use crate::world::World;

const fn uu(v: f32) -> f32 {
    v / 100.0
}

/// ChooseDisarmType's DisarmOffset.
const OFFSET: f32 = uu(125.899);
/// TdMove_MeleeBase.TargetingMaxDistance.
const REACH: f32 = uu(300.0);

/// The takedown clips, by `Event::Takedown::anim`.
pub const TAKEDOWN_ANIMS: [&str; 4] = ["SnatchFwd", "SnatchFwd2", "SnatchFwd3", "SnatchBack"];

fn yaw_of(v: Vec3) -> f32 {
    (-v.x).atan2(-v.z)
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

impl Controller {
    /// Start a takedown on the target in front of her, if there is one.
    pub(crate) fn try_takedown(&mut self, world: &dyn World) -> bool {
        if self.state != State::Ground || self.crouched || self.melee.is_some() {
            return false;
        }
        let centre = self.centre();
        let fwd = forward(self.yaw);
        let Some(t) = pick(&self.targets, REACH, centre, fwd) else { return false };
        let d = t.centre - centre;
        if (t.centre.y - centre.y).abs() > uu(70.0) || horiz(d).length() > REACH {
            return false;
        }
        let toward = horiz(d).normalize_or_zero();
        if toward == Vec3::ZERO || world.sweep(Vec3::splat(0.01), centre, d - toward * t.radius).is_some() {
            return false;
        }
        let back = toward.dot(t.facing) > 0.0;
        let anim = if back { 3 } else { (self.takedowns % 3) as u8 };
        self.takedowns += 1;

        let body = self.body();
        let mut to = Vec3::new(t.centre.x, self.feet.y, t.centre.z) - toward * OFFSET;
        let mut enemy_at = t.centre;
        let mut face = yaw_of(toward);
        if world.sweep(body.aabb(Vec3::ZERO).size() * 0.5, self.feet + Vec3::Y * (body.height * 0.5 + 0.05), to - self.feet).is_some() {
            to = self.feet;
            enemy_at = Vec3::new(centre.x, t.centre.y, centre.z) + fwd * OFFSET;
            face = self.yaw;
        }
        let enemy_dir = -forward(face);
        let len = self.tuning.takedown_clips[anim as usize];
        self.vel = Vec3::ZERO;
        self.state = State::Takedown { t: 0.0, len, target: t.id, from: self.feet, to, face };
        self.events.push(Event::Takedown { target: t.id, anim, enemy_at, enemy_dir });
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn takedown(&mut self, dt: f32, t: f32, len: f32, target: u32, from: Vec3, to: Vec3, face: f32) {
        let t = t + dt;
        let dist = from.distance(to);
        let k = if dist > 1e-4 { (t * uu(400.0) / dist).min(1.0) } else { 1.0 };
        self.feet = from.lerp(to, k);
        self.vel = Vec3::ZERO;
        let a = (dt / 0.2).min(1.0);
        self.yaw += wrap(face - self.yaw) * a;
        self.pitch -= self.pitch * a;
        if t >= len {
            self.state = State::Ground;
            self.events.push(Event::TakedownDone { target });
        } else {
            self.state = State::Takedown { t, len, target, from, to, face };
        }
    }
}
