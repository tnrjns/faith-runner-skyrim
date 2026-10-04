//! Mirror's Edge's air barge (TdMove_AirBarge): attack in the air with a door ahead, and she
//! goes into it shoulder first.
//!
//! - **When** (TdPlayerController's PlayerWalking HandleMoveAction for the jumps and falling:
//!   attack tries the air barge before the air kick; TdMove_Barge.CanDoMove): a door within
//!   BargeMinTraceDistance (300 uu) straight ahead of her middle, or BargeSpeed x BargeTraceTime
//!   moving forwards.
//! - **How** (StartBargin): `AirBargeIdle` (in 0.15, out 0.15) while she flies on (PHYS_Falling);
//!   TotalHeightBoost (60 uu) lifts her over HeightBoostDuration (0.25 s). Hitting the door
//!   (TryGiveBargeDamage on the bump) opens it and plays `AirBargeImpact` (in 0, out 0.1), then
//!   the landing; touching down plays `AirBargeLand` (in 0.1, out 0.1, root motion), then
//!   walking. `AirBargeIdle` running out first: falling.
//!
//! The landing's root motion isn't read: she stops where she touches down while it plays.

use glam::Vec3;

use crate::controller::{forward, horiz, Controller, Event, State};
use crate::world::{move_axis, slide_move, World};

const fn uu(v: f32) -> f32 {
    v / 100.0
}

/// TdMove_AirBarge: BargeMinTraceDistance, TotalHeightBoost, HeightBoostDuration.
const MIN_TRACE: f32 = uu(300.0);
const HEIGHT_BOOST: f32 = uu(60.0);
const BOOST_TIME: f32 = 0.25;

/// Where the air barge is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirBargePhase {
    /// AirBargeIdle: flying at the door.
    Flying,
    /// AirBargeImpact: through it.
    Impact,
    /// AirBargeLand.
    Landing,
}

impl Controller {
    /// TdMove_Barge.CanDoMove: the door straight ahead (BargeActorList), and how far.
    pub(crate) fn door_ahead(&self, min_trace: f32) -> Option<(usize, f32)> {
        let tu = &self.tuning;
        let fwd = forward(self.yaw);
        let h = horiz(self.vel);
        let speed = h.length();
        let forward_moving = speed > 0.01 && fwd.dot(h / speed) > 0.707;
        let barge_speed = (speed + tu.barge_add_speed).min(tu.barge_max_speed);
        let reach = if forward_moving { min_trace.max(barge_speed * tu.barge_trace_time) } else { min_trace };
        let centre = self.feet + Vec3::Y * (self.height() * 0.5);
        self.doors.iter().enumerate().find_map(|(i, b)| {
            let b = (*b)?;
            if self.doors_open.get(i).is_some_and(|&o| o > 0.0) {
                return None;
            }
            // Trace from the cylinder centre straight ahead: the door and how far it is.
            let steps = (reach / 0.05).ceil() as usize;
            (0..=steps).map(|k| reach * k as f32 / steps.max(1) as f32).find(|&d| {
                let p = centre + fwd * d;
                p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y && p.z >= b.min.z && p.z <= b.max.z
            }).map(|d| (i, d))
        })
    }

    /// TdMove_AirBarge.CanDoMove and StartBargin.
    pub(crate) fn try_air_barge(&mut self) -> bool {
        let Some((door, _)) = self.door_ahead(MIN_TRACE) else { return false };
        self.state = State::AirBarge { t: 0.0, door, phase: AirBargePhase::Flying, boost: HEIGHT_BOOST };
        self.events.push(Event::AirBarge);
        true
    }

    pub(crate) fn air_barge(&mut self, dt: f32, t: f32, door: usize, mut phase: AirBargePhase, mut boost: f32, world: &dyn World) {
        let tu = self.tuning.clone();
        let mut t = t + dt;
        let clips = tu.air_barge_clips;
        let body = self.body();
        if phase == AirBargePhase::Landing && self.grounded(world) {
            // AirBargeLand's root motion (not read): held where she came down.
            self.vel = Vec3::new(0.0, self.vel.y.min(0.0), 0.0);
        }
        // PHYS_Falling, plus the height boost spread over its time.
        self.vel.y -= tu.gravity * dt;
        let lift = (HEIGHT_BOOST * dt / BOOST_TIME).min(boost);
        boost -= lift;
        let d = self.vel * dt;
        let mut p = self.feet;
        let walls = slide_move(world, body, &mut p, d);
        for n in &walls {
            let into = self.vel.dot(*n);
            if into < 0.0 {
                self.vel -= *n * into;
            }
        }
        let hy = move_axis(world, body, &mut p, 1, d.y + lift);
        let landed = hy.blocked && d.y + lift < 0.0;
        if hy.blocked {
            self.vel.y = 0.0;
        }
        self.feet = p;
        // TryGiveBargeDamage: bumping the door opens it, once, and not while landing.
        if phase == AirBargePhase::Flying {
            let fwd = forward(self.yaw);
            let touching = self.doors.get(door).copied().flatten().is_some_and(|b| {
                let mut me = body.aabb(self.feet).translated(fwd * 0.1);
                me.min.y += 0.2;
                me.overlaps(&b)
            });
            if touching {
                self.open_door(door);
                phase = AirBargePhase::Impact;
                t = 0.0;
                self.events.push(Event::AirBargeImpact);
            }
        }
        match phase {
            // Landed (PlayLanded), or AirBargeIdle over: falling.
            AirBargePhase::Flying if landed => {
                phase = AirBargePhase::Landing;
                t = 0.0;
            }
            AirBargePhase::Flying if t >= clips[0] => {
                self.state = State::Air;
                return;
            }
            // AirBargeImpact over: the landing (PlayLanded).
            AirBargePhase::Impact if t >= clips[1] => {
                phase = AirBargePhase::Landing;
                t = 0.0;
            }
            // AirBargeLand over: walking.
            AirBargePhase::Landing if t >= clips[2] => {
                self.state = if landed || self.vel.y == 0.0 { State::Ground } else { State::Air };
                self.air_time = 0.0;
                return;
            }
            _ => {}
        }
        if phase == AirBargePhase::Landing && t == 0.0 {
            self.events.push(Event::AirBargeLand);
        }
        self.state = State::AirBarge { t, door, phase, boost };
    }
}
