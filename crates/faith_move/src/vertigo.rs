//! Mirror's Edge's vertigo (TdMove_Vertigo): walking up to a long drop, she stops at the edge
//! and looks down over it.
//!
//! - **When** (ATdPlayerPawn's walking edge check, 0x12be0f0): the step would take her off an
//!   edge; nothing within VertigoEdgeProbingHeight (1000 uu) under a point
//!   VertigoEdgeProbingDistance (70 uu) out that way from her middle; she faces that way
//!   (|dot| > VertigoEffectThreshold, 0.9); walking, slower than a run (CurrentWalkingState
//!   below WAS_Run); not the same edge as last time (2 uu); and TdMove_Vertigo.CanDoMove: the
//!   edge ahead of her, and nothing in the way (a box of her radius and 0.7 of her height, two
//!   radii on from 0.2 of her height up).
//! - **How** (StartMove): she stops there; the view pitches down to -15000 over 0.28 s
//!   (SetLookAtTargetAngle) and zooms to ZoomFOV (84) at ZoomRate (30), until ZoomOutTime
//!   (1.2 s: UnZoom, AbortLookAtTarget); `edgedetection` (1.0, in 0.28, out 0.28). Moving and
//!   looking are off for DisableMovementTime / DisableLookTime (1.5 s).
//! - **Until** she turns more than 8000 off the edge (UpdateViewRotation), or moves other than
//!   over it (the edge check, given a step that doesn't go off, hands back to walking): a step
//!   over it keeps her standing there. RedoMoveTime 3 s. StopMove: `edgedetection` out over 0.4.
//!
//! The step-off test (0x12b8a30, EdgeCheckDistance / EdgeStopMinHeight) isn't decoded: here,
//! no floor within EdgeStopMinHeight (36 uu) under her edge EdgeCheckDistance (20 uu) on.

use glam::{Vec2, Vec3};

use crate::controller::{forward, horiz, right, Controller, Event, Input, State};
use crate::world::World;

const fn uu(v: f32) -> f32 {
    v / 100.0
}

/// Rotation units to radians.
fn rot(u: f32) -> f32 {
    u / 65536.0 * std::f32::consts::TAU
}

const PROBE_OUT: f32 = uu(70.0);
const PROBE_DOWN: f32 = uu(1000.0);
const THRESHOLD: f32 = 0.9;
const EDGE_CHECK: f32 = uu(20.0);
const EDGE_STOP_HEIGHT: f32 = uu(36.0);
/// WAS_Run starts at RunVelocity (400 uu/s).
const RUN_SPEED: f32 = uu(400.0);
pub(crate) const ZOOM_OUT_TIME: f32 = 1.2;
const DISABLE_TIME: f32 = 1.5;
const REDO: f32 = 3.0;

fn yaw_of(v: Vec3) -> f32 {
    (-v.x).atan2(-v.z)
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

impl Controller {
    /// Would a step along `dir` take her off an edge?
    fn steps_off(&self, dir: Vec3, world: &dyn World) -> bool {
        let p = self.feet + dir * (self.tuning.half_width + EDGE_CHECK);
        let top = Vec3::new(p.x, self.feet.y + 0.05, p.z);
        world.sweep(Vec3::splat(0.005), top, Vec3::new(0.0, -(0.05 + EDGE_STOP_HEIGHT), 0.0)).is_none()
    }

    /// The walking edge check's vertigo half, before the step.
    pub(crate) fn try_vertigo(&mut self, world: &dyn World) -> bool {
        if self.vertigo_redo > 0.0 || self.crouched {
            return false;
        }
        let h = horiz(self.vel);
        let speed = h.length();
        if speed < 0.05 || speed >= RUN_SPEED {
            return false;
        }
        let dir = h / speed;
        if !self.steps_off(dir, world) {
            return false;
        }
        // Nothing under the probe for 1000 uu.
        let centre = self.centre();
        let probe = centre + dir * PROBE_OUT;
        if world.sweep(Vec3::splat(0.005), probe, Vec3::new(0.0, -PROBE_DOWN, 0.0)).is_some() {
            return false;
        }
        let fwd = forward(self.yaw);
        if dir.dot(fwd).abs() <= THRESHOLD {
            return false;
        }
        if self.last_vertigo_edge.is_some_and(|e| (e - probe).length() <= uu(2.0)) {
            return false;
        }
        // TdMove_Vertigo.CanDoMove.
        if horiz(probe - centre).normalize_or_zero().dot(fwd) < 0.0 {
            return false;
        }
        let r = self.tuning.half_width;
        let half_h = self.height() * 0.5;
        let start = centre + Vec3::Y * half_h * 0.2;
        if world.sweep(Vec3::new(r, half_h * 0.7, r), start, fwd * r * 2.0).is_some() {
            return false;
        }
        self.last_vertigo_edge = Some(probe);
        self.vel = Vec3::ZERO;
        self.vertigo_zoom = true;
        self.state = State::Vertigo { t: 0.0, edge: probe };
        self.events.push(Event::Vertigo);
        true
    }

    /// Before the look and the move are applied: what the move holds off.
    pub(crate) fn vertigo_input(&self, input: &mut Input) {
        if let State::Vertigo { t, .. } = self.state {
            if t < DISABLE_TIME {
                input.look = Vec2::ZERO;
                input.move_axis = Vec2::ZERO;
                input.jump_pressed = false;
                input.crouch_pressed = false;
            }
        }
    }

    pub(crate) fn vertigo(&mut self, dt: f32, t: f32, edge: Vec3, input: &Input) {
        let t = t + dt;
        self.vel = Vec3::ZERO;
        // SetLookAtTargetAngle(pitch -15000, 0.28) until ZoomOutTime.
        if t < ZOOM_OUT_TIME {
            self.pitch += (-rot(15000.0) - self.pitch) * (dt / 0.28).min(1.0);
        } else {
            self.vertigo_zoom = false;
        }
        let to_edge = horiz(edge - self.feet).normalize_or_zero();
        let off = wrap(yaw_of(to_edge) - self.yaw).abs();
        let wish = forward(self.yaw) * input.move_axis.y + right(self.yaw) * input.move_axis.x;
        let over = wish.length_squared() > 1e-4 && wish.normalize().dot(to_edge) > 0.0;
        if off > rot(8000.0) || (t >= DISABLE_TIME && !over) {
            self.vertigo_zoom = false;
            self.vertigo_redo = REDO;
            self.state = State::Ground;
            return;
        }
        self.state = State::Vertigo { t, edge };
    }
}
