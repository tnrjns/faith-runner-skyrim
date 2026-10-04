//! Camera feel: bob, sway, tilt, landing kicks, screen shake and FOV.
//!
//! The controller hands back a plain eye position and look angles
//! ([`Controller::view`]); [`CameraFx`] layers everything that makes it feel
//! like a body is carrying the camera. Kept separate so a host engine can use
//! the controller with its own camera if it wants.

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::controller::{Controller, Event, Input, State, TraverseKind, View};

#[derive(Clone, Debug)]
pub struct CameraFxSettings {
    /// Head bob, metres/degrees at full sprint.
    pub bob_vertical: f32,
    pub bob_lateral: f32,
    pub bob_roll_deg: f32,
    /// Footsteps per second = base + per_speed × speed (m/s).
    pub step_rate_base: f32,
    pub step_rate_per_speed: f32,
    /// Tilt into strafes and sharp turns.
    pub strafe_lean_deg: f32,
    pub turn_lean_deg: f32,
    /// Tilt away from the wall while wallrunning; lean while sliding.
    pub wallrun_roll_deg: f32,
    pub slide_roll_deg: f32,
    /// Screen shake at full trauma.
    pub shake_pitch_deg: f32,
    pub shake_yaw_deg: f32,
    pub shake_roll_deg: f32,
    pub shake_offset: f32,
    /// Trauma lost per second.
    pub trauma_decay: f32,
    /// Constant low shake at full sprint (wind, footfalls).
    pub sprint_shake: f32,
    pub slide_shake: f32,
    pub fov_base: f32,
    pub fov_sprint_add: f32,
    pub fov_wallrun_add: f32,
    /// Forward somersault on a skill roll, as in Mirror's Edge.
    pub roll_flip: bool,
    /// Tilt the view up toward the wall top while climbing/hanging.
    pub look_up_assist: bool,
    /// Scale for landing dips/kicks (0 disables).
    pub landing_kick: f32,
}

impl CameraFxSettings {
    /// For when the camera motion comes from the character's own animation
    /// (the real Mirror's Edge arms drive bob, tilt, dips and the roll):
    /// only screen shake and a light landing kick are left procedural.
    pub fn animation_driven() -> Self {
        Self {
            bob_vertical: 0.0,
            bob_lateral: 0.0,
            bob_roll_deg: 0.0,
            strafe_lean_deg: 0.0,
            turn_lean_deg: 0.0,
            wallrun_roll_deg: 0.0,
            slide_roll_deg: 0.0,
            sprint_shake: 0.08,
            slide_shake: 0.15,
            fov_sprint_add: 0.0,
            fov_wallrun_add: 0.0,
            roll_flip: false,
            look_up_assist: false,
            landing_kick: 0.5,
            ..Self::default()
        }
    }
}

impl Default for CameraFxSettings {
    fn default() -> Self {
        Self {
            bob_vertical: 0.045,
            bob_lateral: 0.03,
            bob_roll_deg: 0.9,
            step_rate_base: 1.2,
            step_rate_per_speed: 0.2,
            strafe_lean_deg: 1.6,
            turn_lean_deg: 2.0,
            wallrun_roll_deg: 12.0,
            slide_roll_deg: -5.0,
            shake_pitch_deg: 3.0,
            shake_yaw_deg: 2.2,
            shake_roll_deg: 4.0,
            shake_offset: 0.04,
            trauma_decay: 1.5,
            sprint_shake: 0.22,
            slide_shake: 0.3,
            fov_base: 90.0,
            fov_sprint_add: 10.0,
            fov_wallrun_add: 4.0,
            roll_flip: true,
            look_up_assist: true,
            landing_kick: 1.0,
        }
    }
}

/// Final camera for this frame, plus timing the viewmodel can sync to.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shot {
    pub view: View,
    /// Footstep phase in radians; one full cycle is two steps (left + right).
    pub step_phase: f32,
    /// 0..1: how much the body is in its running gait right now.
    pub gait: f32,
    /// Current shake strength 0..1, after squaring.
    pub shake: f32,
}

#[derive(Default)]
struct Spring {
    x: f32,
    v: f32,
}

impl Spring {
    /// Semi-implicit Euler in small substeps, so long frames (hitches, slow
    /// machines) can't make the spring explode.
    fn step(&mut self, dt: f32, stiffness: f32, damping: f32) -> f32 {
        let mut left = dt.min(0.25);
        while left > 0.0 {
            let h = left.min(1.0 / 240.0);
            self.v += (-stiffness * self.x - damping * self.v) * h;
            self.x += self.v * h;
            left -= h;
        }
        self.x
    }
}

pub struct CameraFx {
    pub settings: CameraFxSettings,
    time: f32,
    step_phase: f32,
    gait: f32,
    roll: f32,
    lean: Spring,
    dip: Spring,
    pitch_kick: Spring,
    trauma: f32,
    fov: f32,
    last_yaw: f32,
    yaw_rate: f32,
    flip: Option<f32>,
    /// Extra look-up while climbing/hanging, so the wall top and your hands are in view.
    assist: f32,
    /// TdPlayerController's zoom (TdMove_Vertigo's StartZoom / UnZoom), degrees off the FOV.
    zoom: f32,
}

impl Default for CameraFx {
    fn default() -> Self {
        Self::new(CameraFxSettings::default())
    }
}

/// Smooth, cheap pseudo-noise in about [-1, 1].
fn noise(t: f32, seed: f32) -> f32 {
    0.5 * (t * 23.0 + seed).sin() + 0.3 * (t * 37.3 + seed * 1.7).sin() + 0.2 * (t * 53.1 + seed * 2.3).sin()
}

fn approach(cur: f32, target: f32, rate: f32, dt: f32) -> f32 {
    cur + (target - cur) * (1.0 - (-rate * dt).exp())
}

impl CameraFx {
    pub fn new(settings: CameraFxSettings) -> Self {
        let fov = settings.fov_base;
        Self {
            settings,
            time: 0.0,
            step_phase: 0.0,
            gait: 0.0,
            roll: 0.0,
            lean: Spring::default(),
            dip: Spring::default(),
            pitch_kick: Spring::default(),
            trauma: 0.0,
            fov,
            last_yaw: 0.0,
            yaw_rate: 0.0,
            flip: None,
            assist: 0.0,
            zoom: 0.0,
        }
    }

    /// Add screen shake (0..1). Big events stack up to 1.
    pub fn add_trauma(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount).min(1.0);
    }

    /// Call once per frame after [`Controller::step`].
    pub fn update(&mut self, dt: f32, c: &Controller, input: &Input) -> Shot {
        let dt = dt.clamp(0.0, 0.1);
        let s = self.settings.clone();
        self.time += dt;
        let base = c.view();
        let tu = &c.tuning;
        let speed = c.horizontal_speed();
        let sprint_frac = ((speed - tu.run_speed) / (tu.sprint_speed - tu.run_speed)).clamp(0.0, 1.0);

        // Yaw rate, ignoring the 180 turn (that's a deliberate spin, not a lean).
        let dyaw = base.yaw - self.last_yaw;
        self.last_yaw = base.yaw;
        let raw_rate = if dt > 0.0 { dyaw / dt } else { 0.0 };
        self.yaw_rate = approach(self.yaw_rate, raw_rate.clamp(-12.0, 12.0), 10.0, dt);

        // ---- events → kicks and trauma
        for e in &c.events {
            match *e {
                Event::Jump => self.add_trauma(0.03),
                Event::Land { impact, .. } => {
                    self.dip.v -= s.landing_kick * (impact * 0.07).min(1.8);
                    self.pitch_kick.v -= s.landing_kick * (impact * 0.06).min(1.2);
                    self.add_trauma(((impact - 5.0) * 0.05).clamp(0.0, 0.35));
                }
                Event::HardLand => {
                    self.dip.v -= 2.4 * s.landing_kick;
                    self.pitch_kick.v -= 3.0 * s.landing_kick;
                    self.add_trauma(0.8);
                }
                Event::Roll => {
                    self.add_trauma(0.3);
                    if s.roll_flip {
                        self.flip = Some(0.0);
                    }
                }
                Event::WallRunStart => self.add_trauma(0.12),
                Event::WallJump => self.add_trauma(0.15),
                Event::WallClimbStart => {
                    self.add_trauma(0.12);
                    self.pitch_kick.v += 0.5;
                }
                Event::WallKick => self.add_trauma(0.25),
                Event::LedgeGrab => {
                    self.add_trauma(0.35);
                    self.dip.v -= 1.0;
                    self.pitch_kick.v -= 0.4;
                }
                Event::PullUp | Event::Mantle => self.add_trauma(0.1),
                Event::Vault => {
                    self.add_trauma(0.12);
                    self.dip.v += 0.5;
                }
                Event::Slide => self.add_trauma(0.18),
                Event::Dodge { dir } | Event::WallRunDodge { dir } | Event::WallClimbDodge { dir } => {
                    self.add_trauma(0.15);
                    // Lean into the dodge.
                    let right = Vec3::new(base.yaw.cos(), 0.0, -base.yaw.sin());
                    self.lean.v += dir.dot(right) * 0.9;
                }
                Event::Death => {
                    self.trauma = 0.0;
                    self.flip = None;
                }
                _ => {}
            }
        }

        // ---- gait and footstep phase
        let running = matches!(c.state, State::Ground) && speed > 0.5;
        self.gait = approach(self.gait, if running { 1.0 } else { 0.0 }, 8.0, dt);
        if running {
            self.step_phase = (self.step_phase + dt * PI * (s.step_rate_base + s.step_rate_per_speed * speed)) % TAU;
        }
        let amp = self.gait * (speed / tu.sprint_speed).min(1.0).sqrt();
        let bob_y = s.bob_vertical * amp * (2.0 * self.step_phase).sin();
        let bob_x = s.bob_lateral * amp * self.step_phase.sin();
        let bob_roll = s.bob_roll_deg.to_radians() * amp * self.step_phase.sin();

        // ---- tilt
        let right = Vec3::new(base.yaw.cos(), 0.0, -base.yaw.sin());
        let state_roll = match c.state {
            State::WallRun { normal, .. } => normal.dot(right) * s.wallrun_roll_deg.to_radians(),
            State::Slide { .. } => s.slide_roll_deg.to_radians(),
            _ => 0.0,
        };
        let steer_roll = if matches!(c.state, State::Ground | State::Air) {
            input.move_axis.x * s.strafe_lean_deg.to_radians()
                - (self.yaw_rate / 8.0).clamp(-1.0, 1.0) * s.turn_lean_deg.to_radians()
        } else {
            0.0
        };
        self.roll = approach(self.roll, state_roll + steer_roll, 8.0, dt);
        let lean = self.lean.step(dt, 60.0, 11.0);

        // ---- springs
        let dip = self.dip.step(dt, 120.0, 17.0).clamp(-0.45, 0.2);
        let pitch_kick = self.pitch_kick.step(dt, 110.0, 16.0).clamp(-0.35, 0.2);

        // ---- shake
        self.trauma = (self.trauma - s.trauma_decay * dt).max(0.0);
        let floor = match c.state {
            State::Slide { .. } => s.slide_shake,
            State::Ground => s.sprint_shake * sprint_frac * sprint_frac,
            State::WallRun { .. } => s.sprint_shake * 0.8,
            _ => 0.0,
        };
        let shake = (self.trauma * self.trauma).max(floor * floor);
        let t = self.time;
        let sh_pitch = s.shake_pitch_deg.to_radians() * shake * noise(t, 1.0);
        let sh_yaw = s.shake_yaw_deg.to_radians() * shake * noise(t, 7.0);
        let sh_roll = s.shake_roll_deg.to_radians() * shake * noise(t, 13.0);
        let sh_off = Vec3::new(noise(t, 21.0), noise(t, 29.0), 0.0) * s.shake_offset * shake;

        // ---- skill-roll somersault
        let mut flip_pitch = 0.0;
        let mut flip_drop = 0.0;
        if let Some(f) = &mut self.flip {
            *f += dt / 0.5;
            let k = f.clamp(0.0, 1.0);
            let e = k * k * (3.0 - 2.0 * k);
            flip_pitch = -TAU * e;
            flip_drop = -0.5 * (PI * k).sin();
            if *f >= 1.0 {
                self.flip = None;
            }
        }

        // ---- look-up assist on walls (ME constrains/raises the view here too)
        let assist_target = match c.state {
            State::WallClimb { .. } => 0.5,
            State::WallClimbTurned { .. } => 0.15,
            State::LedgeHang { turned: false, .. } => 0.42,
            State::Traverse(tr) if tr.kind == TraverseKind::PullUp => 0.42 * (1.0 - tr.t).max(0.0),
            _ => 0.0,
        };
        let assist_target = if s.look_up_assist { assist_target } else { 0.0 };
        self.assist = approach(self.assist, assist_target, 7.0, dt);

        // ---- FOV
        let target_fov = s.fov_base
            + s.fov_sprint_add * sprint_frac
            + match c.state {
                State::WallRun { .. } => s.fov_wallrun_add,
                State::Traverse(tr) if tr.kind == TraverseKind::Vault => 3.0,
                State::Vault(v) if !v.onto() => 3.0,
                _ => 0.0,
            };
        self.fov = approach(self.fov, target_fov, 4.0, dt);
        // TdMove_Vertigo: StartZoom(ZoomFOV 84 of the default 90, ZoomRate 30 a second); UnZoom
        // back at 20.
        let (zoom_to, zoom_rate) = if c.vertigo_zoom { (84.0 - 90.0, 30.0) } else { (0.0, 20.0) };
        self.zoom += (zoom_to - self.zoom).clamp(-zoom_rate * dt, zoom_rate * dt);

        let eye = base.eye
            + Vec3::Y * (dip + bob_y + flip_drop)
            + right * bob_x
            + right * sh_off.x
            + Vec3::Y * sh_off.y;

        Shot {
            view: View {
                eye,
                yaw: base.yaw + sh_yaw,
                pitch: base.pitch + self.assist + pitch_kick + sh_pitch + flip_pitch,
                roll: self.roll + bob_roll + lean + sh_roll,
                fov_deg: self.fov + self.zoom,
            },
            step_phase: self.step_phase,
            gait: self.gait,
            shake,
        }
    }
}

/// Mirror's Edge's speed blur (TdMotionBlurPostProcess, its render proxy 0x12d42a0): how hard
/// TdMotionBlurShader.usf blurs outward from the screen centre this frame (its MotionPacked.r).
///
/// The proxy measures the camera's own velocity frame to frame (each axis held to ±720 uu/s,
/// smoothed 0.7 new / 0.3 old; its direction smoothed again by 0.1), ramps its speed from
/// TdMotionBlurStartPlayerSpeed (400 uu/s) to 720 into 0..1, eases that 0.2 a frame, and scales
/// it by TdMotionBlurAmount (0.5) and by how much the motion is along the view.
#[derive(Default)]
pub struct SpeedBlur {
    last_eye: Option<Vec3>,
    vel: Vec3,
    dir: Vec3,
    ramp: f32,
}

impl SpeedBlur {
    /// TdMotionBlurPostProcess defaults.
    pub const START_SPEED: f32 = 4.0;
    pub const AMOUNT: f32 = 0.5;
    const MAX_SPEED: f32 = 7.2;

    /// One frame: the camera's position and look direction. Returns MotionPacked.r.
    pub fn update(&mut self, dt: f32, eye: Vec3, forward: Vec3) -> f32 {
        let cur = match self.last_eye {
            Some(last) if dt > 1e-4 => ((eye - last) / dt).clamp(Vec3::splat(-Self::MAX_SPEED), Vec3::splat(Self::MAX_SPEED)),
            _ => Vec3::ZERO,
        };
        self.last_eye = Some(eye);
        self.vel = cur * 0.7 + self.vel * 0.3;
        self.dir += (self.vel - self.dir) * 0.1;
        let speed = self.vel.length();
        let ramp = ((speed - Self::START_SPEED).max(0.0)).min(Self::MAX_SPEED - Self::START_SPEED) / (Self::MAX_SPEED - Self::START_SPEED);
        self.ramp = ramp * 0.2 + self.ramp * 0.8;
        let along = self.dir.normalize_or_zero().dot(forward.normalize_or_zero());
        Self::AMOUNT * along * self.ramp
    }

    /// A teleport: no streak from the jump.
    pub fn reset(&mut self) {
        *self = SpeedBlur::default();
    }
}
