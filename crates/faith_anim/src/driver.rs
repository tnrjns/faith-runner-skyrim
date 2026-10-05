//! Chooses and blends Mirror's Edge's own first-person animations from the
//! movement state, the way the game's move classes do (each TdMove plays its
//! own sequences: TdMove_WallRun â†’ WallrunLeft/Right, TdMove_Grab â†’ Hang, â€¦).
//!
//! Output is a blended local pose, plus the animated camera (the skeleton's
//! CameraJoint): the arms render relative to it and the game camera takes its
//! pitch/roll and its locomotion bob.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};
use faith_move::{airbarge::AirBargePhase, ClimbStart, Controller, Event as MoveEvent, MeleeKind, State as MoveState, TraverseKind};
use me_assets::pose::{self, Pose, TrackMap};
use me_assets::anim::Notify;
use me_assets::{AnimSet, FaithArms};

/// Mirror's Edge's walking states, straight from the game: TdPawn picks one
/// by speed (SneakVelocity 5, WalkVelocity 50, JogVelocity 260, RunVelocity
/// 400, SprintVelocity 630 uu/s) and AT_C1P's TdAnimNodeWalkingState
/// crossfades to it over its BlendWeight (0.15 / 0.1 / 0.35 / 0.3 / 0.6 / 0.8 s).
/// Each state is a TdAnimNodeBlendDirectional: forward, diagonal ("stiff"
/// arms while the legs turn toward the move) and backward cycles.
struct WalkState {
    min_speed: f32,
    blend: f32,
    fwd: &'static str,
    stiff: &'static str,
    bwd: &'static str,
    /// The cycles' TdAnimNodeSequence.BaseSpeed (m/s): played at speed / it (ScalePlayRateBySpeed),
    /// held within RateMin..RateMax.
    authored: f32,
    rate_min: f32,
    rate_max: f32,
}

/// AT_C1P's WalkingState. `blend`: TdAnimNodeState.SetActiveMove (0x1210870) blends to a state
/// over max(BlendWeight[new], BlendOutWeight[old]); this node has no BlendOutWeights, so the
/// latter is the 0.2 default: max(BlendWeight, 0.2) (BlendWeight 0.15, 0.1, 0.35, 0.3, 0.6, 0.8).
/// The cycles' BaseSpeed / RateMin / RateMax are their AnimNodeSequences' (walk 200, run 500,
/// sprint 600 uu/s; 0.2..1.4, the sprint 0.6..1.4).
const WALK_STATES: [WalkState; 6] = [
    WalkState { min_speed: 0.0, blend: 0.2, fwd: "Stand", stiff: "Stand", bwd: "Stand", authored: 0.0, rate_min: 1.0, rate_max: 1.0 },
    WalkState { min_speed: 0.05, blend: 0.2, fwd: "sneakfwd", stiff: "sneakfwd", bwd: "sneakbwd", authored: 0.5, rate_min: 0.2, rate_max: 1.4 },
    WalkState { min_speed: 0.5, blend: 0.35, fwd: "walkfwd", stiff: "walkfwdstiff", bwd: "walkbwd", authored: 2.0, rate_min: 0.2, rate_max: 1.4 },
    WalkState { min_speed: 2.6, blend: 0.3, fwd: "runfwd", stiff: "runfwdstiff", bwd: "runbwd", authored: 5.0, rate_min: 0.2, rate_max: 1.4 },
    WalkState { min_speed: 4.0, blend: 0.6, fwd: "runfwd", stiff: "runfwdstiff", bwd: "runbwd", authored: 5.0, rate_min: 0.2, rate_max: 1.4 },
    WalkState { min_speed: 6.3, blend: 0.8, fwd: "SprintFwd", stiff: "SprintFwd", bwd: "runbwd", authored: 6.0, rate_min: 0.6, rate_max: 1.4 },
];

/// The walk cycles share one phase (synch group "Walk"); backward cycles sit
/// half a step later (SynchPosOffset 0.32 forward, 0.78 backward).
const SYNC_FWD: f32 = 0.32;
const SYNC_BWD: f32 = 0.78;

/// One cycle inside the locomotion blend.
#[derive(Clone, Debug)]
struct LocoCycle {
    seq: &'static str,
    weight: f32,
    fade_in: f32,
    sync: f32,
    authored: f32,
    rate_min: f32,
    rate_max: f32,
}

#[derive(Clone, Debug)]
enum Source {
    /// Plays at its own rate.
    Play { seq: &'static str, time: f32, rate: f32, looping: bool },
    /// Time set every frame from gameplay progress (vaults, pull-ups).
    Driven { seq: &'static str, time: f32 },
    /// Stand / walk / run / sprint blended by speed, one shared phase.
    Loco,
    /// Two sequences sampled at the same time and mixed by `k` (0 = all
    /// `a`): balance leans, and the swing's front/back poses.
    Blend2 { a: &'static str, b: &'static str, time: f32, rate: f32, k: f32 },
}

struct Layer {
    key: String,
    src: Source,
    weight: f32,
    fade_in: f32,
    dying: bool,
    /// Playback time at the end of the previous update (for sound notifies).
    last: f32,
}

struct OneShot {
    key: &'static str,
    until: f32,
    /// Plays straight after (attack wind-up â†’ strike).
    then: Option<&'static str>,
    /// How long whatever plays next takes to blend in when this one ends.
    blend_out: f32,
}

pub struct Driver {
    map: TrackMap,
    rest: Pose,
    layers: Vec<Layer>,
    loco_phase: f32,
    /// Current walking state (index into WALK_STATES) and the cycles being blended.
    walk_state: usize,
    loco: Vec<LocoCycle>,
    /// Sound cues from the animations that fired this update.
    fired: Vec<Notify>,
    idle_time: f32,
    speed: f32,
    oneshot: Option<OneShot>,
    /// Blend-in for the next animation after a one-shot ends.
    pending_blend: Option<f32>,
    /// The walk cycle the walking layer's sound cues last came from.
    loco_notify_seq: Option<&'static str>,
    /// WallRunVertical's rate for the current wallclimb (TdMove_WallClimb.ReachedWall).
    wallclimb_rate: f32,
    clock: f32,
    prev_state: Option<MoveState>,
    air_clip: &'static str,
    last_wall_side: f32,
    /// The hand the last attack was thrown with (its follow-through uses the same).
    melee_left: bool,
    /// Which sequences carry body travel (see [`is_travel`]).
    travel: HashMap<String, bool>,
    cam_bone: usize,
    rest_cam: Mat4,
    scratch: Pose,
    blended: Pose,
    pub globals: Vec<Mat4>,
    land: Option<LandFx>,
    hang_turn: Option<HangTurn>,
    /// A clip on the camera channel (PlayMoveAnim CNT_Camera): (sequence, time, blend in, out).
    cam_clip: Option<(&'static str, f32, f32, f32)>,
    prev_crouched: bool,
    /// TdAnimNodeTurn: legs' yaw while standing, the turn in progress (rate rad/s, end time),
    /// and how long the view has been between the safe and extended regions.
    leg_yaw: Option<f32>,
    leg_turn: Option<(f32, f32)>,
    standing_still: f32,
    /// Which idle `play_idle` plays next.
    next_idle: usize,
    /// TdMove_Walking's idle timer: standing still with the view untouched this long plays one
    /// of its UnarmedIdleAnims; moving, looking round or any move action restarts it.
    idle_timer: f32,
    last_view: (f32, f32),
    rng: u32,
    /// TdAnimNodeAgainstWallState weights (left, right arm).
    against_wall: (f32, f32),
    /// Lazy springs: WeaponYaw, WeaponPitch, WeaponRoll (SpineX), CameraRoll (EyeJoint).
    springs: [LazySpring; 4],
}

/// TdAnimNodeTurn defaults: SafeRegionLimit 25, ExtendedRegionLimit 65 (degrees), IdleTimer 0.95.
const TURN_SAFE_DEG: f32 = 25.0;
const TURN_EXTENDED_DEG: f32 = 65.0;
const TURN_IDLE_TIMER: f32 = 0.95;

/// TdPawn.CurrentGrabTurnType while hanging: looking more than 90Â° off the wall plays
/// `HangTurnLeft/RightStart` (a hand comes off the ledge and reaches out), holds the turned
/// idle, and looking back plays `â€¦End` (TdMove_Grab.UpdateViewRotation / OnCustomAnimEnd).
#[derive(Clone, Copy, Debug, PartialEq)]
struct HangTurn {
    left: bool,
    phase: HangTurnPhase,
    since: f32,
    /// View yaw relative to facing the wall (positive = left).
    look: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum HangTurnPhase {
    Start,
    Idle,
    End,
}

/// A landing as the game's 1P tree shows it (TdMove_Landing.LandNormal). Nothing replaces the
/// pose: the `LandNode` offset (TdAnimNodeLandOffset) dips the spine and eye, and landing on the
/// run mixes some `FallingLandMedium` into the run cycle (TdAnimNodeCustomBlend `LandingRun`).
/// Both scale with the landing amount, which a normal jump keeps at its 0.2 minimum.
#[derive(Clone, Copy, Debug)]
struct LandFx {
    amount: f32,
    t: f32,
    run: bool,
}

/// TdMove_Landing.GetLandingAmount: (fall - SoftLandingHeight 300) / (HardLandingHeight 530 -
/// 300), clamped to 0.2..1 (fall in metres here).
fn landing_amount(fall: f32) -> f32 {
    ((fall * 100.0 - 300.0) / (530.0 - 300.0)).clamp(0.2, 1.0)
}

/// AT_C1P `LandNode`'s profile (AimComponents, the CU pose `jumplandpose`): component-space
/// rotation and translation per bone at full landing.
const LAND_OFFSET: [(&str, [f32; 4], [f32; 3]); 2] = [
    ("SpineX", [0.069739, 0.0, 0.0, 0.997565], [0.0, 0.0, 2.137655]),
    ("EyeJoint", [-0.11308, 0.0, 0.0, 0.993586], [0.0, 8.24998, 0.0]),
];

/// LandNode's weight over time: in over LandInto (0.1 s), out over LandOut (0.4 s).
fn land_offset_weight(t: f32) -> f32 {
    if t < 0.1 { t / 0.1 } else { (1.0 - (t - 0.1) / 0.4).max(0.0) }
}

/// LandingRun's weight on `FallingLandMedium`: Activate(A, 0.6 A, 0.15, 0.4) blends to A over
/// 0.15 s, holds for 0.6 A, then back to the run over 0.4 s.
fn landing_run_weight(amount: f32, t: f32) -> f32 {
    let dur = 0.6 * amount;
    let up = |t: f32| (t / 0.15).min(1.0);
    amount * if t < dur { up(t) } else { up(dur) * (1.0 - (t - dur) / 0.4).max(0.0) }
}

/// TdMove_Climb.InitClimbAnimSeqNames: ClimbAnims[0..4].
fn climb_clip(pipe: bool, anim: u8) -> &'static str {
    match (pipe, anim) {
        (false, 0) => "LadderClimbUpLeftHand",
        (false, _) => "LadderClimbUpRightHand",
        (true, 0) => "PipeClimbUpLeftHand",
        (true, 1) => "PipeClimbUpRightHand",
        (true, 2) => "pipeclimbupfastlefthand",
        (true, _) => "pipeclimbupfastrighthand",
    }
}

/// Does this animation carry the body through space (root motion baked into
/// the hips), which gameplay already moves the player through? Vaults,
/// climbs, pull-ups, rolls and the ledge shimmy do; crouches and the hard
/// landing only lower the body.
fn is_travel(name: &str, max_camera_offset_cm: f32) -> bool {
    let n = name.to_ascii_lowercase();
    if n.starts_with("hangstrafe") || n.starts_with("hangfreestrafe") {
        return true;
    }
    // Falling onto your back and getting up: the body stays on the feet and the camera goes
    // down with it (the gameplay eye doesn't).
    // Moves whose root motion we apply at the end (stumbles, the back roll): the body stays on
    // the feet and the camera rides the clip.
    if n.starts_with("crouch")
        || n.starts_with("fallinglandhard")
        || n.starts_with("jumpturnlanding")
        || n.starts_with("stumblefwd")
        || n.starts_with("gethitstumble")
        || n.starts_with("evaderoll")
        || n.starts_with("fallinglandsoftlanding")
    {
        return false;
    }
    max_camera_offset_cm > 40.0
}

/// TdSkelControlLazySpring's lagging copy of a view angle.
#[derive(Clone, Copy, Debug)]
struct LazySpring {
    lazy: Option<f32>,
    interp: f32,
}

impl LazySpring {
    fn new(interp: f32) -> Self {
        LazySpring { lazy: None, interp }
    }

    /// Follow `source` (radians) and return how far behind it the copy is.
    fn lag(&mut self, source: f32, dt: f32) -> f32 {
        let wrap = |a: f32| (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let lazy = self.lazy.get_or_insert(source);
        let d = wrap(source - *lazy);
        // At least one rotator unit a frame, never past the target.
        let unit = std::f32::consts::TAU / 65536.0;
        let step = (dt / self.interp.max(1e-3) * d.abs()).max(unit).min(d.abs());
        *lazy = wrap(*lazy + step * d.signum());
        wrap(source - *lazy)
    }
}

/// The animated body for this frame.
#[derive(Clone, Copy, Debug)]
pub struct BodyAnim {
    /// Camera bone (Mirror's Edge mesh space, cm). In the game the camera
    /// *is* this bone, plus the player's look pitch.
    pub cam: Mat4,
    /// Yaw the animation gives the camera (radians about the up axis). The
    /// body is turned back by this so the view always faces gameplay yaw
    /// (turn animations would otherwise double the 180Â° turn).
    pub cam_yaw: f32,
    /// The camera channel's clip rotation (mesh space), added on top of the body's.
    pub cam_delta: Quat,
    /// The CameraRoll lazy spring on EyeJoint: the camera banks into a turn (radians, positive
    /// = right side down).
    pub cam_bank: f32,
    /// Standing still, the legs' yaw (TdAnimNodeTurn steps them round in 45/90 degree turns).
    pub legs_yaw: Option<f32>,
    /// 0..1, how much of the pose comes from travel animations. Those are
    /// placed so the camera bone sits at the gameplay eye; everything else
    /// stands with its feet on the gameplay feet.
    pub align: f32,
}

impl Driver {
    pub fn new(arms: &FaithArms) -> Self {
        let map = TrackMap::new(&arms.mesh, &arms.anims);
        let rest = Pose::rest(&arms.mesh);
        let cam_bone = arms.bone("CameraJoint").unwrap_or(0);
        let mut g = vec![];
        pose::globals(&arms.mesh, &rest, &mut g);
        let rest_cam = g[cam_bone];
        let mut d = Driver {
            map,
            rest: rest.clone(),
            layers: vec![Layer { key: "loco".into(), src: Source::Loco, weight: 1.0, fade_in: 0.0, dying: false, last: 0.0 }],
            loco_phase: 0.0,
            walk_state: 0,
            loco: vec![LocoCycle { seq: "Stand", weight: 1.0, fade_in: 0.2, sync: 0.0, authored: 0.0, rate_min: 1.0, rate_max: 1.0 }],
            fired: vec![],
            idle_time: 0.0,
            speed: 0.0,
            oneshot: None,
            pending_blend: None,
            loco_notify_seq: None,
            wallclimb_rate: 1.0,
            clock: 0.0,
            prev_state: None,
            air_clip: "jumpair",
            last_wall_side: 1.0,
            melee_left: false,
            travel: HashMap::new(),
            cam_bone,
            rest_cam,
            scratch: rest.clone(),
            blended: rest,
            globals: g,
            land: None,
            hang_turn: None,
            cam_clip: None,
            prev_crouched: false,
            leg_yaw: None,
            leg_turn: None,
            standing_still: 0.0,
            next_idle: 0,
            idle_timer: 35.0,
            last_view: (0.0, 0.0),
            rng: 0x2545_F491,
            against_wall: (0.0, 0.0),
            springs: [LazySpring::new(0.2), LazySpring::new(0.25), LazySpring::new(0.15), LazySpring::new(0.2)],
        };
        for (name, seq) in &arms.anims.sequences {
            let mut max = 0.0f32;
            for k in 0..10 {
                let mut p = d.rest.clone();
                pose::sample(seq, &d.map, &d.rest, seq.length * k as f32 / 10.0, true, &mut p);
                let off = d.camera_of(arms, &p).w_axis.truncate() - d.rest_cam.w_axis.truncate();
                max = max.max(off.length());
            }
            d.travel.insert(name.clone(), is_travel(name, max));
        }
        d
    }

    /// Camera bone transform for a pose (walks only the camera's bone chain).
    fn camera_of(&self, arms: &FaithArms, p: &Pose) -> Mat4 {
        let mut chain = vec![];
        let mut i = self.cam_bone;
        loop {
            chain.push(i);
            if i == 0 {
                break;
            }
            i = arms.mesh.bones[i].parent;
        }
        chain.iter().rev().fold(Mat4::IDENTITY, |m, &b| m * Mat4::from_rotation_translation(p.rot[b], p.pos[b]))
    }

    /// Game-speed-aware lengths so gameplay can match the animations.
    pub fn length(arms: &FaithArms, seq: &str) -> Option<f32> {
        arms.anims.sequences.get(seq).map(|s| s.length / s.rate.max(1e-3))
    }

    /// A clip's root yaw from its start (radians about up, negative = right), every 1/60 s:
    /// what UseRootRotation turns the pawn by.
    pub fn root_yaw_curve(&self, arms: &FaithArms, seq: &str) -> Option<Vec<f32>> {
        let sq = arms.anims.sequences.get(seq)?;
        let len = sq.length / sq.rate.max(1e-3);
        let n = (len * faith_move::TurnCurves::RATE).ceil() as usize;
        let mut p = self.rest.clone();
        let mut out = Vec::with_capacity(n + 1);
        let mut prev = 0.0f32;
        let mut unwrapped = 0.0f32;
        for k in 0..=n {
            let t = (k as f32 / faith_move::TurnCurves::RATE).min(len);
            pose::sample(sq, &self.map, &self.rest, t * sq.rate, false, &mut p);
            let q = p.rot[0];
            let yaw = 2.0 * q.y.atan2(q.w);
            if k > 0 {
                unwrapped += (yaw - prev + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            }
            prev = yaw;
            out.push(unwrapped);
        }
        Some(out)
    }

    /// A clip's root bone position from its start, every 1/60 s (the clip's own space and
    /// units): what UseRootMotion moves the pawn by.
    pub fn root_pos_curve(&self, arms: &FaithArms, seq: &str) -> Option<Vec<Vec3>> {
        let sq = arms.anims.sequences.get(seq)?;
        let len = sq.length / sq.rate.max(1e-3);
        let n = (len * faith_move::TurnCurves::RATE).ceil() as usize;
        let mut p = self.rest.clone();
        let mut out = Vec::with_capacity(n + 1);
        for k in 0..=n {
            let t = (k as f32 / faith_move::TurnCurves::RATE).min(len);
            pose::sample(sq, &self.map, &self.rest, t * sq.rate, false, &mut p);
            out.push(p.pos[0]);
        }
        let first = out.first().copied().unwrap_or_default();
        Some(out.into_iter().map(|v| v - first).collect())
    }

    fn want(&mut self, key: &str, src: Source, blend: f32) {
        if self.layers.last().is_some_and(|l| l.key == key && !l.dying) {
            return;
        }
        let blend = self.pending_blend.take().map_or(blend, |b| b.max(blend));
        for l in &mut self.layers {
            l.dying = true;
        }
        self.layers.push(Layer { key: key.into(), src, weight: 0.0, fade_in: blend.max(1e-3), dying: false, last: -1e-4 });
    }

    fn oneshot(&mut self, seq: &'static str, arms: &FaithArms, hold: Option<f32>, blend: f32) {
        let len = Self::length(arms, seq).unwrap_or(0.5);
        let dur = hold.unwrap_or(len).min(len);
        // Cut short, a one-shot hasn't come back to a resting pose yet, so the
        // next animation eases in over longer (landings: AT_C1P LandOut 0.4 s).
        let blend_out = if dur < len - 0.05 { 0.4 } else { 0.2 };
        self.oneshot = Some(OneShot { key: seq, until: self.clock + dur, then: None, blend_out });
        let key = format!("{seq}#{}", self.clock);
        self.want(&key, Source::Play { seq, time: 0.0, rate: 1.0, looping: false }, blend);
    }

    /// A one-shot picked up part way through (`start` seconds in).
    fn oneshot_from(&mut self, seq: &'static str, arms: &FaithArms, start: f32, blend: f32) {
        let len = Self::length(arms, seq).unwrap_or(0.5);
        if start >= len - 0.02 {
            return;
        }
        self.oneshot = Some(OneShot { key: seq, until: self.clock + (len - start), then: None, blend_out: 0.2 });
        let key = format!("{seq}#{}", self.clock);
        self.want(&key, Source::Play { seq, time: start, rate: 1.0, looping: false }, blend);
    }

    fn set_blend_out(&mut self, t: f32) {
        if let Some(o) = &mut self.oneshot {
            o.blend_out = t;
        }
    }

    /// A one-shot played at `rate` (PlayMoveAnim's rate), blending in over `blend` and out over
    /// `out`.
    fn oneshot_rate(&mut self, seq: &'static str, arms: &FaithArms, rate: f32, blend: f32, out: f32) {
        let len = Self::length(arms, seq).unwrap_or(0.5) / rate;
        self.oneshot = Some(OneShot { key: seq, until: self.clock + len, then: None, blend_out: out });
        let key = format!("{seq}#{}", self.clock);
        self.want(&key, Source::Play { seq, time: 0.0, rate, looping: false }, blend);
    }

    /// An attack's wind-up (each move's TriggerMove: PlayMoveAnim(clip, rate, blend in, out)).
    fn melee(&mut self, kind: MeleeKind, left: bool, c: &Controller, arms: &FaithArms) {
        self.melee_left = left;
        let lr = |l: &'static str, r: &'static str| if left { l } else { r };
        match kind {
            MeleeKind::Punch => self.oneshot_rate(lr("MeleeStartLeft", "MeleeStartRight"), arms, 1.5, 0.1, 0.1),
            MeleeKind::Crouch => self.oneshot_rate("MeleeCrouchStart", arms, 1.0, 0.1, 0.1),
            MeleeKind::AirKick => match c.melee.map_or(0, |m| m.air_type) {
                0 => self.oneshot_rate("MeleeInAir", arms, 1.0, 0.1, 0.2),
                1 => self.oneshot_rate("MeleeInAirStill", arms, 1.0, 0.1, 0.2),
                _ => self.oneshot_rate("MeleeFromAbove", arms, 1.0, 0.1, 0.1),
            },
            MeleeKind::SlideKick => self.oneshot_rate("MeleeSlide", arms, 1.0, 0.1, 0.1),
            MeleeKind::WallRunKick => self.oneshot_rate(lr("MeleeWallRunLeft", "MeleeWallRunRight"), arms, 1.0, 0.1, 0.2),
            // TdMove_MeleeVault.TriggerMove: PlayMoveAnim(MeleeVaultOver, 1.0, 0.1, 0.1).
            MeleeKind::VaultKick => self.oneshot_rate("MeleeVaultOver", arms, 1.0, 0.1, 0.1),
        }
    }

    /// The follow-through (TriggerHit / TriggerMiss).
    fn melee_outcome(&mut self, kind: MeleeKind, hit: bool, c: &Controller, arms: &FaithArms) {
        let left = self.melee_left;
        let lr = |l: &'static str, r: &'static str| if left { l } else { r };
        // Another punch queued (ComboQueuedActions > 0): it blends out slower, into that one.
        let queued = c.melee.is_some_and(|m| m.queued > 0);
        match (kind, hit) {
            // TdMove_Melee: hits at 1.5 x (blend 0.2 / 0.1, 0.3 with a punch queued); misses
            // BlendInMissed 0.08, BlendOutMissed 0.1 (0.6 queued).
            (MeleeKind::Punch, true) => self.oneshot_rate(lr("MeleeHitLeft", "MeleeHitRight"), arms, 1.5, 0.2, if queued { 0.3 } else { 0.1 }),
            (MeleeKind::Punch, false) => self.oneshot_rate(lr("MeleeMissedLeft", "MeleeMissedRight"), arms, 1.5, 0.08, if queued { 0.6 } else { 0.1 }),
            // TdMove_MeleeCrouch: MeleeCrouchHit either way (0.1 / 0.2).
            (MeleeKind::Crouch, _) => self.oneshot_rate("MeleeCrouchHit", arms, 1.0, 0.1, 0.2),
            // TdMove_MeleeAir.TriggerDamage: MeleeInAirHit (0.1 / 0.2).
            (MeleeKind::AirKick, true) => self.oneshot_rate("MeleeInAirHit", arms, 1.0, 0.1, 0.2),
            _ => {}
        }
    }

    /// TdPlayerPawn's forearm morphs (Init1pArms: <Side>ForeArmRollBlend90 / 90m): how far
    /// <side>ForeArmRoll is twisted from its rest (radians about the bone's length axis).
    pub fn forearm_twist(&self, arms: &FaithArms, side: &str) -> f32 {
        let Some(b) = arms.bone(&format!("{side}ForeArmRoll")) else { return 0.0 };
        let mut q = self.rest.rot[b].inverse() * self.blended.rot[b];
        if q.w < 0.0 {
            q = -q;
        }
        2.0 * q.x.atan2(q.w)
    }

    /// The arms mesh's forearm twist morphs for this pose (TdPlayerPawn.Init1pArms), as
    /// per-vertex deltas (mesh space) into `out`: each side's ForeArmRollBlend90 (rolling one
    /// way) or Blend90m (the other), weighted by its roll bone's twist, full at 90 degrees.
    /// Without them the skinning pinches the forearm where it twists. Empty if the mesh has none.
    pub fn forearm_morphs(&self, arms: &FaithArms, out: &mut Vec<Vec3>) {
        out.clear();
        if arms.morphs.is_empty() {
            return;
        }
        out.resize(arms.mesh.vertices.len(), Vec3::ZERO);
        for side in ["Left", "Right"] {
            let twist = self.forearm_twist(arms, side);
            let w = (twist.abs() / std::f32::consts::FRAC_PI_2).min(1.0);
            let name = if twist >= 0.0 { format!("{side}ForeArmRollBlend90") } else { format!("{side}ForeArmRollBlend90m") };
            if let Some(t) = arms.morphs.iter().find(|t| t.name == name) {
                for (v, d) in &t.deltas {
                    out[*v as usize] += Vec3::from(*d) * w;
                }
            }
        }
    }

    /// Faith's first-person idles (TdAnimSet 1P), played on request while standing.
    pub const IDLES: [&'static str; 4] = ["standidle1", "standidle2", "standidle3", "edgedetectionidle"];

    /// Play the next idle (standing still on the ground only): whether it started. Moving ends it.
    pub fn play_idle(&mut self, c: &Controller, arms: &FaithArms) -> bool {
        if c.state != MoveState::Ground || c.crouched || c.horizontal_speed() > 0.3 {
            return false;
        }
        let seq = Self::IDLES[self.next_idle % Self::IDLES.len()];
        self.next_idle += 1;
        // As TdMove_Walking plays its own idles: PlayMoveAnim(..., 0.4, 0.4).
        self.oneshot(seq, arms, None, 0.4);
        self.set_blend_out(0.4);
        true
    }

    /// How far the view turns in a frame (radians) before it counts as looking around: a mouse
    /// resting under a hand still twitches by a count now and then.
    const VIEW_STILL: f32 = 0.003;

    /// TdMove_Walking.UnarmedIdleAnims (AnimName, CNT_Canned, bResetCameraLook).
    pub const AUTO_IDLES: [&'static str; 3] = ["standidle1", "standidle2", "standidle3"];
    /// TdMove_Walking.TriggerIdleAnimMinTime / MaxTime.
    const IDLE_MIN_TIME: f32 = 30.0;
    const IDLE_MAX_TIME: f32 = 40.0;

    fn frand(&mut self) -> f32 {
        // xorshift32
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    /// TdMove_Walking.GetNewIdleTriggerTime.
    fn new_idle_time(&mut self) -> f32 {
        Self::IDLE_MIN_TIME + self.frand() * (Self::IDLE_MAX_TIME - Self::IDLE_MIN_TIME)
    }

    pub fn update(&mut self, dt: f32, c: &Controller, arms: &FaithArms) -> BodyAnim {
        self.clock += dt;
        let raw_speed = c.horizontal_speed();
        // TdMove_Walking: walking (CurrentWalkingState > 0), turning the view or any move action
        // stops an idle and restarts the timer; standing still long enough plays one
        // (OnIdleTimer -> PlayIdle: a random UnarmedIdleAnims entry).
        let view_moved = (c.yaw - self.last_view.0).abs() > Self::VIEW_STILL || (c.pitch - self.last_view.1).abs() > Self::VIEW_STILL;
        self.last_view = (c.yaw, c.pitch);
        // Looking round doesn't count here (only moving does): an idle plays, and keeps
        // playing, while the mouse moves.
        let _ = view_moved;
        let still = c.state == MoveState::Ground && !c.crouched && raw_speed <= 0.3 && c.events.is_empty();
        if !still {
            self.idle_timer = self.new_idle_time();
        } else {
            self.idle_timer -= dt;
            if self.idle_timer <= 0.0 {
                let pick = ((self.frand() * Self::AUTO_IDLES.len() as f32) as usize).min(Self::AUTO_IDLES.len() - 1);
                let seq = Self::AUTO_IDLES[pick];
                self.oneshot(seq, arms, None, 0.4);
                self.set_blend_out(0.4);
                // OnCustomAnimEnd sets the timer again once the idle is over.
                self.idle_timer = self.new_idle_time() + Self::length(arms, seq).unwrap_or(0.0);
            }
        }
        self.speed += (raw_speed - self.speed) * (1.0 - (-10.0 * dt).exp());
        let speed = self.speed;
        let right = Vec3::new(c.yaw.cos(), 0.0, -c.yaw.sin());
        let fwd = c.forward();
        let prev = self.prev_state;
        let entered = |f: fn(&MoveState) -> bool| prev.as_ref().is_none_or(|p| !f(p)) && f(&c.state);

        // ---- events â†’ one-shots
        for e in &c.events {
            match *e {
                // TdMove_Jump: by her speed along her facing, JumpStill under 5 uu/s (in 0.15),
                // JumpSlow under LongJumpNormalThreshold (500 uu/s), else JumpFast (in 0.1).
                // (Past 500 the game plays JumpSlow too when its trace finds ground under where
                // she'll land; that prediction isn't made here.)
                MoveEvent::Jump => {
                    let along = Vec3::new(c.vel.x, 0.0, c.vel.z).dot(fwd);
                    self.air_clip = if along < 0.05 { "jumpstill" } else if along < 5.0 { "JumpSlow" } else { "jumpfast" };
                }
                // TdMove_Landing.LandNormal, then straight back to walking (EndLanding): see LandFx.
                MoveEvent::Land { fall, .. } if matches!(c.state, MoveState::Ground) => {
                    self.land = Some(LandFx { amount: landing_amount(fall), t: 0.0, run: raw_speed >= 2.6 });
                }
                MoveEvent::HardLand => {
                    // TdMove_Landing.LandHard: FallingLandHard or FallingLandHard2 at random,
                    // blend in 0.05, out 0.2, played to the end.
                    let seq = if (self.clock * 1000.0) as u32 % 2 == 0 { "fallinglandhard" } else { "fallinglandhard2" };
                    // Control comes back as it starts to blend out (Tuning::hard_land_stun).
                    self.oneshot(seq, arms, Some(c.tuning.hard_land_stun), 0.05);
                    self.set_blend_out(0.2);
                }
                // TdMove_180TurnInAir: PlayMoveAnim(CNT_Weapon, Taunt, 1.0, 0.1, 0.2).
                MoveEvent::Taunt => {
                    self.oneshot("Taunt", arms, None, 0.1);
                    self.set_blend_out(0.2);
                }
                // TdMove_DodgeJump: dodgejumpleft/right (JumpBlendInTime 0.1, out 0.2).
                MoveEvent::Dodge { dir } => {
                    let seq = if dir.dot(right) < 0.0 { "dodgejumpleft" } else { "dodgejumpright" };
                    self.oneshot_rate(seq, arms, 1.0, 0.1, 0.2);
                }
                // TdMove_WallrunDodgeJump: the same clips, 0.2 / 0.2.
                MoveEvent::WallRunDodge { dir } => {
                    let seq = if dir.dot(right) < 0.0 { "dodgejumpleft" } else { "dodgejumpright" };
                    self.oneshot_rate(seq, arms, 1.0, 0.2, 0.2);
                }
                // TdMove_WallClimbDodgeJump: JumpSlow (JumpBlendInTime / OutTime 0.2 / 0.2).
                MoveEvent::WallClimbDodge { .. } => {
                    self.air_clip = "JumpSlow";
                    self.oneshot_rate("JumpSlow", arms, 1.0, 0.2, 0.2);
                }
                // TdMove_WallrunJump.StartMove: WallrunJumpLeft/Right (blend 0.2) only when you jump
                // looking out from the wall (PushSpeed > 0.6; after Q it's 1), else JumpSlow.
                MoveEvent::WallJump => {
                    let push = match prev {
                        Some(MoveState::WallRun { normal, .. }) => fwd.dot(normal),
                        _ => 1.0,
                    };
                    // PlayMoveAnim(3, ..., 1.0, 0.2, 0.2): the whole clip (landing ends it).
                    if push > 0.6 {
                        let seq = if self.last_wall_side < 0.0 { "WallrunJumpLeft" } else { "wallrunjumpright" };
                        self.oneshot_rate(seq, arms, 1.0, 0.2, 0.2);
                    } else {
                        self.air_clip = "JumpSlow";
                        self.oneshot_rate("JumpSlow", arms, 1.0, 0.2, 0.2);
                    }
                }
                MoveEvent::WallKick => match prev {
                    // TdMove_WallClimb180TurnJump.JumpFromWall: you're launched now, but
                    // wallrunvertical180turn keeps playing until the move's 0.6 s
                    // JumpTimeWindow (from the turn) runs out; then OnTimer plays
                    // WallrunJumpLeft (blend 0.1, out 0.2).
                    Some(MoveState::WallClimbTurned { t, .. }) => {
                        let len = Self::length(arms, "wallrunvertical180turn").unwrap_or(0.63);
                        let hold = (c.tuning.wallkick_window - t).max(0.0).min(len - t);
                        if hold > 0.02 {
                            self.oneshot_from("wallrunvertical180turn", arms, t, 0.0);
                            if let Some(o) = &mut self.oneshot {
                                o.until = self.clock + hold;
                                o.then = Some("WallrunJumpLeft");
                            }
                        } else {
                            self.oneshot("WallrunJumpLeft", arms, None, 0.1);
                        }
                    }
                    // TdMove_GrabJump: HangTurnJump (blend 0.2).
                    _ => self.oneshot("hangturnjump", arms, None, 0.2),
                },
                // TdMove_180TurnInAir.StartMove: PlayMoveAnim JumpTurnFly, blend 0.1.
                MoveEvent::Turn180 if c.state == MoveState::Air => self.oneshot_rate("JumpTurnFly", arms, 1.0, 0.1, 0.1),
                // TdMove_180Turn.StartMove: RunTurn180 moving, StandTurn180Right standing
                // (TurnAnimBlendInTime / OutTime 0.2).
                MoveEvent::Turn180 if c.state == MoveState::Ground => {
                    let seq = if c.horizontal_speed() > 0.1 { "RunTurn180" } else { "StandTurn180Right" };
                    self.oneshot(seq, arms, None, 0.2);
                    self.set_blend_out(0.2);
                }
                // TdMove_Swing's turn: Swing180 (blend 0.2, out 0.3).
                MoveEvent::Turn180 if matches!(c.state, MoveState::Swing { .. }) => {
                    self.oneshot("swing180", arms, None, 0.2);
                    self.set_blend_out(0.3);
                }
                // TdMove_IntoGrab.ReachedPreciseLocation, by the speed she came down at
                // (IntoGrabSpeed): faster than HangHardImpactMinZSpeed (-1000 uu/s)
                // HangHardStart3 with the gethitfront camera jolt, faster than
                // HangImpactMinZSpeed (-600) HangHardStart2, else HangHardStart; 1.0, in 0.1,
                // out 0.2, the whole clip (leaving the hang ends it).
                MoveEvent::LedgeGrab => {
                    let v = c.grab_speed;
                    let seq = if v < -10.0 { "HangHardStart3" } else if v < -6.0 { "HangHardStart2" } else { "HangHardStart" };
                    self.oneshot_rate(seq, arms, 1.0, 0.1, 0.2);
                    if v < -10.0 {
                        self.cam_clip = Some(("gethitfront", 0.0, 0.05, 0.2));
                    }
                }
                MoveEvent::Death => {
                    self.oneshot = None;
                    self.want("loco", Source::Loco, 0.01);
                }
                // TdMove_SpringBoard: SpringBoardRightLeg if her left leg is forward
                // (IsLeftLegForward: the walk synch master past half its cycle), else
                // SpringBoardLeftLeg; 1.0, in 0.15, out 0.25.
                MoveEvent::SpringBoard => {
                    let seq = if self.left_leg_forward() { "SpringBoardRightLeg" } else { "SpringBoardLeftLeg" };
                    self.oneshot_rate(seq, arms, 1.0, 0.15, 0.25);
                }
                MoveEvent::BalanceFall => {
                    let lean = if let MoveState::Balance { lean, .. } = prev.unwrap_or(c.state) { lean } else { 0.0 };
                    let seq = if lean < 0.0 { "walkbalancefalloffleft" } else { "walkbalancefalloffright" };
                    // TdMove_Balance: 1.0, in 0.3, out 0.3.
                    self.oneshot_rate(seq, arms, 1.0, 0.3, 0.3);
                }
                // TdMove_IntoZipLine: PlayMoveAnim(ZiplineStart, 0.3 / time to the line, in 0.2,
                // out 0.4).
                MoveEvent::ZipStart => self.oneshot_rate("ziplinestart", arms, c.zip_start_rate, 0.2, 0.4),
                // PrepareForForwardImpact: StopCustomAnim(0.1), then ziplineintohitwall looping
                // (the state's own animation below, blend in 0.3).
                MoveEvent::ZipBrace => {
                    if self.oneshot.as_ref().is_some_and(|o| o.key.starts_with("zipline")) {
                        self.oneshot = None;
                        self.pending_blend = Some(0.1);
                    }
                }
                // PlayForwardImpact: ziplinehitwall (in 0.1, out 0.2).
                MoveEvent::ZipEnd { hit_wall: true } => {
                    self.oneshot("ziplinehitwall", arms, None, 0.1);
                    self.set_blend_out(0.2);
                }
                // TdMove_ZipLine.StopMove otherwise: SwingJumpOff (in 0.2, out 0.2).
                MoveEvent::ZipEnd { hit_wall: false } => self.oneshot_rate("swingjumpoff", arms, 1.0, 0.2, 0.2),
                // TdMove_Barge: MeleeKickObject standing (in 0.1, out 0.1), played to its end.
                MoveEvent::Barge { hands: false } => self.oneshot_rate("meleekickobject", arms, 1.0, 0.1, 0.1),
                // The door gives: BargeOutLeft (in 0, out 0.2), played to its end.
                MoveEvent::DoorOpened { .. } if matches!(c.state, MoveState::Barge { hands: true, .. }) || matches!(prev, Some(MoveState::Barge { hands: true, .. })) => {
                    self.oneshot_rate("bargeoutleft", arms, 1.0, 0.0, 0.2)
                }
                // TdMove_Swing.AnimBlendTime = 0.15
                MoveEvent::SwingStart => self.oneshot("swinghardstart", arms, None, 0.15),
                // TdMove_Swing.JumpOff: SwingJumpOff (1.0, in 0.2, out 0.2).
                MoveEvent::SwingJump => self.oneshot_rate("swingjumpoff", arms, 1.0, 0.2, 0.2),
                // TdMove_IntoClimb.PlayStartAnimation (LadderEnterTop is the state's own).
                MoveEvent::ClimbStart { start, pipe } => {
                    let (seq, blend_in) = match (start, pipe) {
                        (ClimbStart::HangLeft, false) => ("LadderClimbHangStartLeft", 0.15),
                        (ClimbStart::HangLeft, true) => ("PipeClimbHangStartLeft", 0.15),
                        (ClimbStart::HangRight, false) => ("LadderClimbHangStartRight", 0.15),
                        (ClimbStart::HangRight, true) => ("PipeClimbHangStartRight", 0.15),
                        // The ladder's hard start isn't in Faith's AnimSet: the soft one.
                        (ClimbStart::HangHard, false) | (ClimbStart::Hang, false) => ("LadderClimbHangStart", 0.1),
                        (ClimbStart::HangHard, true) => ("PipeClimbHangStartHard", 0.1),
                        (ClimbStart::Hang, true) => ("PipeClimbHangStart", 0.1),
                        (ClimbStart::PipeStart, _) => ("PipeClimbStart", 0.15),
                        (ClimbStart::None, _) | (ClimbStart::EnterTop, _) => ("", 0.0),
                    };
                    if !seq.is_empty() {
                        self.oneshot_rate(seq, arms, 1.0, blend_in, 0.25);
                    }
                }
                // TdMove_Climb.LetGo: PipeExitBottom (1.0, in 0.1, out 0.4).
                MoveEvent::ClimbLetGo => self.oneshot_rate("PipeExitBottom", arms, 1.0, 0.1, 0.4),
                // TdMove_SwingJump.StartMove: SwingOff (1.0, in 0.2, out 0.2).
                MoveEvent::SwingToSwing => self.oneshot_rate("swingoff", arms, 1.0, 0.2, 0.2),
                MoveEvent::Melee { kind, left } => self.melee(kind, left, c, arms),
                // TdMove_AutoStepUp.StartMove: autostepuprightleg (0.8, in 0.15, out 0.25).
                MoveEvent::StepUp => self.oneshot_rate("autostepuprightleg", arms, 0.8, 0.15, 0.25),
                // TdMove_RumpSlide.StartSliding: crouchslideintoend45 (1.0, in 0.15, out 0.2).
                MoveEvent::RumpSlide => self.oneshot_rate("crouchslideintoend45", arms, 1.0, 0.15, 0.2),
                // TdMove_Vertigo.StartMove: PlayMoveAnim(edgedetection, 1.0, in 0.28, out 0.28).
                MoveEvent::Vertigo => self.oneshot_rate("edgedetection", arms, 1.0, 0.28, 0.28),
                // TdMove_GrabTransfer.PlayTransferAnimation: hangtransferup (1.0, in 0.1, out 0.1).
                MoveEvent::GrabTransfer => self.oneshot_rate("hangtransferup", arms, 1.0, 0.1, 0.1),
                // TdMOVE_Disarm.PlayDisarmStart: PlayMoveAnim(DisarmAnim, 1.0, in 0.1, out 0.2).
                MoveEvent::Takedown { anim, .. } => {
                    let seq = faith_move::TAKEDOWN_ANIMS[(anim as usize).min(3)];
                    self.oneshot_rate(seq, arms, 1.0, 0.1, 0.2);
                }
                MoveEvent::MeleeOutcome { kind, hit } => self.melee_outcome(kind, hit, c, arms),
                _ => {}
            }
        }
        if let (Some(MoveState::Vault(v)), false) = (prev, matches!(c.state, MoveState::Vault(_))) {
            // TdMove_SpeedVault.StartMove: PlayMoveAnim(blend in 0.15, out 0.2). The move hands
            // back to walking before the clip's run-out ends; it keeps playing over the run.
            if matches!(c.state, MoveState::Ground | MoveState::Air) {
                self.oneshot_from(v.anim(), arms, v.t, 0.0);
                self.set_blend_out(0.2);
            }
        }
        // TdMove_Vertigo.StopMove: StopCustomAnim(0.4).
        if matches!(prev, Some(MoveState::Vertigo { .. })) && !matches!(c.state, MoveState::Vertigo { .. }) {
            if self.oneshot.as_ref().is_some_and(|o| o.key == "edgedetection") {
                self.oneshot = None;
                self.pending_blend = Some(0.4);
            }
        }
        // TdMove_Slide.StopMove: crouchslidetocrouch (blend in 0.1, out 0.2).
        if let (Some(MoveState::Slide { .. }), MoveState::Ground) = (prev, c.state) {
            self.oneshot("crouchslidetocrouch", arms, None, 0.1);
            self.set_blend_out(0.2);
        }
        if entered(|s| matches!(s, MoveState::WallRun { .. })) {
            if let MoveState::WallRun { normal, .. } = c.state {
                self.last_wall_side = if normal.dot(right) < 0.0 { 1.0 } else { -1.0 };
                // TdMove_WallRun.ReachedWall (PlayCameraHitWallEffect): the camera-channel jolt.
                let seq = if self.last_wall_side > 0.0 { "wallrunimpactright" } else { "wallrunimpactleft" };
                self.cam_clip = Some((seq, 0.0, 0.15, 0.15));
                // WallRunningIntoWallrunBlendInTime / OutTime 0.2 / 0.2, at
                // WallrunStartUpperBodyAnimPlayRate (0.6) with wall above her head, else 1.
                let seq = if self.last_wall_side > 0.0 { "wallrunrightstart" } else { "wallrunleftstart" };
                let rate = if c.wallrun_wall_above { 0.6 } else { 1.0 };
                self.oneshot_rate(seq, arms, rate, 0.2, 0.2);
            }
        }
        if entered(|s| matches!(s, MoveState::WallClimb { .. })) {
            // TdMove_WallClimb.ReachedWall: WallRunVertical's rate is 0.4 over the time to the
            // top of the climb (the vertical speed over twice WallClimbingGravity, as the game
            // has it), 0.1..1.
            let to_top = (c.vel.y / (2.0 * c.tuning.wallclimb_gravity)).max(1e-3);
            self.wallclimb_rate = (0.4 / to_top).clamp(0.1, 1.0);
        }
        // TdMove_WallClimb.StopMove: StopCustomAnim(0.25).
        if matches!(prev, Some(MoveState::WallClimb { .. })) && !matches!(c.state, MoveState::WallClimb { .. }) {
            self.pending_blend = Some(self.pending_blend.unwrap_or(0.0).max(0.25));
        }
        self.update_hang_turn(c, arms);
        self.update_stand_turn(c, arms, raw_speed, dt);
        // TdMove_Crouch.StopMove into walking: CrouchIntoStand on the camera channel (0.2/0.2).
        if self.prev_crouched && !c.crouched && c.state == MoveState::Ground {
            self.cam_clip = Some(("CrouchIntoStand", 0.0, 0.2, 0.2));
        }
        self.prev_crouched = c.crouched;

        // A one-shot keeps playing until it ends, unless the move changed kind.
        let kind_changed = prev.is_some_and(|p| std::mem::discriminant(&p) != std::mem::discriminant(&c.state));
        if let Some(o) = &self.oneshot {
            let hard_state = matches!(c.state, MoveState::Traverse(_) | MoveState::Vault(_) | MoveState::LayOnGround { .. } | MoveState::Barge { .. } | MoveState::AirBarge { .. } | MoveState::IntoClimb { .. } | MoveState::Climb { .. } | MoveState::ClimbExit { .. } | MoveState::Stumble { .. } | MoveState::SoftLand { .. } | MoveState::LedgeHang { .. } | MoveState::Roll { .. } | MoveState::Slide { .. });
            // A move's own one-shot survives only while you're still in that
            // move: the grab must not keep playing into the pull-up (that
            // left the hands reaching far above the ledge as you climbed).
            let keeps = match c.state {
                MoveState::LedgeHang { .. } => o.key.starts_with("Hang"),
                MoveState::Traverse(tr) => tr.kind == TraverseKind::SpringBoard && o.key.starts_with("SpringBoard"),
                MoveState::Swing { .. } => o.key.starts_with("swing"),
                MoveState::ZipLine { .. } => o.key.starts_with("zipline"),
                MoveState::Barge { .. } => o.key == "meleekickobject" || o.key == "bargeoutleft",
                MoveState::Takedown { .. } => o.key.starts_with("Snatch"),
                MoveState::StepUp { .. } => o.key == "autostepuprightleg",
                MoveState::RumpSlide { .. } => o.key == "crouchslideintoend45",
                MoveState::GrabTransfer { .. } => o.key == "hangtransferup",
                MoveState::Vertigo { .. } => o.key == "edgedetection",
                MoveState::SwingJump { .. } => o.key == "swingoff",
                MoveState::IntoClimb { .. } => o.key.contains("ClimbHangStart") || o.key == "PipeClimbStart",
                // The start clip plays on over the hold (move input waits for it).
                MoveState::Climb { mv: None, fast: false, .. } => o.key.contains("ClimbHangStart") || o.key == "PipeClimbStart",
                _ => false,
            };
            // A standing landing gives way as soon as you run off; so does an idle.
            let idle = Self::IDLES.iter().any(|i| *i == o.key);
            let run_off = (o.key == "JumpLand" && raw_speed > 2.6)
                || (idle && (raw_speed > 0.3 || c.state != MoveState::Ground || c.crouched));
            // A jump's own clip (dodge, wall jump-off, kick-off, springboard) belongs to that
            // jump: touching down ends it, blending out over the move's JumpBlendOutTime (0.2).
            // The dodge clips run 0.83 s against ~0.4 s in the air, so without this the dodge
            // pose would hang on over the run cycle.
            let air_clip = ["dodgejump", "wallrunjump", "JumpSlow", "JumpTurnFly", "swingjumpoff", "swingoff", "MeleeInAir", "MeleeFromAbove", "SpringBoard"]
                .iter()
                .any(|k| o.key.to_ascii_lowercase().starts_with(&k.to_ascii_lowercase()));
            let touched_down = prev.is_some_and(|p| matches!(p, MoveState::Air))
                && matches!(c.state, MoveState::Ground | MoveState::Slide { .. } | MoveState::Roll { .. });
            let landed_out = air_clip && touched_down;
            if self.clock >= o.until || run_off || landed_out || (kind_changed && hard_state && !keeps) {
                let then = if self.clock >= o.until { o.then } else { None };
                if then.is_none() {
                    self.pending_blend = Some(if run_off { 0.3 } else if landed_out { 0.2 } else { o.blend_out });
                }
                self.oneshot = None;
                if let Some(next) = then {
                    self.oneshot(next, arms, None, 0.05);
                }
            }
        }

        // ---- the state's own animation (when no one-shot is playing)
        if self.oneshot.is_none() {
            match c.state {
                MoveState::Ground => {
                    let back = c.vel.dot(fwd) < -0.5;
                    if c.crouched {
                        if speed > 0.5 {
                            self.want("crouchfwd", Source::Play { seq: "crouchfwd", time: 0.0, rate: (speed / 2.0).clamp(0.6, 1.4), looping: true }, 0.2);
                        } else {
                            self.want("crouchstill", Source::Play { seq: "crouchstill", time: 0.0, rate: 1.0, looping: true }, 0.25);
                        }
                    } else {
                        let _ = back;
                        self.want("loco", Source::Loco, 0.2);
                    }
                }
                // TdMove_IntoClimb: LadderEnterTop (in 0.25) over the top; else onto the step.
                MoveState::IntoClimb { t, entering: true, .. } => {
                    self.want("LadderEnterTop", Source::Driven { seq: "LadderEnterTop", time: t }, 0.25);
                }
                MoveState::IntoClimb { ladder, .. } => {
                    let seq = if ladder.pipe { "PipeClimbUpLeftHandStill" } else { "LadderClimbUpLeftHandStill" };
                    self.want(seq, Source::Play { seq, time: 0.0, rate: 1.0, looping: true }, 0.15);
                }
                // TdMove_Climb.Climb: the step's clip (in 0.1), backwards going down, kept in step
                // with the move.
                MoveState::Climb { ladder, mv: Some(m), .. } if m.anim.is_some() => {
                    let seq = climb_clip(ladder.pipe, m.anim.unwrap_or(0));
                    let len = Self::length(arms, seq).unwrap_or(0.33);
                    let k = (m.t / m.dur.max(1e-3)).clamp(0.0, 1.0);
                    let time = if m.down { (1.0 - k) * len } else { k * len };
                    let key = format!("{seq}#{}", if m.down { "down" } else { "up" });
                    self.want(&key, Source::Driven { seq, time }, 0.1);
                }
                // Sliding down: ...ClimbDownFast, looping (ClimbDownBlendInTime 0.5).
                MoveState::Climb { ladder, fast: true, .. } => {
                    let seq = if ladder.pipe { "PipeClimbDownFast" } else { "LadderClimbDownFast" };
                    self.want(seq, Source::Play { seq, time: 0.0, rate: 1.0, looping: true }, 0.5);
                }
                // Holding on: the hand's still pose (IdleBlendInTime 0.05).
                MoveState::Climb { ladder, left, .. } => {
                    let seq = match (ladder.pipe, left) {
                        (false, true) => "LadderClimbUpLeftHandStill",
                        (false, false) => "LadderClimbUpRightHandStill",
                        (true, true) => "PipeClimbUpLeftHandStill",
                        (true, false) => "PipeClimbUpRightHandStill",
                    };
                    self.want(seq, Source::Play { seq, time: 0.0, rate: 1.0, looping: true }, 0.05);
                }
                // ExitAtTop: the exit clip (in 0.1).
                MoveState::ClimbExit { ladder, t, left, .. } => {
                    let seq = match (ladder.pipe, left) {
                        (false, true) => "LadderExitTopLeftHand",
                        (false, false) => "LadderExitTopRightHand",
                        (true, true) => "pipeexittoplefthand",
                        (true, false) => "pipeexittoprighthand",
                    };
                    self.want(seq, Source::Driven { seq, time: t }, 0.1);
                }
                MoveState::SwingJump { .. } => {
                    let seq = self.air_clip;
                    self.want(seq, Source::Play { seq, time: 0.0, rate: 1.0, looping: false }, 0.2);
                }
                MoveState::Air => {
                    if c.soft_brace {
                        // TdMove_SoftLanding: fallinglandintosoftlanding, looping, blend in 0.6.
                        self.want("fallinglandintosoftlanding", Source::Play { seq: "fallinglandintosoftlanding", time: 0.0, rate: 1.0, looping: true }, 0.6);
                    } else if c.turned_in_air {
                        // TdMove_180TurnInAir lasts until you land, and the AnimTree's
                        // 180TurnInAir branch holds jumpturnflyend (looping) under JumpTurnFly:
                        // legs out in front, falling backwards. A long fall goes to
                        // fallinguncontrolledbwd (TdAnimNodeMovementState_0).
                        let seq = if c.vel.y < -9.0 { "fallinguncontrolledbwd" } else { "jumpturnflyend" };
                        self.want(seq, Source::Play { seq, time: 0.0, rate: 1.0, looping: true }, if c.vel.y < -9.0 { 0.3 } else { 0.1 });
                    } else if c.is_coiled() {
                        // TdMove_Coil: JumpCoil (1.0, in 0.15).
                        self.want("jumpcoil", Source::Play { seq: "jumpcoil", time: 0.0, rate: 1.0, looping: false }, 0.15);
                    } else if c.vel.y < -9.0 {
                        self.want("fallinguncontrolled", Source::Play { seq: "fallinguncontrolled", time: 0.0, rate: 1.0, looping: true }, 0.3);
                    } else {
                        let seq = self.air_clip;
                        let blend = if seq == "jumpstill" { 0.15 } else { 0.1 };
                        self.want(seq, Source::Play { seq, time: 0.0, rate: 1.0, looping: false }, blend);
                    }
                }
                MoveState::WallRun { .. } => {
                    let seq = if self.last_wall_side > 0.0 { "WallrunRight" } else { "WallrunLeft" };
                    let rate = (speed / 6.3).clamp(0.7, 1.3);
                    self.want(seq, Source::Play { seq, time: 0.0, rate, looping: true }, 0.15);
                }
                // TdMove_WallClimb.StartMove: PlayCustomAnim(WallRunVertical, looping, in 0.2).
                MoveState::WallClimb { .. } => {
                    let rate = self.wallclimb_rate;
                    self.want("WallRunVertical", Source::Play { seq: "WallRunVertical", time: 0.0, rate, looping: true }, 0.2);
                }
                MoveState::WallClimbTurned { .. } => {
                    self.want("wallrunvertical180turn", Source::Play { seq: "wallrunvertical180turn", time: 0.0, rate: 1.0, looping: false }, 0.1);
                }
                MoveState::LedgeHang { .. } => {
                    if let Some(ht) = self.hang_turn {
                        // TdMove_Grab plays these with blend in/out 0.2.
                        let (seq, looping): (&'static str, bool) = match (ht.phase, ht.left) {
                            (HangTurnPhase::Start, true) => ("HangTurnLeftStart", false),
                            (HangTurnPhase::Start, false) => ("HangTurnRightStart", false),
                            (HangTurnPhase::Idle, true) => ("HangTurnLeftIdle", true),
                            (HangTurnPhase::Idle, false) => ("HangTurnRightIdle", true),
                            (HangTurnPhase::End, true) => ("HangTurnLeftEnd", false),
                            (HangTurnPhase::End, false) => ("HangTurnRightEnd", false),
                        };
                        self.want(seq, Source::Play { seq, time: 0.0, rate: 1.0, looping }, 0.2);
                    } else if let Some(sh) = c.shimmy {
                        // TdMove_Grab.StartShimmy / StartShimmyAroundCorner: one cycle of the
                        // strafe clip per hand-over-hand step, kept in step with the movement.
                        let seq = match (sh.corner.is_some(), sh.dir > 0.0) {
                            (false, true) => "HangStrafeRight",
                            (false, false) => "HangStrafeLeft",
                            (true, true) => "HangCornerOutSideRight",
                            (true, false) => "HangCornerOutSideLeft",
                        };
                        self.want(seq, Source::Driven { seq, time: sh.t }, 0.2);
                    } else {
                        self.want("Hang", Source::Play { seq: "Hang", time: 0.0, rate: 1.0, looping: true }, 0.2);
                    }
                }
                // TdMove_Landing.LandBackwards: JumpTurnLanding (blend 0.1), then lying there;
                // TdMove_LayOnGround.GetUp: JumpTurnLandingStand (blend 0.2).
                MoveState::LayOnGround { t, getting_up, back_roll } => match getting_up {
                    // TdMove_LayOnGround.GetUpBack: EvadeRoll (blend 0.2).
                    Some(g) if back_roll => self.want("evaderoll", Source::Driven { seq: "evaderoll", time: g }, 0.2),
                    Some(g) => self.want("jumpturnlandingstand", Source::Driven { seq: "jumpturnlandingstand", time: g }, 0.2),
                    None if t < Self::length(arms, "jumpturnlanding").unwrap_or(1.13) => {
                        self.want("jumpturnlanding", Source::Driven { seq: "jumpturnlanding", time: t }, 0.1)
                    }
                    None => self.want("jumpturnlandingidle", Source::Play { seq: "jumpturnlandingidle", time: 0.0, rate: 1.0, looping: true }, 0.2),
                },
                // TdMove_Barge: BargeInLeft (blend 0.2) running at it, then BargeOutLeft once it
                // gives; standing, MeleeKickObject (blend 0.1).
                MoveState::Barge { hands: true, rate, .. } => {
                    self.want("bargeinleft", Source::Play { seq: "bargeinleft", time: 0.0, rate, looping: false }, 0.2);
                }
                MoveState::Barge { t, .. } => {
                    self.want("meleekickobject", Source::Driven { seq: "meleekickobject", time: t }, 0.1);
                }
                // Its clip is a one-shot (the event); without one, standing.
                MoveState::Takedown { .. } | MoveState::StepUp { .. } | MoveState::Vertigo { .. } => self.want("loco", Source::Loco, 0.2),
                // TdMove_AirBarge: AirBargeIdle (in 0.15), AirBargeImpact (in 0), AirBargeLand
                // (in 0.1).
                MoveState::AirBarge { t, phase, .. } => {
                    let (seq, blend) = match phase {
                        AirBargePhase::Flying => ("AirBargeIdle", 0.15),
                        AirBargePhase::Impact => ("AirBargeImpact", 0.0),
                        AirBargePhase::Landing => ("AirBargeLand", 0.1),
                    };
                    self.want(seq, Source::Driven { seq, time: t }, blend);
                }
                MoveState::GrabTransfer { .. } => self.want("Hang", Source::Play { seq: "Hang", time: 0.0, rate: 1.0, looping: true }, 0.2),
                // AT_C1P's rumpslide state: crouchslideend45, looping.
                MoveState::RumpSlide { .. } => {
                    self.want("crouchslideend45", Source::Play { seq: "crouchslideend45", time: 0.0, rate: 1.0, looping: true }, 0.2)
                }
                // TdMove_Stumble.PlayStumbleAnimation: StumbleFwd (blend 0.3), or GetHitStumbleBwd
                // (blend 0.1).
                MoveState::Stumble { t, forward, .. } => {
                    let (seq, blend) = if forward { ("StumbleFwd", 0.3) } else { ("GetHitStumbleBwd", 0.1) };
                    self.want(seq, Source::Driven { seq, time: t }, blend);
                }
                // TdMove_Landing.LandOnSoftObject: FallingLandSoftLanding (blend 0.1).
                MoveState::SoftLand { t } => {
                    self.want("fallinglandsoftlanding", Source::Driven { seq: "fallinglandsoftlanding", time: t }, 0.1);
                }
                // TdMove_SpeedVault: the type's animation from the start, at rate 1 (its phases
                // are timed to it).
                MoveState::Vault(v) => {
                    let seq = v.anim();
                    self.want(seq, Source::Driven { seq, time: v.t }, 0.15);
                }
                MoveState::Traverse(tr) => {
                    let seq = match tr.kind {
                        TraverseKind::Vault => "VaultOver",
                        TraverseKind::Mantle => if tr.to.y - tr.from.y > 0.9 { "VaultOntoHigh" } else { "VaultOnto" },
                        TraverseKind::PullUp => "HangHeaveUp",
                        TraverseKind::SpringBoard => "SpringBoardLeftLeg",
                    };
                    let len = Self::length(arms, seq).unwrap_or(1.0);
                    self.want(seq, Source::Driven { seq, time: tr.t.clamp(0.0, 1.0) * len }, 0.1);
                }
                MoveState::Slide { t } => {
                    // TdMove_Slide.StartMove: CrouchSlide, blend 0.4 (TdMove_Slide.AnimBlendTime 0.5 is the
                    // move's default; the slide passes 0.4).
                    self.want("CrouchSlide", Source::Driven { seq: "CrouchSlide", time: t }, 0.4);
                }
                MoveState::Roll { t } => {
                    // TdMove_SkillRoll: fallinglandroll (1.0, in 0.2).
                    self.want("fallinglandroll", Source::Driven { seq: "fallinglandroll", time: t }, 0.2);
                }
                MoveState::Stunned { .. } => {
                    self.want("fallinglandhard", Source::Play { seq: "fallinglandhard", time: 0.0, rate: 1.0, looping: false }, 0.05);
                }
                // TdAnimNodeBalanceWalk: at the edge (danger mode) the lose-balance clip of that
                // side plays (BlendWeight 0.4 in, 0.6 back to the walk).
                MoveState::Balance { lean, danger, .. } if danger >= 0.0 => {
                    let seq = if lean < 0.0 { "walkbalancelosebalanceleft" } else { "walkbalancelosebalanceright" };
                    self.want(seq, Source::Play { seq, time: 0.0, rate: 1.0, looping: true }, 0.4);
                }
                MoveState::Balance { lean, .. } => {
                    // TdMove_Balance.AnimBlendTime = 0.4; the lean variant mixes in with the lean.
                    // Coming back from the edge, TdAnimNodeBalanceWalk blends back over 0.6.
                    let from_danger = self.layers.last().is_some_and(|l| l.key.starts_with("walkbalancelosebalance"));
                    if from_danger {
                        self.pending_blend = Some(0.6);
                    }
                    let walking = speed > 0.3;
                    let (base, left, right, rate) = if walking {
                        // TdAnimNodeSequence: ground speed over BaseSpeed (220 uu/s).
                        ("walkbalancefwd", "walkbalancefwdleanleft", "walkbalancefwdleanright", speed / 2.2)
                    } else {
                        ("walkbalancestill", "walkbalancestillleanleft", "walkbalancestillleanright", 1.0)
                    };
                    let b = if lean < 0.0 { left } else { right };
                    let key = format!("balance-{base}-{b}");
                    self.want(&key, Source::Blend2 { a: base, b, time: 0.0, rate, k: lean.abs() }, 0.4);
                }
                MoveState::ZipLine { .. } if c.zip_braced => {
                    self.want("ziplineintohitwall", Source::Play { seq: "ziplineintohitwall", time: 0.0, rate: 1.0, looping: true }, 0.3);
                }
                MoveState::ZipLine { .. } => {
                    // The loop's two leg positions: from the left it starts on the second.
                    let time = if c.zip_from_left { Self::length(arms, "ZipLine").unwrap_or(0.0) * 0.5 } else { 0.0 };
                    self.want("ZipLine", Source::Play { seq: "ZipLine", time, rate: 1.0, looping: true }, 0.2);
                }
                MoveState::Swing { angle, .. } => {
                    let (b, key) = if angle < 0.0 { ("swingposebacktop", "swing-back") } else { ("swingposefronttop", "swing-front") };
                    let k = (angle.abs() / 1.2).clamp(0.0, 1.0);
                    self.want(key, Source::Blend2 { a: "swingposebackstraight", b, time: 0.0, rate: 0.0, k }, 0.12);
                }
            }
        }
        // Mixed layers follow the lean / swing angle.
        if let Some(Layer { src: Source::Blend2 { k, rate, .. }, .. }) = self.layers.last_mut() {
            match c.state {
                MoveState::Balance { lean, .. } => {
                    *k = lean.abs().min(1.0);
                    *rate = speed / 2.2;
                }
                MoveState::Swing { angle, .. } => *k = (angle.abs() / 1.2).clamp(0.0, 1.0),
                _ => {}
            }
        }
        // Driven layers follow gameplay progress.
        if let Some(l) = self.layers.last_mut() {
            if let Source::Driven { seq, time } = &mut l.src {
                *time = match c.state {
                    MoveState::Traverse(tr) => tr.t.clamp(0.0, 1.0) * Self::length(arms, seq).unwrap_or(1.0),
                    MoveState::Slide { t } | MoveState::Roll { t } => t,
                    MoveState::Vault(v) => v.t,
                    MoveState::LayOnGround { t, getting_up, .. } => getting_up.unwrap_or(t),
                    MoveState::Stumble { t, .. } | MoveState::SoftLand { t } => t,
                    MoveState::AirBarge { t, .. } => t,
                    MoveState::IntoClimb { t, entering: true, .. } | MoveState::ClimbExit { t, .. } => t,
                    MoveState::Climb { mv: Some(m), .. } => {
                        let len = Self::length(arms, seq).unwrap_or(0.33);
                        let k = (m.t / m.dur.max(1e-3)).clamp(0.0, 1.0);
                        if m.down { (1.0 - k) * len } else { k * len }
                    }
                    MoveState::LedgeHang { .. } => c.shimmy.map_or(*time, |sh| sh.t),
                    _ => *time,
                };
            }
        }
        self.prev_state = Some(c.state);

        // ---- advance clocks and weights
        self.idle_time += dt;
        if let Some(l) = &mut self.land {
            l.t += dt;
            if l.t > 0.6 * l.amount + 0.6 && l.t > 0.5 {
                self.land = None;
            }
        }
        // ATdPawn::UpdateWalkingState and ScalePlayRateBySpeed both read the pawn's own velocity
        // (not a smoothed one): the walk gives way the moment she slows.
        self.update_walk_state(dt, c, raw_speed);
        let loco_rate = self.loco_cycles_per_sec(arms, raw_speed);
        self.loco_phase = (self.loco_phase + dt * loco_rate).fract();
        let n = self.layers.len();
        for (i, l) in self.layers.iter_mut().enumerate() {
            if let Source::Play { time, rate, .. } | Source::Blend2 { time, rate, .. } = &mut l.src {
                *time += dt * *rate;
            }
            let step = dt / l.fade_in;
            if i + 1 == n && !l.dying {
                l.weight = (l.weight + step).min(1.0);
            }
        }
        // Older layers fade out as the newest fades in.
        let top = self.layers.last().map_or(1.0, |l| l.weight);
        for l in self.layers.iter_mut().rev().skip(1) {
            l.weight = l.weight.min(1.0 - top);
        }
        self.layers.retain(|l| l.weight > 1e-3 || !l.dying);

        // ---- sound notifies crossed this update, from the layers you'd see
        self.fired.clear();
        let loco_name = self
            .loco
            .iter()
            .filter(|l| l.seq != "Stand" && l.weight > 0.3)
            .max_by(|a, b| a.weight.total_cmp(&b.weight))
            .map(|l| (l.seq, l.sync));
        let mut fired = std::mem::take(&mut self.fired);
        let mut loco_seq = self.loco_notify_seq;
        for l in &mut self.layers {
            let (seq, now, looping) = match &l.src {
                Source::Play { seq, time, looping, .. } => (Some(*seq), *time, *looping),
                Source::Driven { seq, time } => (Some(*seq), *time, false),
                Source::Blend2 { a, time, .. } => (Some(*a), *time, true),
                Source::Loco => {
                    let len = loco_name.and_then(|(n, _)| arms.anims.sequences.get(n)).map_or(1.0, |s| s.length);
                    let sync = loco_name.map_or(0.0, |(_, s)| s);
                    let name = loco_name.map(|(n, _)| n);
                    // Each cycle has its own time (an AnimNodeSequence each): when another
                    // takes over, its steps count from now, not from the last one's time
                    // (which, compared across two cycles, crossed every step between).
                    if name != loco_seq {
                        loco_seq = name;
                        l.last = (self.loco_phase + sync).fract() * len;
                    }
                    (name, (self.loco_phase + sync).fract() * len, true)
                }
            };
            // A clip that's just been played fires its notifies from its first frame, even as it
            // fades in (AnimNodeSequence.NotifyWeightThreshold 0: an attack's swoosh is at
            // 0.001 s); the walk cycles, and anything fading out, only while they're mostly
            // what you see (so two blended cycles don't both step).
            let fresh = !l.dying && !matches!(l.src, Source::Loco);
            if l.weight >= 0.5 || fresh {
                if let Some(s) = seq.and_then(|n| arms.anims.sequences.get(n)) {
                    crossed(s, l.last, now, looping, &mut fired);
                }
            }
            l.last = now;
        }
        self.fired = fired;
        self.loco_notify_seq = loco_seq;

        // ---- pose
        let mut blended = self.rest.clone();
        let mut total = 0.0;
        let align: f32 = 0.0;
        let eye_height = c.view().eye.y - c.root().y;
        let layers = std::mem::take(&mut self.layers);
        for l in &layers {
            if l.weight <= 0.0 {
                continue;
            }
            let (mut p, travel) = self.layer_pose(arms, &l.src);
            if travel {
                // A travel clip (vault, climb, pull-up) carries the body through space, which
                // gameplay already moves her through: its camera goes on her eye. Re-anchored
                // here, before blending, so it blends with the other layers in one placement
                // (placing the blend part by eye and part by feet sent the view lurching ahead
                // as a vault clip faded out).
                self.anchor_travel(arms, &mut p, eye_height);
            }
            total += l.weight;
            if total <= l.weight + 1e-6 {
                blended = p;
            } else {
                blended.blend_toward(&p, l.weight / total);
            }
        }
        self.layers = layers;
        if let Some(l) = self.land {
            self.apply_landing(arms, &l, raw_speed, c, &mut blended);
        }
        if let Some(t) = self.hang_turn {
            self.apply_hang_reach(arms, &t, &mut blended);
        }
        self.apply_against_wall(arms, c, dt, &mut blended);
        let cam_bank = self.apply_lazy_springs(arms, c, dt, &mut blended);
        pose::globals(&arms.mesh, &blended, &mut self.globals);
        self.blended = blended;

        let cam_delta = self.camera_channel(arms, dt);
        let cam = self.globals[self.cam_bone];
        let q = Quat::from_mat4(&cam);
        // Twist about the up axis (swing-twist), robust through flips/rolls.
        let cam_yaw = if q.y.abs() + q.w.abs() < 1e-6 { 0.0 } else { 2.0 * q.y.atan2(q.w) };
        BodyAnim { cam, cam_yaw, cam_delta, cam_bank, legs_yaw: self.leg_yaw, align: align.clamp(0.0, 1.0) }
    }

    fn update_hang_turn(&mut self, c: &Controller, arms: &FaithArms) {
        let MoveState::LedgeHang { normal, .. } = c.state else {
            self.hang_turn = None;
            return;
        };
        if c.shimmy.is_some() {
            self.hang_turn = None;
            return;
        }
        // View yaw relative to facing the wall; ours is positive to the left.
        let wall_yaw = (normal.x).atan2(normal.z);
        let d = (c.yaw - wall_yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let away = d.abs() > std::f32::consts::FRAC_PI_2;
        let len = |seq: &str| Self::length(arms, seq).unwrap_or(0.6);
        let now = self.clock;
        self.hang_turn = match self.hang_turn {
            None if away => Some(HangTurn { left: d > 0.0, phase: HangTurnPhase::Start, since: now, look: d }),
            None => None,
            // Back before the reach finished: just stop it (CurrentGrabTurnType 1 -> 0).
            Some(HangTurn { phase: HangTurnPhase::Start, .. }) if !away => None,
            Some(t @ HangTurn { phase: HangTurnPhase::Start, left, since, .. }) => {
                let seq = if left { "HangTurnLeftStart" } else { "HangTurnRightStart" };
                if now - since >= len(seq) { Some(HangTurn { phase: HangTurnPhase::Idle, since: now, ..t }) } else { Some(t) }
            }
            Some(t @ HangTurn { phase: HangTurnPhase::Idle, .. }) if !away => {
                Some(HangTurn { phase: HangTurnPhase::End, since: now, ..t })
            }
            Some(t @ HangTurn { phase: HangTurnPhase::Idle, .. }) => Some(t),
            // The end plays out (a 0.6 s timer in the game) before you can turn again.
            Some(t @ HangTurn { phase: HangTurnPhase::End, since, .. }) => (now - since < 0.6).then_some(t),
        };
        if let Some(t) = &mut self.hang_turn {
            if t.phase != HangTurnPhase::End {
                t.look = d;
            }
        }
    }

    /// Hanging turned: the hand the turn animation lets go of swings round with your view
    /// (the game's 1P arms follow your aim; the grab only keeps the body facing the wall), so
    /// it reaches out where you're looking while the other stays on the ledge.
    /// TdAnimNodeTurn's tick (UTdAnimNodeTurn 0x1214b80, start 0x12133d0): standing, the view's
    /// angle from the legs is checked against the safe (25) and extended (65 degree) regions.
    /// Inside the safe region nothing happens; between the two, once you've stood like that for
    /// IdleTimer (0.95 s) the legs step round 45 degrees (StandTurn45Left/Right); past the
    /// extended region they step 90 degrees at once (StandTurn90). The legs turn by exactly that
    /// much, evenly over the clip (LegTurnPerSecond), and the clip's own root rotation is dropped.
    fn update_stand_turn(&mut self, c: &Controller, arms: &FaithArms, speed: f32, dt: f32) {
        let standing = c.state == MoveState::Ground && !c.crouched && speed < 0.1;
        if !standing {
            self.leg_yaw = None;
            self.leg_turn = None;
            self.standing_still = 0.0;
            if self.oneshot.as_ref().is_some_and(|o| o.key.starts_with("StandTurn")) {
                self.oneshot = None;
                self.pending_blend = Some(0.1);
            }
            return;
        }
        let ly = self.leg_yaw.get_or_insert(c.yaw);
        if let Some((rate, end)) = self.leg_turn {
            *ly += rate * dt;
            if self.clock >= end {
                self.leg_turn = None;
            }
            return;
        }
        let d = (c.yaw - *ly + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let deg = d.abs().to_degrees();
        let big = if deg <= TURN_SAFE_DEG {
            self.standing_still = 0.0;
            return;
        } else if deg <= TURN_EXTENDED_DEG {
            self.standing_still += dt;
            if self.standing_still <= TURN_IDLE_TIMER {
                return;
            }
            false
        } else {
            true
        };
        self.standing_still = 0.0;
        // Our yaw is positive to the left.
        let seq = match (big, d > 0.0) {
            (false, true) => "StandTurn45Left",
            (false, false) => "StandTurn45Right",
            (true, true) => "StandTurn90Left",
            (true, false) => "StandTurn90Right",
        };
        let angle = if big { 90f32 } else { 45f32 }.to_radians() * d.signum();
        let len = Self::length(arms, seq).unwrap_or(0.6).max(0.05);
        self.leg_turn = Some((angle / len, self.clock + len));
        // TdAnimNodeTurn BlendWeight 0.1.
        self.oneshot(seq, arms, None, 0.1);
    }

    /// The camera channel: the camera clip's CameraJoint rotation in mesh space (rest is
    /// identity), blended in and out.
    fn camera_channel(&mut self, arms: &FaithArms, dt: f32) -> Quat {
        let Some((seq, t, bin, bout)) = self.cam_clip else { return Quat::IDENTITY };
        let Some(sq) = arms.anims.sequences.get(seq) else {
            self.cam_clip = None;
            return Quat::IDENTITY;
        };
        let len = sq.length / sq.rate.max(1e-3);
        if t >= len {
            self.cam_clip = None;
            return Quat::IDENTITY;
        }
        pose::sample(sq, &self.map, &self.rest, t * sq.rate, false, &mut self.scratch);
        let mut g = vec![];
        pose::globals(&arms.mesh, &self.scratch, &mut g);
        let rel = Quat::from_mat4(&g[self.cam_bone]).normalize();
        let w = (t / bin.max(1e-3)).min(1.0).min((len - t) / bout.max(1e-3)).clamp(0.0, 1.0);
        self.cam_clip = Some((seq, t + dt, bin, bout));
        Quat::IDENTITY.slerp(rel, w)
    }

    /// The AnimTree's TdSkelControlLazySprings (UTdSkelControlLazySpring tick 0x1223210): each
    /// keeps a lagging copy of the view's yaw or pitch (moving dt / InterpolateTime of the way
    /// each frame) and turns its bone by SpringMultiplier x how far it lags, clamped. On SpineX
    /// (the arms) WeaponYaw / WeaponPitch / WeaponRoll make the torso trail and bank against
    /// your turns and looks; on EyeJoint CameraRoll banks the camera into a turn, scaled by
    /// speed (100..400 uu/s). Returns the camera bank.
    fn apply_lazy_springs(&mut self, arms: &FaithArms, c: &Controller, dt: f32, pose: &mut Pose) -> f32 {
        // Our yaw is positive to the left; the game's to the right.
        let (yaw, pitch) = (-c.yaw, c.pitch);
        let uu = |v: f32| v * std::f32::consts::TAU / 65536.0;
        let lim = uu(1000.0);
        let s = &mut self.springs;
        let w_yaw = s[0].lag(yaw, dt);
        let w_pitch = s[1].lag(pitch, dt);
        let w_roll = s[2].lag(yaw, dt);
        let cam = s[3].lag(yaw, dt);
        // SpineX bone space: its pitch turns the torso (as view yaw), its roll pitches it
        // (as -view pitch), its yaw banks it (as -view roll): CalcCamera's axis mapping.
        let torso_turn = (-0.1 * w_yaw).clamp(-lim, lim);
        let torso_pitch_down = (0.08 * w_pitch).clamp(-lim, lim);
        let torso_bank_left = (0.05 * w_roll).clamp(-lim, lim);
        if let Some(b) = arms.bone("SpineX") {
            let q = Quat::from_rotation_y(-torso_turn) * Quat::from_rotation_x(-torso_pitch_down) * Quat::from_rotation_z(torso_bank_left);
            pose.rot[b] = (pose.rot[b] * q).normalize();
        }
        let speed = (c.horizontal_speed() / 4.0).clamp(0.25, 1.0);
        (0.15 * cam * speed).clamp(-uu(8192.0), uu(8192.0))
    }

    /// TdPawn.AgainstWallState: the AnimTree's AgainstWallState nodes blend the `againstwall`
    /// clip onto the arm that's up against the wall (in 0.35 s, back out 0.55 s), and
    /// TdSkelControlAgainstWall puts that hand on the spot the trace hit, kept inside its
    /// effector box (10-30 cm ahead, 1.0-1.7 m up, 5-35 cm out to the side).
    fn apply_against_wall(&mut self, arms: &FaithArms, c: &Controller, dt: f32, pose: &mut Pose) {
        let aw = c.against_wall;
        let ease = |w: &mut f32, on: bool| {
            let rate = if on { 1.0 / 0.35 } else { 1.0 / 0.55 };
            *w = if on { (*w + rate * dt).min(1.0) } else { (*w - rate * dt).max(0.0) };
        };
        ease(&mut self.against_wall.0, aw.left.is_some());
        ease(&mut self.against_wall.1, aw.right.is_some());
        let (wl, wr) = self.against_wall;
        if wl <= 0.0 && wr <= 0.0 {
            return;
        }
        let Some(sq) = arms.anims.sequences.get("againstwall") else { return };
        let mut wall_pose = pose.clone();
        pose::sample(sq, &self.map, &self.rest, self.clock * sq.rate, true, &mut wall_pose);
        // World point -> mesh space (the body stands on the feet facing the view on the ground).
        let to_mesh = |p: Vec3| -(Quat::from_rotation_y(-c.yaw) * (p - c.feet)) * 100.0;
        for (side, w, hit) in [("Left", wl, aw.left), ("Right", wr, aw.right)] {
            if w <= 0.0 {
                continue;
            }
            let Some(shoulder) = arms.bone(&format!("{side}Shoulder")) else { continue };
            // The arm: the shoulder and everything under it.
            for b in 0..arms.mesh.bones.len() {
                let mut p = b;
                let mut under = false;
                loop {
                    if p == shoulder {
                        under = true;
                        break;
                    }
                    if p == 0 {
                        break;
                    }
                    p = arms.mesh.bones[p].parent;
                }
                if under {
                    pose.rot[b] = pose.rot[b].slerp(wall_pose.rot[b], w);
                }
            }
            if let Some(hit) = hit {
                let mut t = to_mesh(hit);
                // Effector box (mesh space: x left, y down, z forward), mirrored for the left.
                let s = if side == "Left" { -1.0 } else { 1.0 };
                let (x0, x1) = if s > 0.0 { (-35.0, -5.0) } else { (5.0, 35.0) };
                t = Vec3::new(t.x.clamp(x0, x1), t.y.clamp(-170.0, -100.0), t.z.clamp(10.0, 30.0));
                self.arm_ik(arms, side, t, w, pose);
            }
        }
    }

    /// Two-bone IK: turn the upper arm and forearm so the hand reaches `target` (mesh space),
    /// keeping the elbow on its side, blended in by `w`.
    fn arm_ik(&self, arms: &FaithArms, side: &str, target: Vec3, w: f32, pose: &mut Pose) {
        let (Some(ua), Some(fa), Some(h)) = (arms.bone(&format!("{side}Arm")), arms.bone(&format!("{side}ForeArm")), arms.bone(&format!("{side}Hand"))) else { return };
        let mut g = vec![];
        pose::globals(&arms.mesh, pose, &mut g);
        let pos = |b: usize| g[b].w_axis.truncate();
        let (s, e, hp) = (pos(ua), pos(fa), pos(h));
        let a = (e - s).length();
        let b = (hp - e).length();
        let to_t = target - s;
        let d = to_t.length().clamp(1e-3, (a + b) * 0.999);
        let dir = to_t.normalize_or_zero();
        // Elbow: law of cosines, in the plane holding the current elbow.
        let cos_a = ((a * a + d * d - b * b) / (2.0 * a * d)).clamp(-1.0, 1.0);
        let pole = ((e - s) - dir * (e - s).dot(dir)).normalize_or_zero();
        let e2 = s + dir * (a * cos_a) + pole * (a * (1.0 - cos_a * cos_a).max(0.0).sqrt());
        let q1 = Quat::from_rotation_arc((e - s).normalize_or_zero(), (e2 - s).normalize_or_zero());
        let fore_dir = q1 * (hp - e);
        let q2 = Quat::from_rotation_arc(fore_dir.normalize_or_zero(), (s + dir * d - e2).normalize_or_zero());
        let gq = |b: usize| Quat::from_mat4(&g[b]).normalize();
        let parent_q = gq(arms.mesh.bones[ua].parent);
        let new_ua = (q1 * gq(ua)).normalize();
        let new_fa = (q2 * q1 * gq(fa)).normalize();
        let ua_local = parent_q.inverse() * new_ua;
        let fa_local = new_ua.inverse() * new_fa;
        pose.rot[ua] = pose.rot[ua].slerp(ua_local, w);
        pose.rot[fa] = pose.rot[fa].slerp(fa_local, w);
    }

    fn apply_hang_reach(&mut self, arms: &FaithArms, t: &HangTurn, pose: &mut Pose) {
        let len = |seq: &str| Self::length(arms, seq).unwrap_or(0.6).max(0.05);
        let since = self.clock - t.since;
        let w = match t.phase {
            HangTurnPhase::Start => (since / len(if t.left { "HangTurnLeftStart" } else { "HangTurnRightStart" })).min(1.0),
            HangTurnPhase::Idle => 1.0,
            HangTurnPhase::End => 1.0 - (since / 0.6).min(1.0),
        };
        // The game's 1pAim node turns the upper body (SpineX) with the view; the grab keeps the
        // other arm on the ledge. So turn the free arm's root about the chest by the full look.
        let Some(b) = arms.bone(if t.left { "SpineXLeft" } else { "SpineXRight" }) else { return };
        let parent = arms.mesh.bones[b].parent;
        let mut g = vec![];
        pose::globals(&arms.mesh, pose, &mut g);
        let pr = Quat::from_mat4(&g[parent]).normalize();
        // Mesh space and view space differ by a point reflection, so a yaw about the mesh's
        // vertical is the same yaw in the view.
        let dq = pr.inverse() * Quat::from_rotation_y(t.look * w) * pr;
        pose.pos[b] = dq * pose.pos[b];
        pose.rot[b] = (dq * pose.rot[b]).normalize();
    }

    fn apply_landing(&mut self, arms: &FaithArms, l: &LandFx, speed: f32, c: &Controller, pose: &mut Pose) {
        // LandingRun lives in the run branch of the walking tree: only while still running.
        if l.run && speed >= 2.6 && matches!(c.state, MoveState::Ground) && self.oneshot.is_none() {
            let w = landing_run_weight(l.amount, l.t);
            if let (true, Some(seq)) = (w > 0.0, arms.anims.sequences.get("FallingLandMedium")) {
                pose::sample(seq, &self.map, &self.rest, l.t, false, &mut self.scratch);
                pose.blend_toward(&self.scratch, w);
            }
        }
        let w = l.amount * land_offset_weight(l.t);
        if w <= 0.0 {
            return;
        }
        for (name, q, t) in LAND_OFFSET {
            let Some(b) = arms.bone(name) else { continue };
            // Offsets are in component space; these bones hang off the root.
            let parent = arms.mesh.bones[b].parent;
            let pr = if parent == b { Quat::IDENTITY } else { pose.rot[parent] };
            let dq = Quat::IDENTITY.slerp(Quat::from_array(q).normalize(), w);
            pose.rot[b] = (pr.inverse() * dq * pr * pose.rot[b]).normalize();
            pose.pos[b] += pr.inverse() * (Vec3::from(t) * w);
        }
    }

    /// Pick the walking state from speed (ATdPawn::UpdateWalkingState, 0x12b1660: the 2D speed
    /// against Sneak/Walk/Jog/Run/SprintVelocity, no hysteresis) and direction, and crossfade
    /// the cycles.
    fn update_walk_state(&mut self, dt: f32, c: &Controller, speed: f32) {
        let state = WALK_STATES.iter().rposition(|w| speed >= w.min_speed).unwrap_or(0);
        self.walk_state = state;
        let ws = &WALK_STATES[state];
        let fwd = c.forward();
        let v = Vec3::new(c.vel.x, 0.0, c.vel.z);
        let along = if speed > 0.1 { v.dot(fwd) / speed } else { 1.0 };
        let (seq, sync) = if along < -0.2 {
            (ws.bwd, SYNC_BWD)
        } else if along < 0.85 {
            (ws.stiff, SYNC_FWD)
        } else {
            (ws.fwd, SYNC_FWD)
        };
        if self.loco.last().is_none_or(|l| l.seq != seq) {
            self.loco.push(LocoCycle { seq, weight: 0.0, fade_in: ws.blend.max(1e-3), sync, authored: ws.authored, rate_min: ws.rate_min, rate_max: ws.rate_max });
        }
        let n = self.loco.len();
        if let Some(top) = self.loco.last_mut() {
            top.weight = (top.weight + dt / top.fade_in).min(1.0);
        }
        let top = self.loco[n - 1].weight;
        // Older cycles share what's left, keeping their proportions.
        let rest: f32 = self.loco[..n - 1].iter().map(|l| l.weight).sum();
        if rest > 1e-6 {
            let k = (1.0 - top) / rest;
            for l in &mut self.loco[..n - 1] {
                l.weight = (l.weight * k).min(l.weight);
            }
        }
        self.loco.retain(|l| l.weight > 1e-3);
    }

    /// TdPawn.IsLeftLegForward: the Walk synch group's master past half its cycle.
    fn left_leg_forward(&self) -> bool {
        let master = self.loco.iter().filter(|l| l.authored > 0.0).max_by(|a, b| a.weight.total_cmp(&b.weight));
        master.is_some_and(|l| (self.loco_phase + l.sync).fract() > 0.5)
    }

    /// The "Walk" synch group's pace (AnimNodeSynch): its master, the cycle with the most
    /// weight, plays at speed / BaseSpeed within its RateMin..RateMax, and the rest follow its
    /// relative position. Standing isn't in the group.
    fn loco_cycles_per_sec(&self, arms: &FaithArms, speed: f32) -> f32 {
        let master = self.loco.iter().filter(|l| l.authored > 0.0).max_by(|a, b| a.weight.total_cmp(&b.weight));
        let Some(l) = master else { return 0.0 };
        let Some(seq) = arms.anims.sequences.get(l.seq) else { return 0.0 };
        (speed / l.authored).clamp(l.rate_min, l.rate_max) / seq.length.max(0.1)
    }

    /// Sample one layer; also says whether it's a travel animation.
    fn layer_pose(&mut self, arms: &FaithArms, src: &Source) -> (Pose, bool) {
        let set: &AnimSet = &arms.anims;
        if let Source::Blend2 { a, b, time, k, .. } = src {
            let (Some(sa), Some(sb)) = (set.sequences.get(*a), set.sequences.get(*b)) else {
                return (self.rest.clone(), false);
            };
            let mut p = self.rest.clone();
            pose::sample(sa, &self.map, &self.rest, *time, true, &mut p);
            pose::sample(sb, &self.map, &self.rest, *time, true, &mut self.scratch);
            p.blend_toward(&self.scratch, *k);
            let travel = self.travel.get(*a).copied().unwrap_or(false) || self.travel.get(*b).copied().unwrap_or(false);
            return (p, travel);
        }
        let single = match src {
            Source::Play { seq, time, looping, .. } => Some((*seq, *time, *looping)),
            Source::Driven { seq, time } => Some((*seq, *time, false)),
            Source::Loco | Source::Blend2 { .. } => None,
        };
        match single {
            Some((seq, time, looping)) if set.sequences.contains_key(seq) => {
                let seq = &seq;
                let time = &time;
                let mut p = self.rest.clone();
                pose::sample(&set.sequences[*seq], &self.map, &self.rest, *time, looping, &mut p);
                (p, self.travel.get(*seq).copied().unwrap_or(false))
            }
            None if matches!(src, Source::Loco) => {
                let mut out = self.rest.clone();
                let mut total = 0.0;
                let cycles = std::mem::take(&mut self.loco);
                for l in &cycles {
                    let Some(seq) = set.sequences.get(l.seq) else { continue };
                    let t = if l.authored <= 0.0 { self.idle_time } else { (self.loco_phase + l.sync).fract() * seq.length };
                    pose::sample(seq, &self.map, &self.rest, t, true, &mut self.scratch);
                    total += l.weight;
                    if total <= l.weight + 1e-6 {
                        out = self.scratch.clone();
                    } else {
                        out.blend_toward(&self.scratch, l.weight / total);
                    }
                }
                self.loco = cycles;
                (out, false)
            }
            _ => (self.rest.clone(), false),
        }
    }

    /// Sound cues the animations hit this update (footsteps, cloth, breathsâ€¦).
    pub fn notifies(&self) -> &[Notify] {
        &self.fired
    }

    /// Moves a pose (by its root) so its camera bone sits straight above the root at
    /// `eye_height` (metres): where the gameplay eye is.
    fn anchor_travel(&self, arms: &FaithArms, p: &mut Pose, eye_height: f32) {
        let cam = self.camera_of(arms, p).w_axis.truncate();
        // Mesh space is the view's flipped, in centimetres (pose::to_view).
        let target = Vec3::new(0.0, -eye_height * 100.0, 0.0);
        p.pos[0] += target - cam;
    }

    /// The layers now: key, weight, dying (for tests).
    pub fn layer_summary(&self) -> String {
        self.layers.iter().map(|l| format!("{}:{:.2}{}", l.key, l.weight, if l.dying { "d" } else { "" })).collect::<Vec<_>>().join(" ")
    }

    /// Name of the dominant animation right now (for the HUD).
    pub fn current(&self) -> &str {
        self.layers.last().map_or("", |l| l.key.split('#').next().unwrap_or(""))
    }
}

/// Notifies with a time in (from, to], handling a looping wrap.
fn crossed(seq: &me_assets::Sequence, from: f32, to: f32, looping: bool, out: &mut Vec<Notify>) {
    if seq.notifies.is_empty() {
        return;
    }
    let len = seq.length.max(1e-3);
    let (a, b) = if looping { (from.rem_euclid(len), to.rem_euclid(len)) } else { (from, to) };
    let wrapped = looping && (b < a || to - from >= len);
    for n in &seq.notifies {
        let hit = if wrapped { n.time > a || n.time <= b } else { n.time > a && n.time <= b };
        if hit {
            out.push(n.notify.clone());
        }
    }
}

/// Weights for [Stand, walk, run, sprint] at a ground speed.
#[cfg(test)]
mod tests {
    //! Runs real movement through the greybox and checks which of Mirror's
    //! Edge's animations the driver picks. Needs ME_INSTALL (skips otherwise).
    use super::*;
    use glam::Vec2;
    use faith_move::greybox;
    use faith_move::{Input, Tuning};

    fn arms() -> Option<FaithArms> {
        let dir = std::env::var_os("ME_INSTALL")?;
        Some(FaithArms::load(std::path::Path::new(&dir), 64).expect("load"))
    }

    /// CameraJoint through some clips: in component space, and relative to EyeJoint.
    #[test]
    #[ignore]
    fn trace_camera_joint() {
        let Some(a) = arms() else { return };
        let map = TrackMap::new(&a.mesh, &a.anims);
        let rest = Pose::rest(&a.mesh);
        let (cam, eye) = (a.bone("CameraJoint").unwrap(), a.bone("EyeJoint").unwrap());
        let mut p = rest.clone();
        let mut g = vec![];
        pose::globals(&a.mesh, &rest, &mut g);
        let deg = |q: Quat| {
            let (x, y, z) = q.to_euler(glam::EulerRot::YXZ);
            format!("{:7.1} {:7.1} {:7.1}", x.to_degrees(), y.to_degrees(), z.to_degrees())
        };
        eprintln!("rest cam {}  eye {}", deg(Quat::from_mat4(&g[cam])), deg(Quat::from_mat4(&g[eye])));
        let rest_cam = g[cam].w_axis.truncate();
        for (i, b) in a.mesh.bones.iter().enumerate() {
            let n = b.name.to_ascii_lowercase();
            if n.contains("arm") || n.contains("hand") || n.contains("roll") || n.contains("twist") || n.contains("shoulder") || n.contains("elbow") || n.contains("wrist") {
                eprintln!("bone {i} {} parent {} track {:?}", b.name, b.parent, map.track_for_bone[i]);
            }
        }
        for name in ["RunTurn180", "StandTurn180Right", "StandTurn180Left", "JumpTurnFly", "wallrunvertical180turn"] {
            let Some(sq) = a.anims.sequences.get(name) else { eprintln!("no {name}"); continue };
            eprintln!("== {name} {:.2}s", sq.length);
            for k in 0..=12 {
                let t = sq.length * k as f32 / 12.0;
                pose::sample(sq, &map, &rest, t, false, &mut p);
                pose::globals(&a.mesh, &p, &mut g);
                let c = Quat::from_mat4(&g[cam]);
                let e = Quat::from_mat4(&g[eye]);
                eprintln!("      root {}  rootpos {:?}", deg(Quat::from_mat4(&g[0])), g[0].w_axis.truncate());
                for bn in [] as [&str; 0] {
                    let bi = a.bone(bn).unwrap();
                    let tr = map.track_for_bone[bi].and_then(|i| sq.tracks.get(i));
                    eprintln!("      {bn:18} keys {:3} local {}", tr.map_or(0, |t| t.rotations.len()), deg(p.rot[bi]));
                }
                let lh = g[a.bone("LeftHand").unwrap()].w_axis.truncate();
                let rh = g[a.bone("RightHand").unwrap()].w_axis.truncate();
                eprintln!("  {t:5.2} cam {}  off {:?}  lhand {lh:?} rhand {rh:?}", deg(c), g[cam].w_axis.truncate() - rest_cam);
                let _ = e;
            }
        }
    }

    /// A landing roll turns the view through a full forward tumble (GetCameraAnimation).
    #[test]
    fn roll_tumbles_the_camera() {
        let Some(a) = arms() else { return };
        let level = greybox::greybox();
        let w = level.world();
        let spawn = &level.checkpoints[0];
        let mut c = Controller::new(Tuning::default(), spawn.spawn, spawn.yaw);
        c.state = MoveState::Roll { t: 0.0 };
        let mut d = Driver::new(&a);
        let dt = 1.0 / 60.0;
        let (mut lo, mut hi) = (0f32, 0f32);
        let mut upside_down = false;
        for _ in 0..80 {
            c.step(dt, &Input::default(), &w);
            let body = d.update(dt, &c, &a);
            let q = Quat::from_rotation_y(body.cam_yaw).inverse() * Quat::from_mat4(&body.cam);
            let up = q * Vec3::Y;
            upside_down |= up.y < -0.5;
            let (_, pitch, _) = q.to_euler(glam::EulerRot::YXZ);
            lo = lo.min(pitch);
            hi = hi.max(pitch);
        }
        assert!(upside_down && lo < -1.2 && hi > 0.5, "pitch {lo:.2}..{hi:.2}, upside down {upside_down}");
    }

    /// Skinned arm vertices keep their distance to the bone they're bound to (no stretching):
    /// the worst stretch of each arm's skin through the wallrun clips.
    #[test]
    #[ignore]
    fn trace_arm_stretch() {
        let Some(a) = arms() else { return };
        let map = TrackMap::new(&a.mesh, &a.anims);
        let rest = Pose::rest(&a.mesh);
        let skinner = me_assets::Skinner::new(&a.mesh);
        let mut g = vec![];
        pose::globals(&a.mesh, &rest, &mut g);
        let rest_g = g.clone();
        let (mut pos, mut nrm) = (vec![], vec![]);
        let mut p = rest.clone();
        for name in ["wallrunleftstart", "WallrunLeft", "wallrunrightstart", "WallrunRight", "Stand"] {
            let Some(sq) = a.anims.sequences.get(name) else { continue };
            let mut worst: std::collections::BTreeMap<String, f32> = Default::default();
            for k in 0..10 {
                pose::sample(sq, &map, &rest, sq.length * k as f32 / 10.0, false, &mut p);
                pose::globals(&a.mesh, &p, &mut g);
                skinner.skin_full(&a.mesh, &g, Mat4::IDENTITY, &mut pos, &mut nrm, None);
                for (vi, v) in a.mesh.vertices.iter().enumerate() {
                    // The bone carrying most of the weight.
                    let k = (0..4).max_by_key(|&k| v.weights[k]).unwrap();
                    let b = v.bones[k] as usize;
                    let bn = &a.mesh.bones[b].name;
                    if !(bn.contains("Arm") || bn.contains("Hand") || bn.contains("Wrist")) {
                        continue;
                    }
                    let r0 = (Vec3::from(v.position) - rest_g[b].w_axis.truncate()).length();
                    let now = Vec3::from(pos[vi]) * -100.0;
                    let r1 = (now - g[b].w_axis.truncate()).length();
                    let e = worst.entry(bn.clone()).or_default();
                    *e = e.max((r1 - r0).abs());
                }
            }
            eprintln!("== {name}");
            for (b, w) in worst {
                if w > 1.0 {
                    eprintln!("   {b:20} {w:6.1} cm");
                }
            }
        }
    }

    #[test]
    #[ignore]
    fn list_idle_clips() {
        let Some(a) = arms() else { return };
        let mut names: Vec<_> = a.anims.sequences.iter().filter(|(n, _)| {
            let n = n.to_ascii_lowercase();
            n.contains("idle") || n.contains("taunt") || n.contains("stand") || n.contains("bored") || n.contains("look") || n.contains("fidget") || n.contains("breath")
        }).map(|(n, s)| (n.clone(), s.length)).collect();
        names.sort_by(|x, y| x.0.cmp(&y.0));
        for (n, l) in names {
            eprintln!("{n} {l:.2}");
        }
    }

    /// The CameraRoll lazy spring: turning right banks the camera right side down, more when
    /// running than standing, and it settles once you stop turning.
    #[test]
    fn camera_banks_into_turns() {
        let Some(a) = arms() else { return };
        let level = greybox::greybox();
        let w = level.world();
        let spawn = &level.checkpoints[0];
        let bank = |speed_up: bool| {
            let mut c = Controller::new(Tuning::default(), spawn.spawn, spawn.yaw);
            c.state = MoveState::Ground;
            let mut d = Driver::new(&a);
            let dt = 1.0 / 60.0;
            let mut peak = 0.0f32;
            let mut last = 0.0;
            for f in 0..150 {
                let mut i = if speed_up { fwd() } else { Input::default() };
                if (60..72).contains(&f) {
                    i.look.x = -0.05; // turning right
                }
                c.step(dt, &i, &w);
                let b = d.update(dt, &c, &a).cam_bank;
                peak = peak.max(b);
                last = b;
            }
            (peak, last)
        };
        let (still, settled) = bank(false);
        let (running, _) = bank(true);
        assert!(still > 0.0 && running > still * 1.5, "standing {still}, running {running}");
        assert!(settled.abs() < still * 0.1, "settles: {settled}");
    }

    /// Run from a checkpoint with scripted input; returns (state, anim) per frame.
    fn run(cp: usize, secs: f32, mut input: impl FnMut(&Controller) -> Input) -> Vec<(String, String)> {
        let a = arms().unwrap();
        let level = greybox::greybox();
        let w = level.world();
        let spawn = &level.checkpoints[cp];
        let mut c = Controller::new(Tuning::default(), spawn.spawn, spawn.yaw);
        c.state = MoveState::Ground;
        let mut d = Driver::new(&a);
        let dt = 1.0 / 60.0;
        let mut out = vec![];
        let mut t = 0.0;
        while t < secs {
            let i = input(&c);
            c.step(dt, &i, &w);
            let body = d.update(dt, &c, &a);
            assert!(body.cam.is_finite() && body.cam_yaw.is_finite() && body.align.is_finite());
            out.push((c.state.name().to_string(), d.current().to_string()));
            t += dt;
        }
        out
    }

    fn fwd() -> Input {
        Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() }
    }

    #[test]
    fn climb_grab_then_hang_idle() {
        if arms().is_none() {
            return;
        }
        let mut jumped = false;
        // Long enough for HangHardStart to play out (TdMove_IntoGrab plays it whole).
        let log = run(2, 6.0, |c| {
            if matches!(c.state, MoveState::WallClimb { .. } | MoveState::LedgeHang { .. }) {
                return Input::default();
            }
            let mut i = fwd();
            i.jump_pressed = !jumped && c.feet.z < greybox::z::CLIMB_WALL + 1.2;
            jumped |= i.jump_pressed;
            i
        });
        let anims_in = |state: &str| -> Vec<&str> {
            let mut v: Vec<&str> = log.iter().filter(|(s, _)| s == state).map(|(_, a)| a.as_str()).collect();
            v.dedup();
            v
        };
        println!("climb: {:?}\nhang: {:?}", anims_in("Wallclimb"), anims_in("Ledge hang"));
        assert!(anims_in("Wallclimb").iter().any(|a| a.starts_with("wallrunvertical") || *a == "WallRunVertical"));
        let hang = anims_in("Ledge hang");
        assert_eq!(hang.first(), Some(&"HangHardStart"));
        assert_eq!(hang.last(), Some(&"Hang"), "settles into the hang idle");
    }

    #[test]
    fn run_and_sprint_use_locomotion() {
        if arms().is_none() {
            return;
        }
        let log = run(0, 1.5, |_| fwd());
        assert!(log.iter().skip(30).all(|(s, a)| s != "Ground" || a == "loco"), "{log:?}");
    }

    /// Dodge with W held and keep running: once you land, the arms go back to the run cycle
    /// instead of holding the dodge pose (the clip is twice as long as the dodge's airtime).
    #[test]
    fn dodge_hands_back_to_running_on_landing() {
        if arms().is_none() {
            return;
        }
        let mut t = 0.0;
        let mut landed = None;
        let log = run(0, 2.0, |c| {
            t += 1.0 / 60.0;
            let mut i = Input { move_axis: Vec2::new(-1.0, 1.0), ..Default::default() };
            i.jump_pressed = (0.8..0.82).contains(&t);
            if t > 0.9 && landed.is_none() && c.state == MoveState::Ground {
                landed = Some(t);
            }
            i
        });
        let start = log.iter().position(|(_, a)| a.starts_with("dodgejump")).expect("dodge played");
        let land = start + log[start..].iter().position(|(s, _)| s == "Ground").expect("landed");
        // A few frames to blend out, then the run cycle.
        let after: Vec<_> = log[land + 15..].iter().map(|(_, a)| a.as_str()).collect();
        assert!(after.iter().all(|a| *a == "loco"), "after landing: {:?}", &after[..after.len().min(20)]);
    }

    /// Prints the camera height above the ledge through a climb and pull-up.
    #[test]
    #[ignore]
    fn trace_pull_up_camera() {
        let Some(a) = arms() else { return };
        let level = greybox::greybox();
        let w = level.world();
        let spawn = &level.checkpoints[2];
        let mut c = Controller::new(Tuning::default(), spawn.spawn, spawn.yaw);
        c.state = MoveState::Ground;
        let mut d = Driver::new(&a);
        let dt = 1.0 / 60.0;
        let mut jumped = false;
        for f in 0..300 {
            let mut i = fwd();
            i.jump_pressed = !jumped && c.feet.z < greybox::z::CLIMB_WALL + 1.2;
            jumped |= i.jump_pressed;
            c.step(dt, &i, &w);
            let body = d.update(dt, &c, &a);
            let cam_local = me_assets::pose::to_view(body.cam.w_axis.truncate());
            let rot = Quat::from_rotation_y(c.yaw) * Quat::from_rotation_y(body.cam_yaw).inverse();
            let feet = c.root();
            let eye = c.view().eye;
            let aligned = eye - rot * cam_local;
            let origin = feet.lerp(aligned, body.align);
            let cam = origin + rot * cam_local;
            if matches!(c.state, MoveState::LedgeHang { .. } | MoveState::Traverse(_)) || f % 10 == 0 {
                eprintln!("{f:3} {:12} {:14} align {:.2} feet.y {:.2} eye.y {:.2} cam.y {:.2} (ledge 2.3)",
                    c.state.name(), d.current(), body.align, c.feet.y, eye.y, cam.y);
            }
        }
    }

    /// Prints where the right hand sits relative to the camera through a run,
    /// jump, landing and stop (for chasing floating hands).
    #[test]
    #[ignore]
    fn trace_landing_hands() {
        let Some(a) = arms() else { return };
        let level = greybox::greybox();
        let w = level.world();
        let spawn = &level.checkpoints[0];
        let mut c = Controller::new(Tuning::default(), spawn.spawn, spawn.yaw);
        c.state = MoveState::Ground;
        let mut d = Driver::new(&a);
        let hand = a.bone("RightHand").unwrap();
        let dt = 1.0 / 60.0;
        let mut jumped = false;
        let mut landed_at = None;
        for f in 0..260 {
            let t = f as f32 * dt;
            let mut i = Input::default();
            if landed_at.is_none() {
                i.move_axis = Vec2::new(0.0, 1.0);
            }
            if t > 0.8 && !jumped {
                i.jump_pressed = true;
                jumped = true;
            }
            c.step(dt, &i, &w);
            if jumped && landed_at.is_none() && c.events.iter().any(|e| matches!(e, MoveEvent::Land { .. })) {
                landed_at = Some(f);
            }
            let body = d.update(dt, &c, &a);
            let rel = body.cam.inverse() * d.globals[hand];
            let p = me_assets::pose::to_view(rel.w_axis.truncate());
            if f % 3 == 0 && f > 40 {
                eprintln!("{f:3} {:8} {:22} align {:.2} hand-in-cam ({:6.1} {:6.1} {:6.1}) cm  speed {:.1}",
                    c.state.name(), d.current(), body.align, p.x * 100.0, p.y * 100.0, p.z * 100.0, c.horizontal_speed());
            }
        }
    }

    /// Drop from a height, land (JumpLand), stand still: hand and camera.
    #[test]
    #[ignore]
    fn trace_drop_landing() {
        let Some(a) = arms() else { return };
        let mut w = faith_move::BoxWorld::default();
        w.add(faith_move::Aabb::new(Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0)));
        let mut c = Controller::new(Tuning::default(), Vec3::new(0.0, 3.0, 0.0), 0.0);
        let mut d = Driver::new(&a);
        let hand = a.bone("RightHand").unwrap();
        let dt = 1.0 / 60.0;
        for f in 0..150 {
            let i = Input { move_axis: if f < 20 { Vec2::new(0.0, 1.0) } else { Vec2::ZERO }, ..Default::default() };
            c.step(dt, &i, &w);
            let body = d.update(dt, &c, &a);
            let rel = body.cam.inverse() * d.globals[hand];
            let p = me_assets::pose::to_view(rel.w_axis.truncate());
            let cam_local = me_assets::pose::to_view(body.cam.w_axis.truncate());
            // Rendered eye height above the feet (me_viewmodel's placement).
            let eye = c.view().eye.y - c.root().y;
            let cam_h = (1.0 - body.align) * cam_local.y + body.align * eye;
            eprintln!("{f:3} {:8} {:22} align {:.2} cam {:5.2} m (bone {:5.2}) hand ({:6.1} {:6.1} {:6.1})",
                c.state.name(), d.current(), body.align, cam_h, cam_local.y, p.x * 100.0, p.y * 100.0, p.z * 100.0);
        }
    }

    /// Prints the hang/landing clips: length, rate and the camera bone's travel.
    #[test]
    #[ignore]
    fn dump_clip_travel() {
        let Some(a) = arms() else { return };
        let d = Driver::new(&a);
        let mut names: Vec<_> = a.anims.sequences.keys().cloned().collect();
        names.sort();
        for n in &names {
            let l = n.to_ascii_lowercase();
            if !(l.contains("kick") || l.contains("barge")) {
                continue;
            }
            let seq = &a.anims.sequences[n];
            let mut line = format!("{n:32} len {:.2} rate {:.2} cam:", seq.length, seq.rate);
            for k in 0..=8 {
                let mut p = d.rest.clone();
                pose::sample(seq, &d.map, &d.rest, seq.length * k as f32 / 8.0, false, &mut p);
                let o = d.camera_of(&a, &p).w_axis.truncate() - d.rest_cam.w_axis.truncate();
                line += &format!(" ({:.0},{:.0},{:.0})", o.x, o.y, o.z);
            }
            eprintln!("{line}");
        }
    }

    /// TdMove_Landing.LandNormal: landing a jump on the run keeps the run cycle (the game only
    /// mixes a little FallingLandMedium in and dips the spine and eye), instead of swapping in
    /// a whole landing clip with its clenched hands.
    #[test]
    fn running_landing_keeps_the_run_cycle() {
        if arms().is_none() {
            return;
        }
        let mut jumped = false;
        let mut was_air = false;
        let mut landed = false;
        let log = run(0, 2.0, |c| {
            let mut i = fwd();
            i.jump_pressed = !jumped && c.horizontal_speed() > 4.0;
            jumped |= i.jump_pressed;
            landed |= was_air && c.state == MoveState::Ground;
            was_air = c.state == MoveState::Air;
            i
        });
        assert!(landed, "{:?}", log.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>());
        let anims: Vec<_> = log.iter().map(|(_, a)| a.as_str()).collect();
        assert!(!anims.iter().any(|a| a.to_ascii_lowercase().contains("land")), "{anims:?}");
    }

    /// Hanging and looking 100Â° to the left (TdMove_Grab's hang turn), the hand that let go
    /// is out in front of you where you're looking, not still at the wall behind the view.
    #[test]
    fn hang_turn_hand_reaches_where_you_look() {
        let Some(a) = arms() else { return };
        let mut w = faith_move::BoxWorld::default();
        w.add(faith_move::Aabb::new(Vec3::new(-5.0, -1.0, -5.0), Vec3::new(5.0, 0.0, 5.0)));
        w.add(faith_move::Aabb::new(Vec3::new(-2.0, 0.0, -5.0), Vec3::new(2.0, 3.0, -3.0)));
        let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
        c.feet = Vec3::new(0.0, 3.0 - c.tuning.hang_hands_above_feet, -3.0 + c.tuning.half_width + 0.001);
        c.state = MoveState::LedgeHang { normal: Vec3::Z, ledge_y: 3.0, turned: false };
        let mut d = Driver::new(&a);
        for f in 0..180 {
            let look = if f < 30 { 100f32.to_radians() / 30.0 } else { 0.0 };
            c.step(1.0 / 60.0, &Input { look: Vec2::new(look, 0.0), ..Default::default() }, &w);
            d.update(1.0 / 60.0, &c, &a);
        }
        assert_eq!(d.current(), "HangTurnLeftIdle");
        // Body faces the wall (yaw 0 here), so mesh-to-view is just to_view.
        let cam = me_assets::pose::to_view(d.globals[d.cam_bone].w_axis.truncate());
        let hand = me_assets::pose::to_view(d.globals[a.bone("LeftHand").unwrap()].w_axis.truncate()) - cam;
        let view = Vec3::new(-c.yaw.sin(), 0.0, -c.yaw.cos());
        let flat = Vec3::new(hand.x, 0.0, hand.z).normalize();
        let off = flat.dot(view).clamp(-1.0, 1.0).acos().to_degrees();
        assert!(off < 45.0, "free hand {off:.0}Â° off the view ({hand:?})");
    }

    /// Jump at a run, Q in the air: JumpTurnFly, then JumpTurnLanding onto your back, lying
    /// there, then JumpTurnLandingStand when you jump.
    #[test]
    fn air_turn_plays_the_on_your_back_animations() {
        if arms().is_none() {
            return;
        }
        let (mut jumped, mut turned, mut t) = (false, false, 0.0f32);
        let mut landed_at = None;
        let log = run(0, 5.0, |c| {
            t += 1.0 / 60.0;
            let mut i = fwd();
            i.jump_pressed = !jumped && c.horizontal_speed() > 4.0;
            jumped |= i.jump_pressed;
            if jumped && c.state == MoveState::Air && c.vel.y < 1.0 && !turned {
                turned = true;
                i.turn_pressed = true;
            }
            if matches!(c.state, MoveState::LayOnGround { .. }) {
                let at = *landed_at.get_or_insert(t);
                i = Input { jump_pressed: (t - at - 2.0).abs() < 0.01, ..Default::default() };
            }
            i
        });
        let anims: Vec<&str> = log.iter().map(|(_, a)| a.as_str()).collect();
        let order = ["JumpTurnFly", "jumpturnlanding", "jumpturnlandingidle", "jumpturnlandingstand"];
        let mut at = 0;
        for want in order {
            at += anims[at..].iter().position(|a| a.starts_with(want)).unwrap_or_else(|| panic!("no {want} after {at}: {anims:?}"));
        }
    }

    /// Turn straight after the jump: JumpTurnFly ends before you land, and the 180TurnInAir pose
    /// (jumpturnflyend, legs out in front) holds until you come down, never the plain fall.
    #[test]
    fn air_turn_holds_its_pose_until_landing() {
        if arms().is_none() {
            return;
        }
        let (mut jumped, mut turned) = (false, false);
        let log = run(0, 3.0, |c| {
            let mut i = fwd();
            i.jump_pressed = !jumped && c.horizontal_speed() > 4.0;
            jumped |= i.jump_pressed;
            if jumped && c.state == MoveState::Air && !turned {
                turned = true;
                i.turn_pressed = true;
            }
            if turned {
                i.move_axis = Vec2::ZERO;
            }
            i
        });
        let anims: Vec<&str> = log.iter().map(|(_, a)| a.as_str()).collect();
        let fly = anims.iter().position(|a| a.starts_with("JumpTurnFly")).expect("JumpTurnFly");
        let land = anims.iter().position(|a| a.starts_with("jumpturnlanding")).expect("landed on your back");
        let between = &anims[fly..land];
        assert!(between.iter().any(|a| a.starts_with("jumpturnflyend")), "{between:?}");
        assert!(!between.iter().any(|a| a.starts_with("Jump") && !a.starts_with("JumpTurnFly")), "{between:?}");
    }

    /// TdMove_WallClimb180TurnJump: wallclimb, Q, jump. The turn animation plays on through the
    /// jump until 0.6 s after Q, then WallrunJumpLeft; never the air-180 JumpTurnFly.
    #[test]
    fn wallclimb_turn_kick_plays_the_games_animations() {
        let Some(a) = arms() else { return };
        let mut w = faith_move::BoxWorld::default();
        w.add(faith_move::Aabb::new(Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0)));
        w.add(faith_move::Aabb::new(Vec3::new(-10.0, 0.0, -10.0), Vec3::new(10.0, 20.0, -4.0)));
        let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
        c.state = MoveState::Ground;
        let mut d = Driver::new(&a);
        let (mut jumped, mut turned, mut kicked) = (false, false, false);
        let mut anims = vec![];
        for _ in 0..240 {
            let mut i = Input::default();
            match c.state {
                MoveState::WallClimb { t, .. } if t > 0.15 && !turned => {
                    turned = true;
                    i.turn_pressed = true;
                }
                MoveState::WallClimbTurned { t, .. } if t > 0.2 && !kicked => {
                    kicked = true;
                    i.jump_pressed = true;
                }
                _ if !turned => {
                    i = fwd();
                    i.jump_pressed = !jumped && c.feet.z < -2.8;
                    jumped |= i.jump_pressed;
                }
                _ => {}
            }
            c.step(1.0 / 60.0, &i, &w);
            d.update(1.0 / 60.0, &c, &a);
            anims.push(d.current().to_string());
        }
        assert!(kicked, "{anims:?}");
        assert!(!anims.iter().any(|a| a.starts_with("JumpTurnFly")), "{anims:?}");
        let turn_end = anims.iter().rposition(|a| a.starts_with("wallrunvertical180turn")).unwrap();
        let jump = anims.iter().position(|a| a.starts_with("WallrunJumpLeft")).expect("WallrunJumpLeft");
        assert!(jump == turn_end + 1, "{anims:?}");
    }
}
