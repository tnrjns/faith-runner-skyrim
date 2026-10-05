//! Mirror's Edge's ladders and drainpipes (TdLadderVolume, TdMove_IntoClimb, TdMove_Climb).
//!
//! - **The ladder** (TdLadderVolume): pawn positions one StepHeight (32 uu) apart, XYOffset
//!   (ladder 50 uu, pipe 62 uu) out from it, a pipe's ZOffset (-5 uu) lower. The game's level
//!   designers place the steps; here they start standing height above the ladder's foot and the
//!   last is the one the exit clip climbs out of onto `top` (its root rises 95 uu off a ladder,
//!   57 off a pipe, which carries on above the roof).
//! - **Getting on** (TdLadderVolume.PawnUpdate, TdMove_IntoClimb.CanDoMove): touching it, not
//!   pushing down, and pushing up if walking. Above the top step, a ladder (not a pipe) facing out
//!   from it (dot < -0.85): over the top, `LadderEnterTop` (root motion) round onto it. Otherwise
//!   in front of it facing it (dot > -0.1): onto the closest step at max(2D distance, 30) / 0.15
//!   uu/s, turning to it, with the start clip by how she came (PlayStartAnimation): off a wallrun
//!   `...ClimbHangStartLeft/Right`, falling faster than 800 uu/s `...HangStartHard`, than 200
//!   `...HangStart`, else a pipe's `PipeClimbStart`. Move input is ignored for the start clip
//!   less 0.5 s (at least 0.1). RedoMoveTime 0.5 s after letting go.
//! - **Climbing** (TdMove_Climb.HandleClimbAction / Climb): up one step at 96 uu/s
//!   (`LadderClimbUp<hand>`), a pipe two at a time at 64 uu/s a step while it can
//!   (`PipeClimbUpFast<hand>`); down the same played backwards, off the bottom when there's floor
//!   under her. The hand swaps 0.1 s into each. Pushing right down (full stick) above step 4
//!   slides her down (`...ClimbDownFast`) at ClimbDownFastVelocity (200 uu/s) until let go of,
//!   then onto the step below at 100 uu/s, or off with floor under her. Jump lets go
//!   (`PipeExitBottom`, in 0.1, out 0.4: walking with floor within 32 uu, else falling).
//! - **Over the top** (ExitAtTop): pushing up on the last step, `LadderExitTop<hand>` /
//!   `PipeExitTop<hand>` with root motion; at its end walking, at 300 uu/s if still pushing up.
//!
//! The volumes' touch region (designer brushes) is taken as 80 uu in front of the steps to 1 m
//! back over the top, 50 uu either side.

use glam::Vec3;

use crate::controller::{forward, horiz, Controller, Event, Input, State};
use crate::world::{Body, World};

const fn uu(v: f32) -> f32 {
    v / 100.0
}

/// A ladder or drainpipe up a wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ladder {
    /// Its foot, on the wall's face, where it meets the floor.
    pub base: Vec3,
    /// The height of the top she climbs out onto.
    pub top: f32,
    /// Out from the wall (horizontal): where she climbs from.
    pub normal: Vec3,
    pub pipe: bool,
    /// Whether she can climb out over the top (TdLadderVolume.bCanExitAtTop): not where the wall
    /// carries on above the ladder.
    pub exit: bool,
}

/// TdLadderVolume.StepHeight.
pub const STEP: f32 = uu(32.0);
/// Faith's CollisionHeight: her middle above her feet.
const HALF: f32 = uu(90.0);

impl Ladder {
    fn offsets(&self) -> (f32, f32) {
        if self.pipe { (uu(62.0), uu(-5.0)) } else { (uu(50.0), 0.0) }
    }

    /// GetLastStep: the step the exit clip climbs out of onto the top (`exit_up`: its root's
    /// rise); the steps are counted down from it.
    pub fn last_step(&self, exit_up: f32) -> i32 {
        let (_, z) = self.offsets();
        let lowest = self.base.y + HALF + z;
        (((self.top + HALF - exit_up) - lowest) / STEP).floor().max(0.0) as i32
    }

    /// GetLadderLocation: her middle on step `i`.
    pub fn location(&self, i: i32, exit_up: f32) -> Vec3 {
        let (xy, _) = self.offsets();
        let last = self.last_step(exit_up);
        let y = self.top + HALF - exit_up - STEP * (last - i) as f32;
        Vec3::new(self.base.x, y, self.base.z) + self.normal * xy
    }

    /// GetClosestStep.
    pub fn closest_step(&self, centre_y: f32, exit_up: f32) -> i32 {
        let last = self.last_step(exit_up);
        (((centre_y - self.location(0, exit_up).y) / STEP).round() as i32).clamp(0, last)
    }

    /// What to draw (no collision): rods as (from, to, thickness). A ladder's rails stand a
    /// metre above the top, its rungs a step apart; a pipe runs up past the top.
    pub fn rods(&self) -> Vec<(Vec3, Vec3, f32)> {
        let off = self.base + self.normal * 0.08;
        if self.pipe {
            return vec![(off, Vec3::new(off.x, self.top + 1.0, off.z), 0.1)];
        }
        let side = Vec3::Y.cross(self.normal).normalize_or_zero() * 0.22;
        let top = self.top + 1.0;
        let mut out = vec![
            (off - side, Vec3::new(off.x, top, off.z) - side, 0.05),
            (off + side, Vec3::new(off.x, top, off.z) + side, 0.05),
        ];
        let mut y = self.base.y + STEP * 0.5;
        while y < top {
            let p = Vec3::new(off.x, y, off.z);
            out.push((p - side, p + side, 0.035));
            y += STEP;
        }
        out
    }

    /// Is her cylinder (feet `feet`, radius `r`) in the volume?
    pub fn touches(&self, feet: Vec3, r: f32) -> bool {
        let d = feet - self.base;
        let out = d.dot(self.normal);
        let along = d.dot(Vec3::Y.cross(self.normal));
        out > -1.0 - r && out < uu(80.0) + r && along.abs() < uu(50.0) + r && feet.y > self.base.y - 0.1 && feet.y < self.top + 2.0
    }
}

/// How she got on (TdMove_IntoClimb.PlayStartAnimation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClimbStart {
    /// Walking up to a ladder: no clip.
    None,
    /// A pipe from standing: PipeClimbStart.
    PipeStart,
    /// Falling faster than 200 uu/s: ...ClimbHangStart.
    Hang,
    /// Faster than 800 uu/s: ...ClimbHangStartHard.
    HangHard,
    /// Off a wallrun with the wall on her left / right.
    HangLeft,
    HangRight,
    /// Over the top: LadderEnterTop.
    EnterTop,
}

impl ClimbStart {
    /// The clip's length (the game's assets), for SetIgnoreMoveInput.
    fn clip_length(self, pipe: bool) -> f32 {
        match self {
            ClimbStart::HangLeft | ClimbStart::HangRight if pipe => 1.2,
            _ => 1.0,
        }
    }
}

/// A step up or down the ladder (Climb): her middle from `from` to `to` over `dur`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClimbStep {
    pub t: f32,
    pub from: f32,
    pub to: f32,
    pub dur: f32,
    /// ClimbAnims index (0 up left hand, 1 right, 2 / 3 a pipe's fast ones), None for the move
    /// back onto a step after sliding down.
    pub anim: Option<u8>,
    pub down: bool,
}

/// Root motion curves of the climb clips (side, up, forward in metres from the clip's start,
/// every 1/60 s), from the host's animations.
#[derive(Clone, Debug, Default)]
pub struct ClimbCurves {
    /// LadderExitTopLeftHand / RightHand, PipeExitTopLeftHand / RightHand.
    pub exit_ladder: [Vec<Vec3>; 2],
    pub exit_pipe: [Vec<Vec3>; 2],
    pub enter_top: Vec<Vec3>,
}

impl ClimbCurves {
    pub const RATE: f32 = 60.0;
}

/// Without the clips: their root's travel (side, up, forward) and length.
fn fallback(kind: u8) -> (Vec3, f32) {
    match kind {
        0 => (Vec3::new(0.0, uu(95.2), uu(78.9)), 1.53),
        1 => (Vec3::new(0.0, uu(57.5), uu(201.4)), 1.83),
        _ => (Vec3::new(0.0, uu(-128.0), uu(94.2)), 1.67),
    }
}

fn sample(curve: &[Vec3], t: f32, kind: u8) -> (Vec3, bool) {
    if curve.len() < 2 {
        let (end, len) = fallback(kind);
        let k = (t / len).clamp(0.0, 1.0);
        return (end * k, t >= len);
    }
    let f = t * ClimbCurves::RATE;
    let i = f.floor() as usize;
    if i + 1 >= curve.len() {
        return (*curve.last().unwrap(), true);
    }
    (curve[i].lerp(curve[i + 1], f - i as f32), false)
}

fn yaw_of(v: Vec3) -> f32 {
    (-v.x).atan2(-v.z)
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

impl Controller {
    fn exit_curve(&self, l: &Ladder, left: bool) -> Vec<Vec3> {
        let c = self.tuning.climb_curves.as_ref();
        let i = if left { 0 } else { 1 };
        c.map(|c| if l.pipe { c.exit_pipe[i].clone() } else { c.exit_ladder[i].clone() }).unwrap_or_default()
    }

    fn exit_up(&self, l: &Ladder) -> f32 {
        let curve = self.exit_curve(l, true);
        curve.last().map_or(fallback(if l.pipe { 1 } else { 0 }).0.y, |v| v.y)
    }

    pub(crate) fn last_step(&self, l: &Ladder) -> i32 {
        l.last_step(self.exit_up(l))
    }

    fn step_at(&self, l: &Ladder, i: i32) -> Vec3 {
        l.location(i, self.exit_up(l))
    }

    fn closest(&self, l: &Ladder, y: f32) -> i32 {
        l.closest_step(y, self.exit_up(l))
    }

    /// TdLadderVolume.PawnUpdate and TdMove_IntoClimb.CanDoMove / StartMove.
    pub(crate) fn try_into_climb(&mut self, input: &Input, world: &dyn World) -> bool {
        if self.climb_redo > 0.0 || input.move_axis.y < -0.3 {
            return false;
        }
        let walking = self.state == State::Ground;
        if walking && input.move_axis.y <= 0.3 {
            return false;
        }
        let r = self.tuning.half_width;
        let Some(l) = world.fixtures().iter().find_map(|f| match *f {
            crate::world::Fixture::Ladder(l) if l.touches(self.feet, r) => Some(l),
            _ => None,
        }) else {
            return false;
        };
        let into = -l.normal;
        let angle = forward(self.yaw).dot(into);
        let last = self.last_step(&l);
        let centre = self.centre();
        let above = centre.y > self.step_at(&l, last).y;
        if above {
            if angle > -0.85 || l.pipe {
                return false;
            }
        } else if angle < -0.1 || (self.feet - l.base).dot(l.normal) < 0.0 {
            return false;
        }
        self.crouched = false;
        let vy = self.vel.y;
        self.vel = Vec3::ZERO;
        if above {
            // Onto the top: in over it, then LadderEnterTop.
            let to = self.step_at(&l, last) - l.normal * uu(93.5) + Vec3::Y * uu(90.0) - Vec3::Y * HALF;
            let dur = (to - self.feet).length() / uu(500.0);
            self.state = State::IntoClimb { t: 0.0, ladder: l, from: self.feet, to, dur, start: ClimbStart::EnterTop, entering: false };
            return true;
        }
        let start = match self.state {
            State::WallRun { normal, .. } => {
                if normal.dot(crate::controller::right(self.yaw)) > 0.0 { ClimbStart::HangLeft } else { ClimbStart::HangRight }
            }
            _ if vy < -uu(800.0) => ClimbStart::HangHard,
            _ if vy < -uu(200.0) => ClimbStart::Hang,
            _ if l.pipe => ClimbStart::PipeStart,
            _ => ClimbStart::None,
        };
        let step = self.closest(&l, centre.y);
        let to = self.step_at(&l, step) - Vec3::Y * HALF;
        let d2 = horiz(to - self.feet).length().max(uu(30.0));
        let dur = (to - self.feet).length() / (d2 / 0.15);
        self.state = State::IntoClimb { t: 0.0, ladder: l, from: self.feet, to, dur, start, entering: false };
        self.events.push(Event::ClimbStart { start, pipe: l.pipe });
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn into_climb(&mut self, dt: f32, t: f32, l: Ladder, from: Vec3, to: Vec3, dur: f32, start: ClimbStart, entering: bool) {
        let t = t + dt;
        self.vel = Vec3::ZERO;
        if start == ClimbStart::EnterTop && entering {
            // LadderEnterTop's root motion, from facing out over the top.
            let curve = self.tuning.climb_curves.as_ref().map(|c| c.enter_top.clone()).unwrap_or_default();
            let (m, done) = sample(&curve, t, 2);
            self.feet = from + Vec3::Y * m.y + l.normal * m.z;
            if done {
                self.begin_climb(l, false, 0.0);
            } else {
                self.state = State::IntoClimb { t, ladder: l, from, to, dur, start, entering };
            }
            return;
        }
        let k = if dur > 1e-4 { (t / dur).min(1.0) } else { 1.0 };
        self.feet = from.lerp(to, k);
        if start != ClimbStart::EnterTop {
            // SetPreciseRotation to face it over 0.15 s (and ResetCameraLook).
            self.yaw += wrap(yaw_of(-l.normal) - self.yaw) * (dt / 0.15).min(1.0);
        }
        if k < 1.0 {
            self.state = State::IntoClimb { t, ladder: l, from, to, dur, start, entering };
            return;
        }
        if start == ClimbStart::EnterTop {
            self.state = State::IntoClimb { t: 0.0, ladder: l, from: self.feet, to, dur, start, entering: true };
            self.events.push(Event::ClimbStart { start, pipe: l.pipe });
            return;
        }
        let hold = match start {
            ClimbStart::None => 0.0,
            s => (s.clip_length(l.pipe) - 0.5).max(0.1),
        };
        self.begin_climb(l, false, hold);
    }

    /// StartClimbMove: on the closest step, facing it, climbing.
    fn begin_climb(&mut self, l: Ladder, left: bool, hold: f32) {
        let step = self.closest(&l, self.centre().y);
        self.feet = self.step_at(&l, step) - Vec3::Y * HALF;
        self.yaw = yaw_of(-l.normal);
        self.vel = Vec3::ZERO;
        self.state = State::Climb { ladder: l, step, mv: None, left, fast: false, hold };
    }

    /// LetGo: walking with floor within 32 uu under her, else falling; PipeExitBottom.
    fn climb_let_go(&mut self, world: &dyn World) {
        let body = Body { half_width: self.tuning.half_width, height: uu(16.0) };
        let half = body.aabb(Vec3::ZERO).size() * 0.5;
        let start = self.feet + Vec3::Y * half.y;
        let floor = world.sweep(half, start, Vec3::new(0.0, -uu(32.0), 0.0));
        if let Some(h) = floor {
            self.feet.y -= uu(32.0) * h.t;
            self.state = State::Ground;
        } else {
            self.state = State::Air;
            self.air_time = 0.0;
        }
        self.vel = Vec3::ZERO;
        self.climb_redo = 0.5;
        self.events.push(Event::ClimbLetGo);
    }

    /// HasFloorBelow (20 uu under the step; the steps here count down from the top, so the
    /// bottom one can be up to a step above the floor: two steps and 20 uu).
    fn floor_below_step(&self, world: &dyn World) -> bool {
        let body = Body { half_width: self.tuning.half_width * 0.5, height: 0.05 };
        let half = body.aabb(Vec3::ZERO).size() * 0.5;
        world.sweep(half, self.feet + Vec3::Y * 0.05, Vec3::new(0.0, -(2.0 * STEP + uu(20.0)) - 0.05, 0.0)).is_some()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn climb(&mut self, dt: f32, l: Ladder, mut step: i32, mut mv: Option<ClimbStep>, mut left: bool, mut fast: bool, hold: f32, input: &Input, world: &dyn World) {
        let hold = (hold - dt).max(0.0);
        let last = self.last_step(&l);
        self.vel = Vec3::ZERO;
        let (up, down, full_down) = if hold > 0.0 {
            (false, false, false)
        } else {
            (input.move_axis.y > 0.3, input.move_axis.y < -0.3, input.move_axis.y < -0.96)
        };
        // HandleMoveAction(MA_Jump): let go.
        if self.jump_buffer > 0.0 {
            self.jump_buffer = 0.0;
            self.climb_let_go(world);
            return;
        }
        let xz = self.step_at(&l, 0);
        let mut y = self.centre().y;
        if let Some(mut m) = mv {
            let was = m.t;
            m.t += dt;
            // OnTimer: the hand swaps 0.1 s in.
            if m.anim.is_some() && was < 0.1 && m.t >= 0.1 {
                left = !left;
            }
            let k = if m.dur > 1e-4 { (m.t / m.dur).min(1.0) } else { 1.0 };
            y = m.from + (m.to - m.from) * k;
            if k >= 1.0 {
                step = self.closest(&l, y);
                mv = None;
            } else {
                mv = Some(m);
            }
        } else if fast {
            // Sliding down: ClimbDownFastVelocity.
            y -= uu(200.0) * dt;
            let bottom = self.step_at(&l, 0).y;
            if y <= bottom {
                // Landed.
                self.feet = Vec3::new(xz.x, bottom - HALF, xz.z);
                self.climb_let_go(world);
                return;
            }
            if !full_down {
                fast = false;
                self.feet = Vec3::new(xz.x, y - HALF, xz.z);
                if self.floor_below_step(world) {
                    self.climb_let_go(world);
                    return;
                }
                // StopClimbDownFast: onto the step below at 100 uu/s.
                let below = (((y - bottom) / STEP).floor() as i32).clamp(0, last);
                let to = self.step_at(&l, below).y;
                mv = Some(ClimbStep { t: 0.0, from: y, to, dur: (y - to).abs() / uu(100.0), anim: None, down: true });
                self.events.push(Event::ClimbSlideEnd);
            }
        } else if full_down && step > 4 {
            fast = true;
            self.events.push(Event::ClimbSlide);
        } else if up {
            if step >= last && !l.exit {
                // bCanExitAtTop off: holding on at the top.
                return;
            } else if step >= last {
                // ExitAtTop (bClimbLeftHand ? RightHand : LeftHand).
                let exit_left = !left;
                self.state = State::ClimbExit { ladder: l, t: 0.0, from: self.feet, left: exit_left };
                self.events.push(Event::ClimbExit);
                return;
            }
            let (anim, n) = if l.pipe && last - step > 1 { (if left { 3 } else { 2 }, 2) } else { (if left { 1 } else { 0 }, 1) };
            let speed = n as f32 * if l.pipe { uu(64.0) } else { uu(96.0) };
            let to = self.step_at(&l, step + n).y;
            mv = Some(ClimbStep { t: 0.0, from: y, to, dur: (to - y).abs() / speed, anim: Some(anim), down: false });
            self.events.push(Event::ClimbStep);
        } else if down {
            if step > 1 {
                let (anim, n) = if l.pipe && step > 2 { (if left { 2 } else { 3 }, 2) } else { (if left { 0 } else { 1 }, 1) };
                let speed = n as f32 * if l.pipe { uu(64.0) } else { uu(96.0) };
                let to = self.step_at(&l, step - n).y;
                mv = Some(ClimbStep { t: 0.0, from: y, to, dur: (to - y).abs() / speed, anim: Some(anim), down: true });
                self.events.push(Event::ClimbStep);
            } else if self.floor_below_step(world) {
                self.climb_let_go(world);
                return;
            }
        }
        self.feet = Vec3::new(xz.x, y - HALF, xz.z);
        self.state = State::Climb { ladder: l, step, mv, left, fast, hold };
    }

    /// ExitAtTop's root motion, then walking (OnCeaseRelevantRootMotion).
    pub(crate) fn climb_exit(&mut self, dt: f32, l: Ladder, t: f32, from: Vec3, left: bool, input: &Input, world: &dyn World) {
        let t = t + dt;
        let curve = self.exit_curve(&l, left);
        let (m, done) = sample(&curve, t, if l.pipe { 1 } else { 0 });
        self.feet = from + Vec3::Y * m.y - l.normal * m.z;
        self.vel = Vec3::ZERO;
        if !done {
            self.state = State::ClimbExit { ladder: l, t, from, left };
            return;
        }
        if input.move_axis.y > 0.3 {
            self.vel = -l.normal * uu(300.0);
        }
        self.climb_redo = 0.5;
        self.state = if self.grounded(world) { State::Ground } else { State::Air };
        self.air_time = 0.0;
    }
}
