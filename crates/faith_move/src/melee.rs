//! Faith's attacks landing on someone: Mirror's Edge's melee moves (TdMove_Melee,
//! TdMove_MeleeCrouch, TdMove_MeleeAir, TdMove_MeleeSlide, TdMove_MeleeWallrun) against
//! targets the host gives (`Controller::targets`), with the game's own target choice, hit tests,
//! damage and recoil. faith_move has no enemies of its own: in the app nothing is ever hit.
//!
//! - **Choosing who** (ATdPlayerController::GetMeleeTarget, native): of the targets within reach,
//!   the best `max(0, dir . facing) x 0.8 + (reach - distance) / reach x 0.2`, if above zero.
//!   Reach is the move's TargetingMaxDistance x 3.
//! - **Punches and the crouch attack** decide when the wind-up clip ends (TestHit): the target
//!   ahead (dot > 0.8) and close (170 / 110 uu), and the follow-through clip is the hit or the
//!   miss.
//! - **Kicks** (air, slide, wallrun) sweep a box along a limb while the move's hit detection is on
//!   (UTdMove_MeleeBase's tick, native): each frame from the bone (plus TraceOffset, turned with
//!   her) 10 uu on, the way the limb moved, clamped into a cone round her facing. A touch counts
//!   if the target is ahead (dot > 0.4) and within 140 uu (TriggerDamage).

use glam::Vec3;

use crate::controller::{forward, horiz, Controller, Event, MeleeKind, MeleePhase, State};

/// Someone Faith can hit: the host's actor, as a cylinder (faith_move's frame, metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    /// The host's handle for it, given back in `Event::MeleeHit`.
    pub id: u32,
    /// The cylinder's centre (Mirror's Edge's pawn Location).
    pub centre: Vec3,
    pub radius: f32,
    pub half_height: f32,
    /// Its eyes above the centre (BaseEyeHeight): where a wallrun kick aims.
    pub eye: f32,
    /// Which way it faces (horizontal): a takedown from behind or the front.
    pub facing: Vec3,
}

const fn uu(v: f32) -> f32 {
    v / 100.0
}

/// TdMove_MeleeVault.StartMove: SetTimer(0.3) to the kick.
const VAULT_KICK_AT: f32 = 0.3;

/// Per move: TargetingMaxDistance, TraceExtent, TraceOffset, MeleeDamage (the class defaults).
pub(crate) struct MeleeClass {
    pub targeting: f32,
    pub extent: Vec3,
    pub offset: Vec3,
    pub damage: f32,
}

pub(crate) fn class(kind: MeleeKind) -> MeleeClass {
    let v = |x: f32| Vec3::splat(uu(x));
    match kind {
        // TdMove_Melee (TargetingMaxDistance from TdMove_MeleeBase: 300).
        MeleeKind::Punch => MeleeClass { targeting: uu(300.0), extent: v(12.0), offset: Vec3::ZERO, damage: 33.5 },
        MeleeKind::Crouch => MeleeClass { targeting: uu(300.0), extent: v(30.0), offset: Vec3::ZERO, damage: 33.5 },
        MeleeKind::AirKick => MeleeClass { targeting: uu(800.0), extent: Vec3::new(uu(60.0), uu(160.0), uu(60.0)), offset: Vec3::ZERO, damage: 100.0 },
        MeleeKind::SlideKick => MeleeClass { targeting: uu(300.0), extent: v(40.0), offset: Vec3::ZERO, damage: 60.0 },
        // TraceOffset (30, 0, 0): 30 uu ahead.
        MeleeKind::WallRunKick => MeleeClass { targeting: uu(300.0), extent: v(30.0), offset: Vec3::new(0.0, 0.0, -uu(30.0)), damage: 80.0 },
        // TdMove_MeleeVault: TraceExtent (60, 60, 160), MeleeDamage from TdMove_MeleeBase.
        MeleeKind::VaultKick => MeleeClass { targeting: uu(800.0), extent: Vec3::new(uu(60.0), uu(160.0), uu(60.0)), offset: Vec3::ZERO, damage: 50.0 },
    }
}

/// GetMeleeTarget's score (0x11c2dc0).
fn score(t: &Target, reach: f32, from: Vec3, facing: Vec3) -> f32 {
    let d = t.centre - from;
    let dist = d.length();
    let near = (reach - dist) / reach * 0.2;
    let aim = d.normalize_or_zero().dot(facing).max(0.0) * 0.8;
    if aim == 0.0 {
        return 0.0;
    }
    aim + near
}

/// GetMeleeTarget: the best scoring target, if any scores above zero.
pub(crate) fn pick(targets: &[Target], reach: f32, from: Vec3, facing: Vec3) -> Option<Target> {
    let mut best: Option<(f32, Target)> = None;
    for t in targets {
        let s = score(t, reach, from, facing);
        if s > best.map_or(0.0, |b| b.0) {
            best = Some((s, *t));
        }
    }
    best.map(|b| b.1)
}

/// The sweep's direction (0x11f2eb0): `v` kept if it's within the cone round `dir`, else turned
/// onto its edge. The game compares the cosine with the cone's angle in radians (35 degrees:
/// 0.611), so "within" reaches out to about 52 degrees. Kept as it is.
pub(crate) fn clamp_to_cone(v: Vec3, dir: Vec3, degrees: f32) -> Vec3 {
    let a = degrees.to_radians();
    let vn = v.normalize_or_zero();
    let dn = dir.normalize_or_zero();
    let dot = vn.dot(dn);
    if dot >= a {
        return v;
    }
    let perp = (vn - dn * dot).normalize_or_zero();
    (dn + perp * a.sin()).normalize_or_zero() * v.length()
}

/// A box (half size `extent`) swept from `start` along `delta`: does it touch the target's
/// cylinder? As UE3's cylinder line check: the cylinder grown by the box (radius + x, height + y).
pub(crate) fn sweep_touches(t: &Target, start: Vec3, delta: Vec3, extent: Vec3) -> bool {
    let r = t.radius + extent.x.max(extent.z);
    let h = t.half_height + extent.y;
    // Closest approach of the segment to the axis, in the horizontal plane, then the height.
    let p = horiz(start - t.centre);
    let d = horiz(delta);
    let dd = d.length_squared();
    let s = if dd > 1e-12 { (-p.dot(d) / dd).clamp(0.0, 1.0) } else { 0.0 };
    for k in [0.0, s, 1.0] {
        let q = start + delta * k - t.centre;
        if horiz(q).length() <= r && q.y.abs() <= h {
            return true;
        }
    }
    false
}

impl Controller {
    /// Where Mirror's Edge's pawn Location is: the middle of her cylinder.
    pub(crate) fn centre(&self) -> Vec3 {
        self.feet + Vec3::Y * self.height() * 0.5
    }

    /// The bone the current attack sweeps (HitDetectionBone), while its hit detection is on.
    /// The host puts that bone's position in `hit_bone` before the next step.
    pub fn melee_bone(&self) -> Option<&'static str> {
        let m = self.melee?;
        if !m.detecting {
            return None;
        }
        Some(match m.kind {
            MeleeKind::AirKick if m.air_type == 2 => "LeftFoot",
            MeleeKind::AirKick | MeleeKind::SlideKick | MeleeKind::VaultKick => "RightFoot",
            MeleeKind::WallRunKick if m.left => "LeftLeg",
            MeleeKind::WallRunKick => "RightLeg",
            MeleeKind::Punch if m.left => "LeftHand",
            MeleeKind::Punch => "RightHand",
            MeleeKind::Crouch => "RightHand",
        })
    }

    fn target_now(&self) -> Option<Target> {
        let id = self.melee?.target?;
        self.targets.iter().copied().find(|t| t.id == id)
    }

    fn deal(&mut self, t: &Target, damage: f32, momentum: Vec3) {
        let kind = self.melee.map(|m| m.kind).unwrap_or(MeleeKind::Punch);
        self.events.push(Event::MeleeHit { target: t.id, damage, momentum, kind });
    }

    /// TestHit (TdMove_Melee, TdMove_MeleeCrouch), when the wind-up clip ends.
    fn test_hit(&mut self) -> bool {
        let Some(m) = self.melee else { return false };
        let Some(t) = self.target_now() else { return false };
        let facing = forward(self.yaw);
        let to = t.centre - self.centre();
        let (ahead, reach, push) = match m.kind {
            // Normal(ToTarget2d) . Rotation > 0.8 && VSize(ToTarget) < 170; momentum 150.
            MeleeKind::Punch => (horiz(to).normalize_or_zero().dot(facing), uu(170.0), uu(150.0)),
            // Normal(ToTarget) . Rotation > 0.8 && VSize(ToTarget) < 110; momentum 800.
            _ => (to.normalize_or_zero().dot(facing), uu(110.0), uu(800.0)),
        };
        if ahead > 0.8 && to.length() < reach {
            self.deal(&t, class(m.kind).damage, facing * push);
            return true;
        }
        false
    }

    /// TdMove_MeleeBase's tick: the limb's sweep, then TriggerDamage's checks.
    fn sweep(&mut self) {
        let Some(m) = self.melee else { return };
        if !m.detecting {
            return;
        }
        let c = class(m.kind);
        let facing = forward(self.yaw);
        let turn = glam::Quat::from_rotation_y(self.yaw);
        // Without a skeleton (no animation), the limb is taken as just ahead of her middle.
        let bone = self.hit_bone.unwrap_or(self.centre() + facing * (self.tuning.half_width + uu(30.0)));
        let start = bone + turn * c.offset;
        let moved = start - self.melee_last_start;
        self.melee_last_start = start;
        let dir = clamp_to_cone(moved, facing, 35.0).normalize_or_zero();
        let delta = dir * uu(10.0);
        let Some(hit) = self.targets.iter().copied().find(|t| sweep_touches(t, start, delta, c.extent)) else { return };
        // TriggerDamage: only the chosen target counts (the wallrun kick's check compares the
        // traced pawn with itself, so for it anyone touched does).
        let t = match m.kind {
            MeleeKind::WallRunKick => self.target_now().unwrap_or(hit),
            _ => match self.target_now() {
                Some(t) if t.id == hit.id => t,
                _ => return,
            },
        };
        let to = t.centre - self.centre();
        let verified = match (m.kind, m.air_type) {
            (MeleeKind::AirKick, 2) => true,
            _ => to.normalize_or_zero().dot(facing) > 0.4 && to.length() < uu(140.0),
        };
        if !verified {
            return;
        }
        let speed2d = self.horizontal_speed();
        match m.kind {
            MeleeKind::AirKick => {
                // MeleeDamage x clamp(GetAverageSpeed(0.25) / 650, 0.6, 1).
                let damage = c.damage * (self.average_speed() / uu(650.0)).clamp(0.6, 1.0);
                self.deal(&t, damage, m.momentum);
                if m.air_type == 0 && m.phase == MeleePhase::Start {
                    self.events.push(Event::MeleeOutcome { kind: m.kind, hit: true });
                    if let Some(mm) = &mut self.melee {
                        mm.phase = MeleePhase::Hit;
                        mm.t = 0.0;
                    }
                }
                if m.air_type == 0 || m.air_type == 2 {
                    // Knocked back off them: 500 uu/s away, 50 up, move input ignored.
                    let away = horiz(self.centre() - t.centre).normalize_or_zero();
                    self.vel = away * uu(500.0) + Vec3::Y * uu(50.0);
                    self.melee_no_input = true;
                }
            }
            MeleeKind::SlideKick => self.deal(&t, c.damage, facing * speed2d * 1.6),
            // ImpactMomentum: Vector(Rotation) x 200.
            MeleeKind::VaultKick => self.deal(&t, c.damage, facing * uu(200.0)),
            MeleeKind::WallRunKick => {
                self.deal(&t, c.damage, facing * uu(500.0));
                self.vel.x *= -0.5;
                self.vel.z *= -0.5;
            }
            MeleeKind::Punch | MeleeKind::Crouch => return,
        }
        if let Some(mm) = &mut self.melee {
            mm.detecting = false;
        }
    }

    /// TdPawn.GetAverageSpeed(0.25).
    fn average_speed(&self) -> f32 {
        let (mut d, mut t) = (0.0, 0.0);
        for &(dt, s) in self.speed_log.iter() {
            d += s * dt;
            t += dt;
        }
        if t > 0.0 { d / t } else { self.vel.length() }
    }

    /// Each step: the speed log, the attack's clock, its phases and sweep.
    pub(crate) fn melee_tick(&mut self, dt: f32) {
        self.speed_log.push_back((dt, self.vel.length()));
        let mut kept = 0.0;
        let mut n = 0;
        for &(d, _) in self.speed_log.iter().rev() {
            kept += d;
            n += 1;
            if kept >= 0.25 {
                break;
            }
        }
        while self.speed_log.len() > n {
            self.speed_log.pop_front();
        }

        let Some(mut m) = self.melee else { return };
        m.t += dt;
        m.window -= dt;
        if let Some(d) = &mut m.detect_in {
            *d -= dt;
            if *d <= 0.0 {
                m.detect_in = None;
                m.detecting = true;
                // TdMove_MeleeVault.TriggerMove: the kick's clip with its hit detection.
                if m.kind == MeleeKind::VaultKick {
                    self.events.push(Event::Melee { kind: m.kind, left: false });
                }
            }
        }
        self.melee = Some(m);
        self.sweep();
        let Some(mut m) = self.melee else { return };
        let clips = self.tuning.melee_clips;
        let length = match (m.kind, m.phase) {
            (MeleeKind::Punch, MeleePhase::Start) => clips.punch_start,
            (MeleeKind::Punch, MeleePhase::Hit) => clips.punch_hit,
            (MeleeKind::Punch, _) => clips.punch_missed,
            (MeleeKind::Crouch, MeleePhase::Start) => clips.crouch_start,
            (MeleeKind::Crouch, _) => clips.crouch_hit,
            (MeleeKind::AirKick, MeleePhase::Hit) => clips.air_hit,
            (MeleeKind::AirKick, _) => match m.air_type {
                0 => clips.air,
                1 => clips.air_still,
                _ => clips.air_from_above,
            },
            (MeleeKind::SlideKick, _) => clips.slide,
            (MeleeKind::WallRunKick, _) => clips.wallrun,
            // The clip starts with the kick, 0.3 s in.
            (MeleeKind::VaultKick, _) => VAULT_KICK_AT + clips.vault_kick,
        };
        if m.t < length {
            return;
        }
        match (m.kind, m.phase) {
            // OnCustomAnimEnd in MS_MeleeAttackNormal: TestHit, then the hit or the miss.
            (MeleeKind::Punch | MeleeKind::Crouch, MeleePhase::Start) => {
                let hit = self.test_hit();
                m.phase = if hit { MeleePhase::Hit } else { MeleePhase::Missed };
                m.t = 0.0;
                self.melee = Some(m);
                self.events.push(Event::MeleeOutcome { kind: m.kind, hit });
            }
            // TdMove_Melee: a punch queued while the window was open follows. StartMove's Reset
            // clears the counters before the queued one is taken off (leaving -1), and the hand
            // flips twice: the next punch is thrown with the same hand.
            (MeleeKind::Punch, _) if m.queued > 0 => {
                let target = self.melee_target(MeleeKind::Punch);
                self.melee = Some(crate::controller::Melee {
                    kind: MeleeKind::Punch,
                    t: 0.0,
                    left: m.left,
                    phase: MeleePhase::Start,
                    target,
                    air_type: 0,
                    detecting: false,
                    detect_in: None,
                    queued: -1,
                    combo: 0,
                    window: 0.33,
                    momentum: Vec3::ZERO,
                });
                self.events.push(Event::Melee { kind: MeleeKind::Punch, left: m.left });
            }
            _ => self.melee = None,
        }
    }

    /// The move's target at its start (StartMove: GetMeleeTarget(TargetingMaxDistance x 3)).
    pub(crate) fn melee_target(&self, kind: MeleeKind) -> Option<u32> {
        pick(&self.targets, class(kind).targeting * 3.0, self.centre(), forward(self.yaw)).map(|t| t.id)
    }

    /// TdMove_Melee.HandleMoveAction: attack pressed during a punch, while its window is open.
    pub(crate) fn melee_pressed_again(&mut self) {
        if let Some(m) = &mut self.melee {
            if m.kind == MeleeKind::Punch && m.window > 0.0 && m.combo < 2 {
                m.combo += 1;
                m.queued += 1;
            }
        }
    }

    /// TdMove_MeleeAir: which air attack (StartMove / CanDoMove), and its setup (TriggerMove).
    pub(crate) fn start_air_kick(&mut self, m: &mut crate::controller::Melee) {
        // MAT_FromJump moving faster than 200 uu/s, else MAT_FromJumpStill.
        m.air_type = if self.horizontal_speed() > uu(200.0) { 0 } else { 1 };
        if m.air_type == 0 {
            if let Some(t) = m.target.and_then(|id| self.targets.iter().find(|t| t.id == id)) {
                // Coming down onto them from above: MAT_FromJumpHigh.
                if (t.centre - self.centre()).normalize_or_zero().y < -0.6 && self.vel.y < 0.0 {
                    m.air_type = 2;
                }
            }
        }
        match m.air_type {
            0 => m.detecting = true,
            1 => {
                m.detect_in = Some(0.25);
                self.vel.y = 0.0;
            }
            _ => m.detect_in = Some(0.15),
        }
        m.momentum = self.vel * 1.6;
    }

    /// TdMove_MeleeVault.StartMove: the target now, the kick (TriggerMove) 0.3 s on.
    pub(crate) fn start_vault_kick(&mut self) {
        let kind = MeleeKind::VaultKick;
        self.melee = Some(crate::controller::Melee {
            kind,
            t: 0.0,
            left: false,
            phase: MeleePhase::Start,
            target: self.melee_target(kind),
            air_type: 0,
            detecting: false,
            detect_in: Some(VAULT_KICK_AT),
            queued: 0,
            combo: 0,
            window: 0.0,
            momentum: Vec3::ZERO,
        });
    }

    /// TdMove_MeleeWallrun.TriggerMove: off the wall at the target, or 33 degrees out from it.
    pub(crate) fn start_wallrun_kick(&mut self, m: &mut crate::controller::Melee) {
        let speed2d = self.horizontal_speed();
        match m.target.and_then(|id| self.targets.iter().copied().find(|t| t.id == id)) {
            Some(t) => {
                let to = (t.centre + Vec3::Y * t.eye - self.centre()).normalize_or_zero();
                self.vel = to * speed2d * 0.75;
                let f = horiz(to);
                if f.length_squared() > 1e-6 {
                    self.yaw = (-f.x).atan2(-f.z);
                }
            }
            None => {
                // 6000 rotation units (33 degrees) away from the wall (to the right with the wall on
                // the left: a negative yaw here), 1.2 x as fast.
                let a = 6000.0 / 65536.0 * std::f32::consts::TAU * if m.left { -1.0 } else { 1.0 };
                self.yaw += a;
                self.vel = glam::Quat::from_rotation_y(a) * self.vel * 1.2;
            }
        }
        m.detecting = true;
        self.state = State::Air;
    }
}
