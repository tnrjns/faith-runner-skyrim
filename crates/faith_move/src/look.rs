//! Mirror's Edge's first-person camera rules that aren't animation:
//!
//! - **Look limits** per move (`TdMove_*.MinLookConstraint` / `MaxLookConstraint`):
//!   e.g. hanging on a ledge you can look down only ~18°, sliding ±55°.
//! - **Swan neck** (`TdSwanNeck`, `TdMove.SwanNeck*`): looking down past 15°
//!   slides the camera forward and down (quadratically, up to 35 cm / 30 cm),
//!   so you look over your chest instead of out from inside it.
//!
//! Angles from the game are in Unreal rotation units (65536 = 360°).

use std::f32::consts::{FRAC_PI_2, TAU};

use crate::controller::{Controller, State, Traverse, TraverseKind};

fn uu(r: i32) -> f32 {
    r as f32 / 65536.0 * TAU
}

/// Pitch range (down, up) and optional yaw half-range around the yaw the move
/// started with, in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookLimit {
    pub pitch: (f32, f32),
    pub yaw: Option<f32>,
}

/// The player controller's own limit when no move constrains the look.
const FREE: LookLimit = LookLimit { pitch: (-1.48, 1.48), yaw: None };

/// The game's constraint for the current move.
pub fn look_limit(c: &Controller) -> LookLimit {
    let l = |min_pitch: i32, max_pitch: i32, yaw: Option<i32>| LookLimit {
        pitch: (uu(min_pitch).max(-1.48), uu(max_pitch).min(1.48)),
        yaw: yaw.map(uu),
    };
    match c.state {
        // TdMove_Crouch
        State::Ground if c.crouched => l(-14000, 14000, None),
        State::Ground => FREE,
        // TdMove_Coil
        State::Air if c.is_coiled() => l(-5000, 30000, None),
        State::Air => FREE,
        // TdMove_Slide
        State::Slide { .. } => l(-10000, 10000, Some(10000)),
        // TdMove_SkillRoll
        State::Roll { .. } => l(-2000, 32768, Some(5000)),
        // TdMove_SoftLanding (closest to the hard-landing recovery)
        State::Stunned { .. } => l(-16384, 16384, Some(5000)),
        // TdMove_WallRun
        State::WallRun { .. } => l(-13000, 13000, Some(16384)),
        State::WallClimb { .. } | State::WallClimbTurned { .. } => FREE,
        // TdMove_Grab
        State::LedgeHang { .. } => l(-3200, 16000, None),
        // TdMove_GrabTransfer: look input ignored (DisableLookTime -1); held as the hang.
        State::GrabTransfer { .. } => l(-3200, 16000, None),
        State::Traverse(tr) => match tr.kind {
            // TdMove_SpeedVault / TdMove_StepUp
            TraverseKind::Vault | TraverseKind::Mantle => l(-3000, 6000, Some(8000)),
            // TdMove_GrabPullUp
            TraverseKind::PullUp => l(0, 16384, Some(10000)),
            TraverseKind::SpringBoard => FREE,
        },
        // TdMove_SpeedVault
        State::Vault(_) => l(-3000, 6000, Some(8000)),
        // TdMove_LayOnGround
        State::LayOnGround { .. } => l(-2000, 32768, Some(5000)),
        // TdMove_Barge
        State::Barge { .. } => l(-14000, 16384, Some(5000)),
        // TdMove_Stumble / TdMove_Landing (soft): look input ignored, levelled
        State::Stumble { .. } | State::SoftLand { .. } => l(-16384, 16384, Some(5000)),
        // TdMove_Balance
        State::Balance { .. } => l(-13000, 25000, Some(6000)),
        // TdMove_ZipLine (IntoZipLine constraints)
        State::ZipLine { .. } => l(-2500, 32768, Some(7000)),
        // TdMove_SwingJump's range (the swing itself doesn't clamp)
        State::Swing { .. } => l(-11000, 16384, None),
        // TdMOVE_Disarm: bConstrainLook off (the look input is ignored instead)
        State::Takedown { .. } => FREE,
        // DisableLookTime -1: the look is held instead (Controller::step).
        State::AirBarge { .. } => FREE,
        // TdMove_AutoStepUp (no constraint)
        State::StepUp { .. } => FREE,
        // TdMove_RumpSlide
        State::RumpSlide { .. } => l(-5000, 5000, Some(5000)),
        State::Vertigo { .. } => FREE,
        State::SwingJump { .. } => l(-11000, 16384, None),
        // TdMove_IntoClimb: look input ignored (DisableLookTime -1); TdMove_Climb.
        State::IntoClimb { .. } | State::ClimbExit { .. } => FREE,
        State::Climb { .. } => l(-5000, 10000, Some(32000)),
    }
}

/// Applies [`look_limit`] to the controller's view: eases into a new limit
/// over a moment (the game blends the view too), then holds it.
#[derive(Default)]
pub struct LookLimiter {
    kind: Option<std::mem::Discriminant<State>>,
    entry_yaw: f32,
    age: f32,
}

impl LookLimiter {
    pub fn apply(&mut self, c: &mut Controller, dt: f32) {
        let kind = std::mem::discriminant(&c.state);
        if self.kind != Some(kind) {
            self.kind = Some(kind);
            self.entry_yaw = c.yaw;
            self.age = 0.0;
        }
        self.age += dt;
        let lim = look_limit(c);
        let ease = |cur: f32, target: f32, age: f32| {
            if age > 0.3 { target } else { cur + (target - cur) * (1.0 - (-14.0 * dt).exp()) }
        };
        let (lo, hi) = lim.pitch;
        if c.pitch < lo {
            c.pitch = ease(c.pitch, lo, self.age);
        } else if c.pitch > hi {
            c.pitch = ease(c.pitch, hi, self.age);
        }
        if let Some(half) = lim.yaw {
            // TdMove_Slide's constraint is around the body, which turns during the slide.
            let base = if matches!(c.state, State::Slide { .. }) { c.slide_yaw } else { self.entry_yaw };
            let d = wrap(c.yaw - base);
            if d.abs() > half {
                let target = c.yaw - d + d.clamp(-half, half);
                c.yaw = ease(c.yaw, target, self.age);
            }
        }
    }
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI
}

/// Swan-neck settings for the current move (degrees, centimetres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwanNeckSettings {
    /// TdMove.SwanNeckEnableAtPitch: start translating past this much look-down.
    pub start_deg: f32,
    /// TdMove.SwanNeckForward / SwanNeckDown at full look-down.
    pub forward_cm: f32,
    pub down_cm: f32,
}

pub fn swan_neck_settings(c: &Controller) -> Option<SwanNeckSettings> {
    // TdMove defaults; each move sets its own on StartMove and the defaults come back on StopMove.
    let base = SwanNeckSettings { start_deg: 15.0, forward_cm: 35.0, down_cm: 30.0 };
    let off = SwanNeckSettings { start_deg: 0.0, forward_cm: 0.0, down_cm: 0.0 };
    match c.state {
        // TdMove_Grab: SwanNeckEnableAtPitch 0, SwanNeckForward 70
        State::LedgeHang { .. } => Some(SwanNeckSettings { start_deg: 0.0, forward_cm: 70.0, ..base }),
        // TdMove_Climb: SwanNeckForward 40
        State::Traverse(Traverse { kind: TraverseKind::PullUp, .. }) => {
            Some(SwanNeckSettings { forward_cm: 40.0, ..base })
        }
        // TdMove_180TurnInAir and TdMove_LayOnGround: SwanNeck 0, 0, 0
        State::LayOnGround { .. } => Some(off),
        State::Air if c.turned_in_air => Some(off),
        // TdMove_SkillRoll: a 0.2 s move timer calls DisableSwanneck (constraints 0, 0, 0)
        State::Roll { t } if t >= 0.2 => Some(off),
        _ => Some(base),
    }
}

/// The camera's swan-neck offset (TdSwanNeck, "quadratic" type, as TdPawn.CalcCamera applies it).
#[derive(Default)]
pub struct SwanNeck {
    forward: f32,
    down: f32,
}

impl SwanNeck {
    /// Returns (forward, down) in metres, in the view's yaw frame (GetSwanNeckPos takes
    /// the view rotation with pitch and roll zeroed).
    pub fn update(&mut self, c: &Controller, dt: f32) -> (f32, f32) {
        let s = swan_neck_settings(c).unwrap_or(SwanNeckSettings { start_deg: 0.0, forward_cm: 0.0, down_cm: 0.0 });
        // TdSwanNeck.GetSwanNeckTranslation: over the look-down range DownwardPitchWorld
        // (48151) to ForwardPitchWorld (65536), i.e. 0-95.5 degrees down, past the start
        // angle u goes 0..1; forward is F*u*cos(u*pi/4), down is D*u*sin(u*pi/4).
        let range_deg = (65536.0 - 48151.0) / 65536.0 * 360.0;
        let down_deg = (-c.pitch).to_degrees();
        let (f, d) = if down_deg > 0.0 && down_deg < range_deg {
            let u = ((down_deg - s.start_deg) / (range_deg - s.start_deg)).max(0.0);
            let a = u * std::f32::consts::FRAC_PI_4;
            (s.forward_cm * u * a.cos() * 0.01, s.down_cm * u * a.sin() * 0.01)
        } else {
            (0.0, 0.0)
        };
        // UpdateSwanNeck: move a fraction dt/0.07 of the way each frame, never stepping
        // further out than the constraint in one go (FMin, so moving back is uncapped).
        let k = (dt / 0.07).min(1.0);
        self.forward += ((f - self.forward) * k).min(s.forward_cm * 0.01);
        self.down += ((d - self.down) * k).min(s.down_cm * 0.01);
        (self.forward, self.down)
    }
}

/// Upper bound on looking down at all (the controller's own clamp).
pub const MAX_LOOK_DOWN: f32 = -FRAC_PI_2 + 0.09;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Tuning, Input};
    use glam::{Vec2, Vec3};

    #[test]
    fn ledge_hang_limits_look_down() {
        let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
        c.state = State::LedgeHang { normal: Vec3::Z, ledge_y: 1.8, turned: false };
        c.pitch = -1.2;
        let mut l = LookLimiter::default();
        for _ in 0..60 {
            l.apply(&mut c, 1.0 / 60.0);
        }
        assert!((c.pitch - uu(-3200)).abs() < 1e-3, "{}", c.pitch);
    }

    #[test]
    fn wallrun_limits_yaw_around_entry() {
        let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
        c.state = State::WallRun { normal: Vec3::X, t: 0.0 };
        let mut l = LookLimiter::default();
        l.apply(&mut c, 0.01);
        c.yaw = 2.5; // way past ±90°
        for _ in 0..60 {
            l.apply(&mut c, 1.0 / 60.0);
        }
        assert!((c.yaw - uu(16384)).abs() < 1e-3, "{}", c.yaw);
        let _ = (Input::default(), Vec2::ZERO);
    }

    #[test]
    fn swan_neck_moves_camera_forward_when_looking_down() {
        let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
        c.state = State::Ground;
        let mut s = SwanNeck::default();
        c.pitch = -0.1; // under 15°: nothing
        let (f0, _) = (0..120).fold((0.0, 0.0), |_, _| s.update(&c, 1.0 / 60.0));
        assert!(f0 < 1e-3, "{f0}");
        c.pitch = -1.45; // nearly straight down
        let (f1, d1) = (0..120).fold((0.0, 0.0), |_, _| s.update(&c, 1.0 / 60.0));
        // TdSwanNeck at 83° down: u = (83.1 - 15) / (95.5 - 15) = 0.846, so forward
        // 35 * u * cos(u * pi/4) = 23 cm and down 30 * u * sin(u * pi/4) = 16 cm.
        let u = ((1.45f32).to_degrees() - 15.0) / ((65536.0 - 48151.0) / 65536.0 * 360.0 - 15.0);
        let a = u * std::f32::consts::FRAC_PI_4;
        let (want_f, want_d) = (0.35 * u * a.cos(), 0.30 * u * a.sin());
        assert!((f1 - want_f).abs() < 0.005 && (d1 - want_d).abs() < 0.005, "{f1} {d1} want {want_f} {want_d}");
        assert!((f1 - 0.233).abs() < 0.005 && (d1 - 0.156).abs() < 0.005, "{f1} {d1}");
    }

    #[test]
    fn skill_roll_turns_the_swan_neck_off() {
        let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
        c.pitch = -1.2;
        let mut s = SwanNeck::default();
        let (f0, _) = (0..60).fold((0.0, 0.0), |_, _| s.update(&c, 1.0 / 60.0));
        assert!(f0 > 0.1, "{f0}");
        c.state = State::Roll { t: 0.1 };
        let (f1, _) = s.update(&c, 1.0 / 60.0);
        assert!((f1 - f0).abs() < 1e-3, "still on before 0.2 s: {f1}");
        c.state = State::Roll { t: 0.3 };
        let (f2, d2) = (0..60).fold((0.0, 0.0), |_, _| s.update(&c, 1.0 / 60.0));
        assert!(f2 < 1e-3 && d2 < 1e-3, "{f2} {d2}");
    }
}
