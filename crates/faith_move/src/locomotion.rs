//! Running on the ground, the way Mirror's Edge does it.
//!
//! Ported from the game's native code (`MirrorsEdge.exe`, read with Ghidra;
//! see `tools/me-extract/ghidra`), not the scripts:
//!
//! - `ATdPawn::GetSprintAcceleration` / `GetWalkAcceleration`: the
//!   acceleration the player controller asks for each frame
//!   (`TdPlayerController.PlayerWalking.PlayerMove` picks which).
//! - `ATdPawn::CalcVelocity`: how that acceleration, friction and braking turn
//!   into velocity, capped at `GroundSpeed`.
//! - The sprint build-up is a designer curve, `TdPawn.SpeedCurve_LightWeapon`:
//!   speed against seconds of running. On load the game turns it into
//!   acceleration against speed (`AccelCurve_LightWeapon`), so sprinting
//!   straight follows the curve exactly: friction is cancelled by a matching
//!   term in the sprint acceleration.
//!
//! Units here are metres and seconds; the game's are uu (cm). Every constant
//! names where it came from.

use glam::Vec3;

/// `TdPawn.SpeedCurve_LightWeapon`: (seconds of running, speed m/s), linear.
pub const SPEED_CURVE: [(f32, f32); 5] = [(0.0, 0.0), (0.4, 4.0), (1.0, 5.2), (3.5, 6.5), (7.0, 7.2)];

/// A piecewise-linear `FInterpCurveFloat` (CIM_Linear points), evaluated the
/// way `FInterpCurve::Eval` does: clamped at both ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve(pub Vec<(f32, f32)>);

impl Curve {
    pub fn eval(&self, x: f32) -> f32 {
        let p = &self.0;
        match p.len() {
            0 => 0.0,
            1 => p[0].1,
            n => {
                if x <= p[0].0 {
                    return p[0].1;
                }
                if x >= p[n - 1].0 {
                    return p[n - 1].1;
                }
                for w in p.windows(2) {
                    let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                    if x < x1 {
                        let d = x1 - x0;
                        return if d <= 0.0 { y0 } else { y0 + (y1 - y0) * (x - x0) / d };
                    }
                }
                p[n - 1].1
            }
        }
    }
}

/// `ATdPawn` building `AccelCurve_LightWeapon` from the speed curve (0x12c2db0,
/// called with 10): sample the speed curve at keys x 10 + 1 even times to get
/// time-at-speed, then at 11 speeds from 0 to the top one, take the speed gained
/// over the next 0.5 / 10 s.
pub fn accel_curve_from(speed: &[(f32, f32)], steps: usize) -> Curve {
    let s = Curve(speed.to_vec());
    let (t_last, v_last) = *speed.last().expect("a speed curve");
    let samples = speed.len() * 10;
    let inverse = Curve(
        (0..=samples)
            .map(|k| {
                let t = k as f32 * t_last / samples as f32;
                (s.eval(t), t)
            })
            .collect(),
    );
    let dt = 0.5 / steps as f32;
    Curve(
        (0..=steps)
            .map(|i| {
                let v = i as f32 * v_last / steps as f32;
                let t = inverse.eval(v);
                (v, (s.eval(t + dt) - s.eval(t)) / dt)
            })
            .collect(),
    )
}

/// TdPawn's ground-movement numbers (defaults from TdGame.u, uu converted to m).
#[derive(Clone, Debug)]
pub struct Locomotion {
    pub accel_curve: Curve,
    /// SpeedMaxBaseVelocity = 400: above this, the excess is sprint energy.
    pub max_base: f32,
    /// SpeedMinBaseVelocity = 10.
    pub min_base: f32,
    /// Speed{Sprint,Walk,Strafe}VelocityAccelerationFactor = 30 / 7 / 10.
    pub sprint_factor: f32,
    pub walk_factor: f32,
    pub strafe_factor: f32,
    /// SpeedEnergyDecelerationTime = 3, SpeedEnergyDecelerationExponent = 0.5.
    pub energy_decel_time: f32,
    pub energy_decel_exponent: f32,
    /// SpeedTurnDecelerationFactor = 10.
    pub turn_decel: f32,
    /// Pawn.GroundSpeed = 720 (top speed) and AccelRate = 6144 (most acceleration).
    pub ground_speed: f32,
    pub accel_rate: f32,
    /// PhysicsVolume.GroundFriction = 8.
    pub ground_friction: f32,
    /// TdPawn.BrakingFrictionStrength = 1.
    pub braking_strength: f32,
    /// TdPlayerController.InputMaxSprintHeightLimit / RaduisLimit = 0.7: how hard you push forward
    /// (and in all) for the controller to ask for sprint acceleration.
    pub sprint_input_forward: f32,
    pub sprint_input_size: f32,
}

impl Default for Locomotion {
    fn default() -> Self {
        Locomotion {
            accel_curve: accel_curve_from(&SPEED_CURVE, 10),
            max_base: 4.0,
            min_base: 0.1,
            sprint_factor: 30.0,
            walk_factor: 7.0,
            strafe_factor: 10.0,
            energy_decel_time: 3.0,
            energy_decel_exponent: 0.5,
            turn_decel: 10.0,
            ground_speed: 7.2,
            accel_rate: 61.44,
            ground_friction: 8.0,
            // TdPlayerPawn.BrakingFrictionStrength (TdPawn's own is 1.0)
            braking_strength: 0.5,
            sprint_input_forward: 0.7,
            sprint_input_size: 0.7,
        }
    }
}

/// What the pawn carries between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunState {
    /// TdPawn.SpeedSprintEnergy (m/s above SpeedMaxBaseVelocity).
    pub energy: f32,
    /// TdPlayerController.AccelerationTime / bIsStopping (the tap-stop).
    pub accel_time: f32,
    pub stopping: f32,
}

/// One frame's input, as the controller hands it to the pawn.
#[derive(Clone, Copy, Debug)]
pub struct RunInput {
    /// Facing (horizontal, unit) and its right.
    pub forward: Vec3,
    pub right: Vec3,
    /// PlayerInput.aForward / aStrafe (-1..1).
    pub a_forward: f32,
    pub a_strafe: f32,
    /// How far the view turned this frame, in radians (PlayerInput.aTurn is in rotation units:
    /// 65536 a turn).
    pub turn: f32,
}

fn horiz(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// The game rounds requested acceleration to 0.1 uu/s² (`floor(x * 10 + 0.5) / 10`).
fn round_accel(a: Vec3) -> Vec3 {
    let r = |x: f32| ((x * 100.0 * 10.0 + 0.5).floor() / 10.0) / 100.0;
    Vec3::new(r(a.x), r(a.y), r(a.z))
}

impl Locomotion {
    fn input_dir(&self, i: &RunInput) -> Vec3 {
        (i.forward * i.a_forward + i.right * i.a_strafe).normalize_or_zero()
    }

    /// Friction compensation both accelerations add on the ground: CalcVelocity takes
    /// velocity x friction x 0.1 off every frame, so this hands it back.
    fn friction_comp(&self, vel: Vec3) -> Vec3 {
        vel * self.ground_friction * 0.1
    }

    fn turn_damping(&self, vel: Vec3, turn: f32) -> Vec3 {
        let turn_uu = turn.abs() * 65536.0 / std::f32::consts::TAU;
        vel * self.turn_decel * turn_uu / 32768.0
    }

    /// ATdPawn::GetSprintAcceleration (0x12bd920).
    pub fn sprint_accel(&self, st: &mut RunState, i: &RunInput, vel: Vec3, dt: f32, falling: bool) -> Vec3 {
        let speed = horiz(vel).length();
        let dir = self.input_dir(i);
        if dir.abs().max_element() < 1e-4 {
            st.energy = 0.0;
            return Vec3::ZERO;
        }
        st.energy = (speed - self.max_base).max(0.0);
        let mut accel = dir * self.accel_curve.eval(speed);
        if !falling {
            // Steer the velocity onto the input direction, keeping its speed.
            let diff = Vec3::new(dir.x * speed - vel.x, 0.0, dir.z * speed - vel.z);
            let d = diff.length();
            let steer = (self.sprint_factor * d).min(if dt > 0.0 { d / dt } else { 0.0 });
            accel += diff.normalize_or_zero() * steer + self.friction_comp(vel);
        }
        round_accel(accel - self.turn_damping(vel, i.turn))
    }

    /// ATdPawn::GetWalkAcceleration (0x12b57f0).
    pub fn walk_accel(&self, st: &mut RunState, i: &RunInput, vel: Vec3, dt: f32, falling: bool) -> Vec3 {
        let dir = self.input_dir(i);
        if dir.abs().max_element() < 1e-4 {
            st.energy = 0.0;
            return Vec3::ZERO;
        }
        // Sprint energy drains unless you keep pushing the way you face:
        // (GroundSpeed - MaxBase) x (1 - clamp(dir . facing, 0, 0.9)^exponent) / time.
        if st.energy > 0.0 {
            let along = dir.dot(i.forward).clamp(0.0, 0.9);
            let drain = (self.ground_speed - self.max_base) * (1.0 - along.powf(self.energy_decel_exponent))
                / self.energy_decel_time;
            st.energy = (st.energy - drain * dt).max(0.0);
        }
        // Aim for MinBase along the input plus (MaxBase - MinBase + energy) scaled by the
        // stick, per axis, and close the gap at the walk (forward) and strafe rates.
        let top = self.max_base - self.min_base + st.energy;
        let want_f = dir.dot(i.forward) * self.min_base + top * i.a_forward;
        let want_s = dir.dot(i.right) * self.min_base + top * i.a_strafe;
        let have_f = vel.dot(i.forward);
        let have_s = vel.dot(i.right);
        let mut accel =
            i.forward * (want_f - have_f) * self.walk_factor + i.right * (want_s - have_s) * self.strafe_factor;
        if !falling {
            accel += self.friction_comp(vel);
        }
        round_accel(accel)
    }

    /// TdPlayerController's PlayerWalking: which acceleration to ask for, and the tap-stop
    /// (let go within 0.15 s of starting and you stop dead for 0.25 s, at 35 uu/s).
    /// Returns the acceleration; may set `vel` for the stop.
    pub fn controller_accel(&self, st: &mut RunState, i: &RunInput, vel: &mut Vec3, dt: f32) -> Vec3 {
        if st.stopping > 0.0 {
            st.stopping -= dt;
            *vel = horiz(*vel).normalize_or_zero() * 0.35;
            return *vel;
        }
        let dir = self.input_dir(i);
        if horiz(dir).length() > 0.0 {
            st.accel_time += dt;
            let size = (i.a_forward * i.a_forward + i.a_strafe * i.a_strafe).sqrt();
            let sprint = i.a_forward > self.sprint_input_forward && size > self.sprint_input_size && vel.dot(dir) > 0.0;
            if sprint {
                self.sprint_accel(st, i, *vel, dt, false)
            } else {
                self.walk_accel(st, i, *vel, dt, false)
            }
        } else {
            let a = self.walk_accel(st, i, *vel, dt, false);
            if st.accel_time > 0.0 && st.accel_time < 0.15 {
                st.stopping = 0.25;
                *vel = horiz(*vel).normalize_or_zero() * 0.35;
            }
            st.accel_time = 0.0;
            a
        }
    }

    /// ATdPawn::CalcVelocity (0x12b3050) for walking: `speed_mod` is the move's SpeedModifier
    /// (TdMove_Crouch 0.2, ...), `friction` is GroundFriction x the move's FrictionModifier.
    pub fn calc_velocity(&self, vel: Vec3, accel: Vec3, dt: f32, speed_mod: f32, friction: f32) -> Vec3 {
        let max_accel = self.accel_rate * speed_mod;
        let max_speed = self.ground_speed * speed_mod;
        let accel = if accel.length_squared() > max_accel * max_accel { accel.normalize() * max_accel } else { accel };
        let mut v = vel;
        if accel != Vec3::ZERO {
            v -= v * dt * friction * 0.1;
        } else {
            // Braking, in steps of at most 0.03 s at 2 x friction x BrakingFrictionStrength; the
            // result is the average over the steps, and it stops dead once reversed or slow.
            let before = v;
            let mut left = dt;
            let mut avg = Vec3::ZERO;
            while left > 0.0 {
                let step = left.min(0.03);
                left -= step;
                v -= v * 2.0 * step * friction * self.braking_strength;
                if v.dot(before) > 0.0 && dt > 0.0 {
                    avg += v * step / dt;
                }
            }
            v = avg;
            if v.dot(before) < 0.0 || v.length_squared() < 0.1 * 0.1 {
                v = Vec3::ZERO;
            }
        }
        v += accel * dt;
        if v.length_squared() > max_speed * max_speed {
            v = v.normalize() * max_speed;
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(loco: &Locomotion, secs: f32, dt: f32) -> Vec<(f32, f32)> {
        let mut st = RunState::default();
        let mut vel = Vec3::ZERO;
        let i = RunInput { forward: Vec3::NEG_Z, right: Vec3::X, a_forward: 1.0, a_strafe: 0.0, turn: 0.0 };
        let mut t = 0.0;
        let mut out = vec![];
        while t < secs {
            let a = loco.controller_accel(&mut st, &i, &mut vel, dt);
            vel = loco.calc_velocity(vel, a, dt, 1.0, loco.ground_friction);
            t += dt;
            out.push((t, vel.length()));
        }
        out
    }

    #[test]
    fn accel_curve_is_the_speed_curves_slope() {
        let c = accel_curve_from(&SPEED_CURVE, 10);
        assert_eq!(c.0.len(), 11);
        // 0..400 uu/s in 0.4 s: 1000 uu/s² = 10 m/s². 520..650 in 2.5 s: 0.52 m/s².
        assert!((c.eval(2.0) - 10.0).abs() < 0.1, "{}", c.eval(2.0));
        assert!((c.eval(5.76) - 0.52).abs() < 0.05, "{}", c.eval(5.76));
        assert!(c.eval(7.2).abs() < 1e-3);
    }

    /// Sprinting straight from a standstill follows SpeedCurve_LightWeapon.
    #[test]
    fn sprint_follows_the_speed_curve() {
        let loco = Locomotion::default();
        let log = run(&loco, 8.0, 1.0 / 60.0);
        let at = |t: f32| log.iter().find(|(x, _)| *x >= t).unwrap().1;
        for (t, v) in [(0.4, 4.0), (1.0, 5.2), (3.5, 6.5), (7.0, 7.2)] {
            assert!((at(t) - v).abs() < 0.25, "at {t} s: {} m/s, curve says {v}", at(t));
        }
        assert!(at(8.0) <= 7.2 + 1e-3);
    }

    /// Letting go brakes to a stop in well under a second.
    #[test]
    fn letting_go_brakes() {
        let loco = Locomotion::default();
        let mut st = RunState { accel_time: 2.0, ..Default::default() };
        let mut vel = Vec3::new(0.0, 0.0, -6.0);
        let i = RunInput { forward: Vec3::NEG_Z, right: Vec3::X, a_forward: 0.0, a_strafe: 0.0, turn: 0.0 };
        let mut t = 0.0;
        while vel.length() > 0.0 && t < 2.0 {
            let a = loco.controller_accel(&mut st, &i, &mut vel, 1.0 / 60.0);
            vel = loco.calc_velocity(vel, a, 1.0 / 60.0, 1.0, loco.ground_friction);
            t += 1.0 / 60.0;
        }
        assert!(t < 0.5, "stopped after {t} s");
    }

    /// Whipping the view round while sprinting costs speed.
    #[test]
    fn turning_hard_bleeds_sprint() {
        let loco = Locomotion::default();
        let mut st = RunState::default();
        let vel = Vec3::new(0.0, 0.0, -7.0);
        let straight = RunInput { forward: Vec3::NEG_Z, right: Vec3::X, a_forward: 1.0, a_strafe: 0.0, turn: 0.0 };
        let turning = RunInput { turn: 0.2, ..straight };
        let a0 = loco.sprint_accel(&mut st, &straight, vel, 1.0 / 60.0, false);
        let a1 = loco.sprint_accel(&mut st, &turning, vel, 1.0 / 60.0, false);
        assert!(a1.dot(Vec3::NEG_Z) < a0.dot(Vec3::NEG_Z) - 1.0, "{a0} vs {a1}");
    }
}
