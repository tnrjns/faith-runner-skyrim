//! Where Faith's body and the camera go each frame: the animated pose from [`Driver`], placed
//! in the world the way Mirror's Edge places it (TdPawn / TdPlayerPawn.CalcCamera). Engine
//! independent: any host draws from the same [`RigFrame`].
//!
//! Everything is in faith_move's world frame: metres, Y up, yaw 0 looking down -Z.

use std::f32::consts::{PI, TAU};

use faith_move::{Controller, Event as MoveEvent, Shot, State as MoveState, SwanNeck};
use glam::{EulerRot, Quat, Vec3};
use me_assets::FaithArms;
use me_assets::pose::to_view;

use crate::driver::{BodyAnim, Driver};

/// The body and camera for one frame.
#[derive(Clone, Copy, Debug)]
pub struct RigFrame {
    /// The animation's own outputs (camera bone, travel blend, ...).
    pub body: BodyAnim,
    /// Where the skeleton's root goes, and how the upper body (arms mesh) and the legs mesh
    /// are turned. Both meshes share the one pose; the legs can face another way.
    pub origin: Vec3,
    pub body_rot: Quat,
    pub legs_rot: Quat,
    /// The camera: the camera bone plus your look, screen shake and the swan neck.
    pub cam_pos: Vec3,
    pub cam_rot: Quat,
    /// The move draws the body in the world's depth (SDPG_Intermediate) rather than over it.
    pub intermediate: bool,
}

/// Body facing, legs facing and the swan neck: what the placement keeps between frames.
pub struct Rig {
    pub driver: Driver,
    /// Camera slides forward and down over the chest when looking down.
    swan: SwanNeck,
    /// Where the legs face. They trail the view when you turn on the spot,
    /// like the game's leg rotation (TdPawn.LegRotationSpeed).
    leg_yaw: f32,
    legs_turning: bool,
    /// Where the body faces. Usually your view, but moves that lock the body
    /// (slide, wallrun, beam, zipline, swing, vaults; the game's
    /// bDisableFaceRotation) keep it on the move while you turn your head.
    body_yaw: f32,
    /// Easing the body back onto the view after a locked move.
    body_unlocking: bool,
}

pub fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

impl Rig {
    pub fn new(arms: &FaithArms, yaw: f32) -> Self {
        Rig { driver: Driver::new(arms), swan: Default::default(), leg_yaw: yaw, legs_turning: false, body_yaw: yaw, body_unlocking: false }
    }

    /// Animate and place everything for this frame. `shot` is this frame's camera effects
    /// (`CameraFx::update`), after the controller stepped.
    pub fn update(&mut self, dt: f32, c: &Controller, shot: &Shot, arms: &FaithArms) -> RigFrame {
        let body = self.driver.update(dt, c, arms);

        // ---- place the body
        let yaw = Quat::from_rotation_y(c.yaw);
        let twist = Quat::from_rotation_y(body.cam_yaw);
        let yaw_of = |v: Vec3| (v.x * v.x + v.z * v.z > 0.25).then(|| (-v.x).atan2(-v.z));
        // Some(Some(y)): body locked facing y; Some(None): locked, hold; None: follows the view.
        let lock: Option<Option<f32>> = match c.state {
            MoveState::Slide { .. } | MoveState::WallRun { .. } => Some(yaw_of(c.vel)),
            // TdMove_RumpSlide.bDisableFaceRotation: facing down the slope.
            MoveState::RumpSlide { face, .. } => Some(Some(face)),
            MoveState::GrabTransfer { normal, .. } => Some(yaw_of(-normal)),
            // TdMove_Climb.bDisableFaceRotation: facing the ladder; over the top from the roof,
            // facing out until LadderEnterTop turns her round.
            MoveState::IntoClimb { ladder, start: faith_move::ClimbStart::EnterTop, .. } => Some(yaw_of(ladder.normal)),
            MoveState::IntoClimb { ladder, .. } | MoveState::Climb { ladder, .. } | MoveState::ClimbExit { ladder, .. } => Some(yaw_of(-ladder.normal)),
            MoveState::ZipLine { a, b, .. } => Some(yaw_of(b - a)),
            MoveState::Swing { dir, .. } => Some(yaw_of(dir)),
            MoveState::Balance { a, b, .. } => {
                let u = b - a;
                let u = if u.dot(c.forward()) < 0.0 { -u } else { u };
                Some(yaw_of(u))
            }
            // TdMove_LayOnGround.bDisableFaceRotation: lying there, looking round doesn't turn you.
            MoveState::Traverse(_) | MoveState::Vault(_) | MoveState::LayOnGround { .. } | MoveState::Barge { .. } | MoveState::AirBarge { .. } | MoveState::Stumble { .. } | MoveState::SoftLand { .. } => Some(None),
            // TdMove_Grab.bDisableFaceRotation: hanging, the body keeps facing the wall and only
            // the view turns (looking right round plays the hang-turn reach instead).
            MoveState::LedgeHang { normal, .. } if c.shimmy.is_none_or(|sh| sh.corner.is_none()) => Some(yaw_of(-normal)),
            MoveState::LedgeHang { .. } => Some(None),
            _ => None,
        };
        match lock {
            Some(Some(target)) => {
                self.body_yaw += wrap_angle(target - self.body_yaw) * (1.0 - (-12.0 * dt).exp());
                self.body_unlocking = true;
            }
            Some(None) => self.body_unlocking = true,
            // Back on the view: ease over, then stick to it exactly (no lag when
            // you whip the mouse around normally).
            None if self.body_unlocking => {
                let d = wrap_angle(c.yaw - self.body_yaw);
                let step = 9.0 * dt;
                if d.abs() <= step {
                    self.body_yaw = c.yaw;
                    self.body_unlocking = false;
                } else {
                    self.body_yaw += step * d.signum();
                }
            }
            None => self.body_yaw = c.yaw,
        }
        // TdMove_WallrunJump.StartMove: FaceRotationTimeLeft 0.1 (and SetPreciseRotation to the
        // wall normal after a Q): jumping off a wall, the body turns to where you're looking at
        // once, so the jump-off clip isn't seen side-on.
        if c.events.iter().any(|e| matches!(e, MoveEvent::WallJump)) {
            self.body_yaw = c.yaw;
            self.body_unlocking = false;
        }
        // TdMove_Swing.SetPawnRotation: SwingControl (a SkelControlSingleBone on the root) turns
        // the whole skeleton by the swing angle and shifts it so it pivots about the grip, so the
        // hands stay on the bar and the view swings with the body (GetCameraAnimation reads the
        // controlled skeleton).
        let swing = match c.state {
            MoveState::Swing { at, angle, .. } => Some((at, Quat::from_rotation_x(angle))),
            _ => None,
        };
        let tilt = swing.map_or(Quat::IDENTITY, |(_, q)| q);
        let body_rot = Quat::from_rotation_y(self.body_yaw) * tilt * twist.inverse();
        let cam_local = to_view(body.cam.w_axis.truncate());
        let feet = c.root();
        let gameplay_eye = c.view().eye;
        let aligned = gameplay_eye - body_rot * cam_local;
        // After a step up or down the mesh (and her camera on it) eases to her feet
        // (Controller::mesh_offset).
        let mut origin = feet.lerp(aligned, body.align) + Vec3::Y * c.mesh_offset;
        if let Some((bar, _)) = swing {
            let hand = |n: &str| arms.bone(n).map(|b| to_view(self.driver.globals[b].w_axis.truncate()));
            // The bar runs through the palms, at the fingers' roots.
            if let (Some(l), Some(r)) = (hand("LeftHandMiddle0"), hand("RightHandMiddle0")) {
                origin = bar - body_rot * ((l + r) * 0.5);
            }
        }

        // ---- legs: face where you're running (strafing turns them up to 90°,
        // TdPawn.GoBackLegAngleLimit; past that the backward cycle plays), and
        // standing still they trail the view until it's turned too far.
        let deg = |d: f32| d.to_radians();
        let leg_speed = 50000.0 / 65536.0 * TAU * dt; // TdPawn.LegRotationSpeed
        let free_legs = matches!(c.state, MoveState::Ground) && !c.crouched;
        if !free_legs {
            self.leg_yaw = self.body_yaw;
            self.legs_turning = false;
        } else {
            let v = Vec3::new(c.vel.x, 0.0, c.vel.z);
            let target = if v.length() > 0.5 {
                self.legs_turning = false;
                let move_yaw = (-v.x).atan2(-v.z);
                let mut d = wrap_angle(move_yaw - c.yaw);
                if d.abs() > deg(95.0) {
                    d = wrap_angle(d + PI); // running backwards
                }
                c.yaw + d.clamp(-deg(90.0), deg(90.0))
            } else if let Some(ly) = body.legs_yaw {
                // Standing still: TdAnimNodeTurn steps the legs round (the driver).
                self.legs_turning = false;
                self.leg_yaw = ly;
                ly
            } else {
                self.leg_yaw
            };
            self.leg_yaw += wrap_angle(target - self.leg_yaw).clamp(-leg_speed, leg_speed);
            let d = wrap_angle(self.leg_yaw - c.yaw).clamp(-deg(100.0), deg(100.0));
            self.leg_yaw = c.yaw + d;
        }
        let legs_rot = Quat::from_rotation_y(self.leg_yaw) * tilt * twist.inverse();

        // Swan neck: looking down moves the eye forward over the chest and down,
        // so you see your legs instead of the inside of your torso.
        let (swan_fwd, swan_down) = self.swan.update(c, dt);
        let swan = yaw * Vec3::new(0.0, -swan_down, -swan_fwd);

        // TdMove.FirstPersonDPG: the body normally draws over the world (SDPG_Foreground);
        // swinging and pipe climbing use SDPG_Intermediate, depth-tested against the world, so
        // the bar hides the fingers wrapped round it.
        let intermediate = matches!(c.state, MoveState::Swing { .. });

        // ---- the camera is the camera bone, plus look pitch and screen shake
        let fx_offset = shot.view.eye - gameplay_eye;
        let fx_pitch = shot.view.pitch - c.pitch;
        let fx_yaw = shot.view.yaw - c.yaw;
        // TdPlayerPawn.CalcCamera: the camera sits at EyeJoint and looks where you look, plus
        // GetCameraAnimation (0x12b5690): the EyeJoint / CameraJoint rotation in mesh space, so
        // every body clip turns it (a roll tumbles the view through 360 degrees), plus the camera
        // slot's clip. The clip's yaw is already taken out of the body placement (twist).
        let bone_rot = (twist.inverse() * Quat::from_mat4(&body.cam)).normalize() * body.cam_delta;
        let look = Quat::from_euler(EulerRot::YXZ, c.yaw + fx_yaw, (c.pitch + fx_pitch).clamp(-1.55, 1.55), -shot.view.roll);
        let cam_pos = origin + body_rot * cam_local + fx_offset + swan;
        // CameraRoll (lazy spring on EyeJoint): bank right-side-down into a right turn.
        let cam_rot = look * Quat::from_rotation_z(-body.cam_bank) * tilt * bone_rot;

        RigFrame { body, origin, body_rot, legs_rot, cam_pos, cam_rot, intermediate }
    }

    /// Let the move timings follow the animations they play (as the app does at startup).
    pub fn tune(&self, arms: &FaithArms, t: &mut faith_move::Tuning) {
        if let Some(l) = Driver::length(arms, "HangHeaveUp") {
            t.pullup_time = l;
        }
        if let Some(l) = Driver::length(arms, "fallinglandroll") {
            t.roll_time = l * 0.75;
        }
        // The attacks end with their clips (OnCustomAnimEnd); TdMove_Melee plays its at 1.5 x.
        let c = &mut t.melee_clips;
        let len = |seq: &str, rate: f32, into: &mut f32| {
            if let Some(l) = Driver::length(arms, seq) {
                *into = l / rate;
            }
        };
        len("MeleeStartLeft", 1.5, &mut c.punch_start);
        len("MeleeHitLeft", 1.5, &mut c.punch_hit);
        len("MeleeMissedLeft", 1.5, &mut c.punch_missed);
        len("MeleeCrouchStart", 1.0, &mut c.crouch_start);
        len("MeleeCrouchHit", 1.0, &mut c.crouch_hit);
        len("MeleeInAir", 1.0, &mut c.air);
        len("MeleeInAirStill", 1.0, &mut c.air_still);
        len("MeleeFromAbove", 1.0, &mut c.air_from_above);
        len("MeleeInAirHit", 1.0, &mut c.air_hit);
        len("MeleeSlide", 1.0, &mut c.slide);
        len("MeleeWallRunLeft", 1.0, &mut c.wallrun);
        len("MeleeVaultOver", 1.0, &mut c.vault_kick);
        // The ladder clips' root motion (root space: -Y up, Z forward, uu).
        let root = |seq: &str| {
            self.driver
                .root_pos_curve(arms, seq)
                .unwrap_or_default()
                .into_iter()
                .map(|p| Vec3::new(p.x, -p.y, p.z) / 100.0)
                .collect::<Vec<_>>()
        };
        t.climb_curves = Some(std::sync::Arc::new(faith_move::climb::ClimbCurves {
            exit_ladder: [root("LadderExitTopLeftHand"), root("LadderExitTopRightHand")],
            exit_pipe: [root("pipeexittoplefthand"), root("pipeexittoprighthand")],
            enter_top: root("LadderEnterTop"),
        }));
        for (i, seq) in ["AirBargeIdle", "AirBargeImpact", "AirBargeLand"].iter().enumerate() {
            len(seq, 1.0, &mut t.air_barge_clips[i]);
        }
        // A takedown lasts its clip (TdMOVE_Disarm.OnCustomAnimEnd).
        for (i, seq) in faith_move::TAKEDOWN_ANIMS.iter().enumerate() {
            if let Some(l) = Driver::length(arms, seq) {
                t.takedown_clips[i] = l;
            }
        }
        // The 180 turns follow their clips' root rotation.
        let curve = |seq: &str| self.driver.root_yaw_curve(arms, seq).unwrap_or_default();
        t.turn_curves = Some(std::sync::Arc::new(faith_move::TurnCurves {
            run: curve("RunTurn180"),
            stand: curve("StandTurn180Right"),
            air: curve("JumpTurnFly"),
            wallclimb: curve("wallrunvertical180turn"),
            swing: curve("swing180"),
        }));
    }
}
