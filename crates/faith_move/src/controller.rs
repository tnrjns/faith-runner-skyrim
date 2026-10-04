//! The movement state machine.
//!
//! Call [`Controller::step`] once per frame with the frame's input. It
//! substeps internally at 120 Hz so feel doesn't change with frame rate.

use std::f32::consts::PI;

use glam::{Vec2, Vec3};

use crate::locomotion::{RunInput, RunState};
use crate::tuning::Tuning;
use crate::vault::{self, Vault};
pub use crate::climb::{ClimbStart, ClimbStep, Ladder};
pub use crate::airbarge::AirBargePhase;
use crate::world::{closest_on_segment, column_top, find_ledge, move_axis, slide_move, probe_wall, tops_below, trace_box, trace_wall, Aabb, Body, Fixture, WallHit, WithDoors, World};

/// One frame of player input. Buttons are "pressed this frame" edges plus
/// "held" levels; `look` is the mouse/stick delta in radians.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    /// x = strafe right, y = forward. Length is clamped to 1.
    pub move_axis: Vec2,
    /// x = yaw delta (positive turns left), y = pitch delta (positive looks up).
    pub look: Vec2,
    pub jump_pressed: bool,
    pub jump_held: bool,
    pub crouch_pressed: bool,
    pub crouch_held: bool,
    pub turn_pressed: bool,
    /// Kick / punch.
    pub melee_pressed: bool,
    /// The takedown (TdMOVE_Disarm) on whoever's in front of her.
    pub takedown_pressed: bool,
    /// The strafe axis before the move vector is clamped (PlayerInput.aStrafe): a key is the
    /// full +-1 even with W held. 0 = take it from `move_axis`.
    pub strafe_raw: f32,
}

/// Which attack: Mirror's Edge picks it from what you're doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeleeKind {
    /// On the ground, standing or running (TdMove_Melee): punches, alternating hands.
    Punch,
    /// Jump kick (TdMove_MeleeAir).
    AirKick,
    /// Sliding kick (TdMove_MeleeSlide).
    SlideKick,
    /// Kick off a wallrun (TdMove_MeleeWallrun).
    WallRunKick,
    /// Crouched (TdMove_MeleeCrouch): MeleeCrouchStart, then MeleeCrouchHit.
    Crouch,
    /// Kick on the way down from a vault (TdMove_MeleeVault): MeleeVaultOver.
    VaultKick,
}

/// Where an attack is (TdMove_MeleeBase.MeleeState).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeleePhase {
    /// The wind-up (MS_MeleeAttackNormal).
    Start,
    /// The follow-through after a hit or a miss.
    Hit,
    Missed,
}

/// An attack in progress. It plays over whatever move you're in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Melee {
    pub kind: MeleeKind,
    /// Time in the current phase.
    pub t: f32,
    /// Left hand/foot (punches alternate). For the wallrun kick: the wall was on the left.
    pub left: bool,
    pub phase: MeleePhase,
    /// The target chosen at the start (`Target::id`).
    pub target: Option<u32>,
    /// TdMove_MeleeAir: 0 from a moving jump, 1 from a still one, 2 coming down onto them.
    pub air_type: u8,
    /// The limb's sweep is on (bHitDetection); it comes on after `detect_in` seconds.
    pub detecting: bool,
    pub detect_in: Option<f32>,
    /// TdMove_Melee's combo: punches queued, punches counted, and how long the window stays open.
    pub queued: i32,
    pub combo: i32,
    pub window: f32,
    /// The momentum a kick lands with (TdMove_MeleeAir: the velocity at the start x 1.6).
    pub momentum: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TraverseKind {
    Vault,
    Mantle,
    PullUp,
    /// Foot plant on an obstacle, then launch (TdMove_SpringBoard).
    SpringBoard,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Traverse {
    pub kind: TraverseKind,
    pub from: Vec3,
    pub to: Vec3,
    pub t: f32,
    pub dur: f32,
    pub arc: f32,
    pub exit_vel: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Ground,
    Air,
    Slide { t: f32 },
    Roll { t: f32 },
    Stunned { t: f32 },
    WallRun { normal: Vec3, t: f32 },
    WallClimb { normal: Vec3, t: f32 },
    /// Turned around at the top of a wallclimb, ready to kick off.
    WallClimbTurned { normal: Vec3, t: f32 },
    LedgeHang { normal: Vec3, ledge_y: f32, turned: bool },
    Traverse(Traverse),
    /// Vaulting or stepping up (TdMove_SpeedVault / TdMove_VaultOver).
    Vault(Vault),
    /// On your back after landing out of a 180 in the air (TdMove_Landing.LandBackwards into
    /// TdMove_LayOnGround). `getting_up` counts up once you've asked to stand.
    LayOnGround { t: f32, getting_up: Option<f32>, back_roll: bool },
    /// Barging a door (TdMove_Barge): `hands` shoulder-first at a run, else a kick.
    /// `rate`: BargeInLeft's play rate, BargeAnimTime over the time to the door (0.7..1.3).
    Barge { t: f32, door: usize, hands: bool, hit: bool, dir: Vec3, rate: f32 },
    /// Into a door from the air (TdMove_AirBarge): `boost` is the height boost still to come.
    AirBarge { t: f32, door: usize, phase: AirBargePhase, boost: f32 },
    /// Tripped by barbed wire (TdMove_Stumble), `forward` over it or back from it.
    Stumble { t: f32, forward: bool, dir: Vec3 },
    /// Landed a big drop on something soft (TdMove_Landing.LandOnSoftObject).
    SoftLand { t: f32 },
    /// Walking a balance beam from `a` to `b`. `lean` runs -1..1 (right is
    /// positive); past ±1 you fall off. Counter it with left/right.
    /// `lean` is TdMove_Balance.BalanceFactor (-1 left .. 1 right); `danger` counts the time
    /// spent at the edge (bIsInDangerMode's CounterTimer), negative when not in danger.
    Balance { a: Vec3, b: Vec3, lean: f32, danger: f32, t: f32 },
    /// Riding a zipline from `a` down to `b`; `s` is the distance along it.
    ZipLine { a: Vec3, b: Vec3, s: f32, speed: f32 },
    /// Hanging from a swing pole at `at`. `dir` is the horizontal swing
    /// direction, `angle` the pendulum angle from straight down (positive =
    /// swung forward) and `rate` its angular speed.
    Swing { a: Vec3, b: Vec3, at: Vec3, dir: Vec3, angle: f32, rate: f32 },
    /// Taking someone down (TdMOVE_Disarm, `takedown`): sliding from `from` to `to`, turning to
    /// `face`, for the clip's `len`.
    Takedown { t: f32, len: f32, target: u32, from: Vec3, to: Vec3, face: f32 },
    /// Stepping up onto something knee-high (TdMove_AutoStepUp, `stepup`): straight from
    /// `from` to `to` over `dur`, then on at `saved`.
    StepUp { t: f32, from: Vec3, to: Vec3, dur: f32, saved: Vec3 },
    /// Sliding down a slope too steep to stand on (TdMove_RumpSlide, `rumpslide`): `air` counts
    /// time off it, `face` is down the slope.
    RumpSlide { t: f32, air: f32, face: f32 },
    /// Stopped at a long drop, looking over it (TdMove_Vertigo); `edge` is the point past it.
    Vertigo { t: f32, edge: Vec3 },
    /// Carried from one swing bar to the next (TdMove_SwingJump), to `to` (feet) at `speed`.
    SwingJump { t: f32, to: Vec3, speed: f32 },
    /// Getting onto a ladder (TdMove_IntoClimb): to `to` (feet) over `dur`; over the top,
    /// `entering` is LadderEnterTop's part from `from`.
    IntoClimb { t: f32, ladder: Ladder, from: Vec3, to: Vec3, dur: f32, start: ClimbStart, entering: bool },
    /// On a ladder (TdMove_Climb): on `step`, maybe moving (`mv`), sliding down (`fast`), move
    /// input ignored for `hold`.
    Climb { ladder: Ladder, step: i32, mv: Option<ClimbStep>, left: bool, fast: bool, hold: f32 },
    /// Over the top of a ladder (TdMove_Climb.ExitAtTop), from `from` (feet).
    ClimbExit { ladder: Ladder, t: f32, from: Vec3, left: bool },
    /// Reaching up from one ledge to another above (TdMove_GrabTransfer, `grabtransfer`).
    GrabTransfer { t: f32, from: Vec3, to: Vec3, dur: f32, normal: Vec3, ledge_y: f32 },
}

impl State {
    pub fn name(&self) -> &'static str {
        match self {
            State::Ground => "Ground",
            State::Air => "Air",
            State::Slide { .. } => "Slide",
            State::Roll { .. } => "Roll",
            State::Stunned { .. } => "Hard landing",
            State::WallRun { .. } => "Wallrun",
            State::WallClimb { .. } => "Wallclimb",
            State::WallClimbTurned { .. } => "Wallclimb (turned)",
            State::LedgeHang { turned: false, .. } => "Ledge hang",
            State::LedgeHang { turned: true, .. } => "Ledge hang (turned)",
            State::Balance { .. } => "Balance",
            State::ZipLine { .. } => "Zipline",
            State::Swing { .. } => "Swing",
            State::Traverse(t) => match t.kind {
                TraverseKind::Vault => "Vault",
                TraverseKind::Mantle => "Mantle",
                TraverseKind::PullUp => "Pull up",
                TraverseKind::SpringBoard => "Springboard",
            },
            State::Vault(v) if v.onto() => "Vault onto",
            State::Vault(_) => "Vault",
            State::LayOnGround { back_roll: true, .. } => "Back roll",
            State::LayOnGround { getting_up: None, .. } => "On your back",
            State::LayOnGround { .. } => "Getting up",
            State::Barge { hands: true, .. } => "Barge",
            State::Barge { .. } => "Kick",
            State::AirBarge { .. } => "Air barge",
            State::Stumble { .. } => "Stumble",
            State::SoftLand { .. } => "Soft landing",
            State::Takedown { .. } => "Takedown",
            State::StepUp { .. } => "Step up",
            State::RumpSlide { .. } => "Rump slide",
            State::Vertigo { .. } => "Vertigo",
            State::SwingJump { .. } => "Swing jump",
            State::IntoClimb { .. } => "Onto ladder",
            State::Climb { .. } => "Ladder",
            State::ClimbExit { .. } => "Off ladder",
            State::GrabTransfer { .. } => "Grab transfer",
        }
    }
}

/// Things that happened this frame, for sounds/animation/UI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Jump,
    /// `impact`: downward speed (m/s); `fall`: metres fallen from the top of the arc.
    Land { impact: f32, fall: f32 },
    HardLand,
    Roll,
    Slide,
    WallRunStart,
    WallJump,
    WallClimbStart,
    WallKick,
    LedgeGrab,
    PullUp,
    Vault,
    Mantle,
    Turn180,
    /// Q on a wallrun: looking straight out from the wall (TdMove_WallRun's turn).
    WallRunTurn,
    /// Landed on your back out of a 180 in the air.
    LandOnBack,
    /// Starting round an outside corner of a ledge.
    ShimmyCorner,
    /// A slide ended (TdMove_Slide.StopMove).
    SlideEnd,
    /// Rolled back off your back (TdMove_LayOnGround.GetUpBack).
    BackRoll,
    /// Started barging (`hands`) or kicking a door.
    Barge { hands: bool },
    /// TdMove_AirBarge: started (AirBargeIdle), through the door (AirBargeImpact), touching
    /// down (AirBargeLand).
    AirBarge,
    AirBargeImpact,
    /// TdMove_Vertigo started.
    Vertigo,
    /// TdMove_SwingJump: off one bar at the next (SwingOff).
    SwingToSwing,
    /// TdMove_IntoClimb's start clip (PlayStartAnimation, or LadderEnterTop).
    ClimbStart { start: ClimbStart, pipe: bool },
    /// TdMove_Climb: a step up or down, over the top, letting go (PipeExitBottom), sliding down
    /// and stopping.
    ClimbStep,
    ClimbExit,
    ClimbLetGo,
    ClimbSlide,
    ClimbSlideEnd,
    AirBargeLand,
    /// A door was knocked open (fixture index).
    DoorOpened { door: usize },
    /// Tripped on barbed wire.
    Stumble { forward: bool },
    /// Landed a big drop on something soft.
    SoftLand,
    /// Sideways dodge jump; `dir` is the horizontal launch direction.
    Dodge { dir: Vec3 },
    WallRunDodge { dir: Vec3 },
    WallClimbDodge { dir: Vec3 },
    Death,
    SpringBoard,
    BalanceStart,
    BalanceFall,
    /// A takedown started on `target` (TdMOVE_Disarm): `anim` indexes
    /// `takedown::TAKEDOWN_ANIMS`. They stay at `enemy_at` facing `enemy_dir` (as they stood);
    /// their side of it (the enemy's clip) is placed at `clip_at` (her spot, feet) turned to
    /// `clip_dir` (the way she faces them).
    Takedown { target: u32, anim: u8, enemy_at: Vec3, enemy_dir: Vec3, clip_at: Vec3, clip_dir: Vec3 },
    /// The takedown's clip ended: what becomes of them is the host's call.
    TakedownDone { target: u32 },
    /// Stepped up onto something knee-high (TdMove_AutoStepUp).
    StepUp,
    /// Started sliding down a slope too steep to stand on (TdMove_RumpSlide).
    RumpSlide,
    /// Reached up from a ledge to the one above (TdMove_GrabTransfer).
    GrabTransfer,
    ZipStart,
    /// TdMove_ZipLine.PrepareForForwardImpact: something solid within 600 units ahead.
    ZipBrace,
    ZipEnd { hit_wall: bool },
    SwingStart,
    SwingJump,
    Melee { kind: MeleeKind, left: bool },
    /// The wind-up ended: the hit or the miss follows (punches, the crouch attack; the air kick
    /// when it connects).
    MeleeOutcome { kind: MeleeKind, hit: bool },
    /// An attack landed on a target (`Target::id`): Mirror's Edge's damage, and the momentum
    /// (m/s) it hits with.
    MeleeHit { target: u32, damage: f32, momentum: Vec3, kind: MeleeKind },
    /// Melee during the air 180 (TdMove_180TurnInAir.HandleMoveAction(MA_Melee)): Taunt.
    Taunt,
}

/// Camera output: where to put the first-person view this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct View {
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Radians, positive tilts the head to the right.
    pub roll: f32,
    pub fov_deg: f32,
}

/// How the current time in the air began, for the rules that depend on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Takeoff {
    /// Walked off something, or any other move ended in the air.
    Other,
    /// A plain jump (TdMove_Jump): the landing hands back its forward boost.
    Jump,
    /// A wallrun jump-off (TdMove_WallrunJump): a wallrun straight after it is a chained one.
    WallJump,
}

/// A springboard you've committed to but haven't reached yet: TdMove_SpringBoard runs you in
/// to the step before the two foot plants.
#[derive(Clone, Copy, Debug)]
struct SpringRunIn {
    /// A point on the step's front face, and its outward normal.
    face: Vec3,
    n: Vec3,
    /// Where the feet go on top of the tall part, and the launch velocity.
    plant: Vec3,
    launch: Vec3,
    speed: f32,
    t: f32,
}

/// A 180 turn. The game turns you by the clip's root rotation (UseRootRotation), so the yaw
/// follows that clip's curve (Tuning::turn_curves); without the game's animations it eases
/// round to the right over Tuning::turn180_time.
#[derive(Clone, Copy, Debug)]
struct Turn {
    from: f32,
    /// Seconds since the turn started.
    t: f32,
    kind: TurnKind,
}

/// Which clip turns you round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnKind {
    /// TdMove_180Turn while moving: RunTurn180.
    Run,
    /// TdMove_180Turn standing: StandTurn180Right.
    Stand,
    /// TdMove_180TurnInAir: JumpTurnFly.
    Air,
    /// TdMove_WallClimb180TurnJump: wallrunvertical180turn.
    WallClimb,
    /// TdMove_Swing's turn: Swing180.
    Swing,
}

/// The root yaw of the game's turn clips (radians, negative = to the right), one sample every
/// 1/60 s from the clip's start to its end.
#[derive(Clone, Debug, Default)]
pub struct TurnCurves {
    pub run: Vec<f32>,
    pub stand: Vec<f32>,
    pub air: Vec<f32>,
    pub wallclimb: Vec<f32>,
    pub swing: Vec<f32>,
}

impl TurnCurves {
    pub const RATE: f32 = 60.0;

    fn get(&self, kind: TurnKind) -> &[f32] {
        match kind {
            TurnKind::Run => &self.run,
            TurnKind::Stand => &self.stand,
            TurnKind::Air => &self.air,
            TurnKind::WallClimb => &self.wallclimb,
            TurnKind::Swing => &self.swing,
        }
    }
}

/// TdPawn.AgainstWallState (ATdPawn::CheckAgainstWall, 0x12b4f10): which hands are up against
/// a wall right in front of you, and where they touch it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AgainstWall {
    pub left: Option<Vec3>,
    pub right: Option<Vec3>,
}

/// TdMove.SetLookAtTargetAngle: each frame the view moves min(1, dt / interp) of the way to
/// the target (an ease-out that never quite arrives), for `left` more seconds or until the
/// move ends.
#[derive(Clone, Copy, Debug)]
struct LookAt {
    yaw: f32,
    interp: f32,
    left: f32,
}

/// A shimmy along a ledge (TdMove_Grab.StartShimmy / StartShimmyAroundCorner).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shimmy {
    /// +1 to the right as you face the wall, -1 to the left.
    pub dir: f32,
    /// Seconds into this step (or the corner).
    pub t: f32,
    pub from: Vec3,
    pub to: Vec3,
    /// Going round an outside corner onto the face with this normal.
    pub corner: Option<Vec3>,
    yaw_from: f32,
}

/// How far through its 60-unit step `HangStrafeLeft`/`Right` has carried the body, at
/// eighths of the clip (measured from the game's animation: the hands reach first, then
/// the body swings over).
const SHIMMY_PROFILE: [f32; 9] = [0.0, 0.0, 0.0, 0.22, 0.5, 0.75, 0.92, 1.0, 1.0];

fn shimmy_progress(u: f32) -> f32 {
    let x = u.clamp(0.0, 1.0) * 8.0;
    let i = (x.floor() as usize).min(7);
    let f = x - i as f32;
    SHIMMY_PROFILE[i] + (SHIMMY_PROFILE[i + 1] - SHIMMY_PROFILE[i]) * f
}

pub struct Controller {
    pub tuning: Tuning,
    pub feet: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub state: State,
    /// Turned round in the air this jump (TdMove_180TurnInAir): you'll land on your back.
    pub turned_in_air: bool,
    /// The body's yaw while sliding (the pawn's rotation: the view turns separately).
    pub slide_yaw: f32,
    /// Per fixture: how far its door has swung open (0 = closed), indexed like World::fixtures.
    pub doors_open: Vec<f32>,
    /// Falling toward a soft landing object fast enough to brace (TdMove_SoftLanding).
    pub soft_brace: bool,
    /// The doors' boxes by fixture index (None for other fixtures).
    pub(crate) doors: Vec<Option<Aabb>>,
    /// Shimmying along (or round the corner of) the ledge you hang from.
    pub shimmy: Option<Shimmy>,
    pub crouched: bool,
    /// 0..1, how far into the sprint build-up we are.
    pub sprint_charge: f32,
    pub spawn: Vec3,
    pub spawn_yaw: f32,
    pub events: Vec<Event>,

    pub(crate) air_time: f32,
    pub(crate) jumped_since_ground: bool,
    pub(crate) jump_buffer: f32,
    /// Seconds since the last crouch press that counted as a roll trigger (TdPawn.RollTriggerTime).
    roll_trigger_age: f32,
    takeoff: Takeoff,
    /// Horizontal speed when the last plain jump started (TdMove_Jump.PreJumpMomentum).
    pre_jump_momentum: f32,
    /// Feet position at the last jump of any kind (TdPawn.LastJumpLocation).
    last_jump_at: Vec3,
    /// Wallruns in a row, each started from the previous one's jump-off
    /// (TdMove_WallRun.ConsequtiveWallruns).
    chain_wallruns: u32,
    climbed_this_air: bool,
    last_wall_normal: Option<Vec3>,
    regrab_cooldown: f32,
    turn: Option<Turn>,
    yaw_rate: f32,
    /// Sprint energy and the controller's stop state (locomotion.rs).
    pub(crate) run: RunState,
    /// How far the view turned this frame (radians): sprinting sheds speed for it.
    frame_turn: f32,

    // View smoothing
    eye_height: f32,
    /// Highest point of the current fall (for landing: ME judges landings by fall height).
    fall_peak: f32,
    /// Legs tucked in the air (coil).
    coiled: bool,
    /// The attack playing right now, if any.
    pub melee: Option<Melee>,
    melee_left: bool,
    /// Who Faith can hit (the host's actors); none in the app.
    pub targets: Vec<crate::melee::Target>,
    /// Where the attacking limb is (`melee_bone`), from the animation; without one it's taken
    /// as just ahead of her.
    pub hit_bone: Option<Vec3>,
    pub(crate) melee_last_start: Vec3,
    pub(crate) speed_log: std::collections::VecDeque<(f32, f32)>,
    /// Knocked back off an air kick: no air control until she lands.
    pub(crate) melee_no_input: bool,
    /// Lighter gravity just after leaving a swing pole.
    pub(crate) low_grav: f32,
    /// The GravityModifier while `low_grav` lasts.
    pub(crate) low_grav_k: f32,
    /// In TdMove_DodgeJump (see `dodge_jump`).
    dodging: bool,
    /// The move's view look-at (TdMove_WallRun's turn).
    look_at: Option<LookAt>,
    /// TdMove_WallRun.bTurned90FromWall: Q pressed on this wallrun.
    wall_turned: bool,
    /// Hands up against the wall in front of you (walking or crouched).
    pub against_wall: AgainstWall,
    /// TdMove_DodgeJump.RedoMoveTime: no new dodge until this runs out.
    dodge_redo: f32,
    /// Can't re-grab a zipline or pole straight after letting go.
    pub(crate) fixture_cooldown: f32,
    /// On a zipline: braced for the wall ahead (ZLS_CloseToEnd); whether you came onto it from
    /// its left (the ZipLine loop then starts half way through); the ZiplineStart rate.
    pub zip_braced: bool,
    pub zip_from_left: bool,
    pub zip_start_rate: f32,
    /// Takedowns so far (which front snatch plays next).
    pub(crate) takedowns: u32,
    /// The wall goes on above her head where a wallrun started (TdMove_WallRun's upper-body
    /// trace): its start clip plays at WallrunStartUpperBodyAnimPlayRate (0.6), else at 1.
    pub wallrun_wall_above: bool,
    /// Her vertical speed as she caught the last ledge (TdMove_IntoGrab.IntoGrabSpeed, m/s):
    /// which grab-impact clip plays.
    pub grab_speed: f32,
    /// How far her mesh (and with it the camera) is held below or above her feet after a step
    /// up or down (ATdPawn's TargetMeshTranslationZ smoothing): it eases back to them.
    pub mesh_offset: f32,
    /// TdMove_RumpSlide.RedoMoveTime: not again for a second after one.
    pub(crate) rump_redo: f32,
    /// TdMove_Vertigo: RedoMoveTime left, and the last edge it looked over.
    pub(crate) vertigo_redo: f32,
    /// TdMove_IntoClimb.RedoMoveTime after letting go of a ladder.
    pub(crate) climb_redo: f32,
    pub(crate) last_vertigo_edge: Option<Vec3>,
    /// TdMove_Vertigo's zoom to ZoomFOV is on (StartZoom; off at UnZoom).
    pub vertigo_zoom: bool,
    springboard: Option<SpringRunIn>,
    /// Seconds since grabbing the current ledge.
    pub(crate) hang_time: f32,
}

const SUBSTEP: f32 = 1.0 / 120.0;

pub(crate) fn forward(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}
pub(crate) fn right(yaw: f32) -> Vec3 {
    Vec3::new(yaw.cos(), 0.0, -yaw.sin())
}
pub(crate) fn horiz(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}
/// Unreal units per second (cm/s) to m/s, for thresholds written inline in the game's scripts.
const fn uu_per_s(v: f32) -> f32 {
    v / 100.0
}
/// Unreal units to metres.
const fn uu(v: f32) -> f32 {
    v / 100.0
}
fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Controller {
    pub fn new(tuning: Tuning, spawn: Vec3, spawn_yaw: f32) -> Self {
        let eye = tuning.stand_height - tuning.eye_from_top;
        Self {
            tuning,
            feet: spawn,
            vel: Vec3::ZERO,
            yaw: spawn_yaw,
            pitch: 0.0,
            state: State::Air,
            crouched: false,
            sprint_charge: 0.0,
            spawn,
            spawn_yaw,
            events: Vec::new(),
            air_time: 0.0,
            jumped_since_ground: false,
            jump_buffer: 0.0,
            roll_trigger_age: f32::INFINITY,
            takeoff: Takeoff::Other,
            pre_jump_momentum: 0.0,
            last_jump_at: spawn,
            chain_wallruns: 0,
            climbed_this_air: false,
            last_wall_normal: None,
            regrab_cooldown: 0.0,
            turn: None,
            yaw_rate: 0.0,
            run: RunState::default(),
            frame_turn: 0.0,
            eye_height: eye,
            fall_peak: spawn.y,
            coiled: false,
            melee: None,
            melee_left: false,
            targets: Vec::new(),
            hit_bone: None,
            melee_last_start: Vec3::ZERO,
            speed_log: Default::default(),
            melee_no_input: false,
            low_grav: 0.0,
            low_grav_k: 1.0,
            dodging: false,
            look_at: None,
            wall_turned: false,
            against_wall: AgainstWall::default(),
            dodge_redo: 0.0,
            fixture_cooldown: 0.0,
            zip_braced: false,
            zip_from_left: false,
            zip_start_rate: 1.0,
            takedowns: 0,
            wallrun_wall_above: true,
            grab_speed: 0.0,
            mesh_offset: 0.0,
            rump_redo: 0.0,
            vertigo_redo: 0.0,
            climb_redo: 0.0,
            last_vertigo_edge: None,
            vertigo_zoom: false,
            springboard: None,
            hang_time: 0.0,
            shimmy: None,
            turned_in_air: false,
            slide_yaw: 0.0,
            doors_open: Vec::new(),
            soft_brace: false,
            doors: Vec::new(),
        }
    }

    /// Move everything by `d`: a host re-centring its coordinates near the player (floats lose
    /// precision far from the origin). Only standing on the ground, where no move holds a
    /// position of its own; returns whether it moved.
    pub fn rebase(&mut self, d: Vec3) -> bool {
        if self.state != State::Ground {
            return false;
        }
        self.feet += d;
        self.spawn += d;
        self.last_jump_at += d;
        self.fall_peak += d.y;
        true
    }

    pub fn respawn(&mut self) {
        self.feet = self.spawn;
        self.vel = Vec3::ZERO;
        self.yaw = self.spawn_yaw;
        self.pitch = 0.0;
        self.state = State::Air;
        self.crouched = false;
        self.sprint_charge = 0.0;
        self.run = RunState::default();
        self.turn = None;
        self.climbed_this_air = false;
        self.last_wall_normal = None;
        self.fall_peak = self.spawn.y;
        self.coiled = false;
        self.melee = None;
        self.low_grav = 0.0;
        self.dodging = false;
        self.look_at = None;
        self.wall_turned = false;
        self.dodge_redo = 0.0;
        self.fixture_cooldown = 0.0;
        self.springboard = None;
        self.takeoff = Takeoff::Other;
        self.chain_wallruns = 0;
        self.last_jump_at = self.spawn;
    }

    pub fn height(&self) -> f32 {
        if self.coiled {
            self.tuning.stand_height - self.tuning.coil_lift
        } else if self.crouched {
            self.tuning.crouch_height
        } else {
            self.tuning.stand_height
        }
    }

    /// Feet height as if the legs weren't tucked.
    fn body_y(&self) -> f32 {
        self.feet.y - if self.coiled { self.tuning.coil_lift } else { 0.0 }
    }

    pub fn body(&self) -> Body {
        Body { half_width: self.tuning.half_width, height: self.height() }
    }

    pub fn horizontal_speed(&self) -> f32 {
        horiz(self.vel).length()
    }

    pub fn forward(&self) -> Vec3 {
        forward(self.yaw)
    }

    /// Advance by `dt` seconds.
    pub fn step(&mut self, dt: f32, input: &Input, world: &dyn World) {
        self.events.clear();
        let mut input = *input;
        if input.strafe_raw == 0.0 {
            input.strafe_raw = input.move_axis.x.clamp(-1.0, 1.0);
        }
        input.move_axis = input.move_axis.clamp_length_max(1.0);

        // Doors are solid until knocked open.
        let fixtures = world.fixtures();
        self.doors_open.resize(fixtures.len(), 0.0);
        for (i, f) in fixtures.iter().enumerate() {
            if matches!(f, Fixture::Door { .. }) && self.doors_open[i] > 0.0 {
                self.doors_open[i] = (self.doors_open[i] + dt / 0.4).min(1.0);
            }
        }
        let closed: Vec<Aabb> = fixtures
            .iter()
            .enumerate()
            .filter_map(|(i, f)| match f {
                Fixture::Door { b, .. } if self.doors_open[i] == 0.0 => Some(*b),
                _ => None,
            })
            .collect();
        self.doors = fixtures.iter().map(|f| if let Fixture::Door { b, .. } = f { Some(*b) } else { None }).collect();
        let doors = WithDoors { world, closed };
        let world: &dyn World = &doors;

        // Look.
        let yaw_before = self.yaw;
        // TdMove_Landing.LandHard: SetIgnoreLookInput, and ResetCameraLook(0.3) levels the view.
        let stunned = matches!(self.state, State::Stunned { .. });
        if stunned {
            input.look = Vec2::ZERO;
            self.pitch -= self.pitch * (dt / 0.3).min(1.0);
        }
        // TdMOVE_Disarm: DisableLookTime / DisableMovementTime -1, for the whole move.
        self.vertigo_input(&mut input);
        // TdMove_AirBarge, TdMove_IntoClimb: DisableLookTime -1; ExitAtTop ignores it for 0.9 s.
        if matches!(self.state, State::AirBarge { .. } | State::IntoClimb { .. }) || matches!(self.state, State::ClimbExit { t, .. } if t < 0.9) {
            input.look = Vec2::ZERO;
        }
        if matches!(self.state, State::Takedown { .. }) {
            input.look = Vec2::ZERO;
            input.move_axis = Vec2::ZERO;
            input.jump_pressed = false;
            input.crouch_pressed = false;
            input.turn_pressed = false;
            input.melee_pressed = false;
        }
        if self.turn.is_none() {
            self.yaw += input.look.x;
        }
        self.frame_turn = if self.turn.is_none() { input.look.x } else { 0.0 };
        self.pitch = (self.pitch + input.look.y).clamp(-1.5, 1.5);

        if input.jump_pressed {
            self.jump_buffer = self.tuning.jump_buffer;
        }
        // TdPawn.Tick: a crouch press arms the roll, but only 0.6 s after the last one did.
        if input.crouch_pressed && self.roll_trigger_age >= self.tuning.roll_retrigger {
            self.roll_trigger_age = 0.0;
        }
        if input.turn_pressed && self.turn.is_none() && self.can_turn() {
            self.start_turn();
        }
        if input.melee_pressed {
            if self.melee.is_some() {
                self.melee_pressed_again();
            } else {
                self.start_melee();
            }
        }
        if self.melee_no_input {
            input.move_axis = Vec2::ZERO;
        }
        if input.takedown_pressed {
            self.try_takedown(world);
        }


        // Substep physics. Edge-triggered inputs only count on the first substep.
        let mut remaining = dt.min(0.1);
        let mut first = true;
        while remaining > 1e-6 {
            let h = remaining.min(SUBSTEP);
            let mut sub = input;
            if !first {
                sub.jump_pressed = false;
                sub.crouch_pressed = false;
                sub.turn_pressed = false;
                sub.melee_pressed = false;
                sub.takedown_pressed = false;
                sub.look = Vec2::ZERO;
            }
            self.substep(h, &sub, world);
            remaining -= h;
            first = false;
        }
        self.melee_tick(dt.min(0.1));
        self.smooth_mesh(dt.min(0.1));
        if self.state == State::Ground {
            self.melee_no_input = false;
        }

        if self.state != State::Air {
            self.turned_in_air = false;
        }


        // Turn animation: the clip's root rotation.
        if let Some(mut t) = self.turn {
            t.t += dt;
            let curve = self.tuning.turn_curves.as_ref().map(|c| c.get(t.kind)).filter(|c| c.len() > 1);
            let (angle, done) = match curve {
                Some(c) => {
                    let f = t.t * TurnCurves::RATE;
                    let i = (f as usize).min(c.len() - 1);
                    let j = (i + 1).min(c.len() - 1);
                    let a = f - i as f32;
                    (c[i] + (c[j] - c[i]) * a.min(1.0), i + 1 >= c.len())
                }
                None => {
                    let u = (t.t / self.tuning.turn180_time).min(1.0);
                    (-PI * smoothstep(u), u >= 1.0)
                }
            };
            self.yaw = t.from + angle;
            self.turn = if done { None } else { Some(t) };
        }
        // The move's look-at; ending the move ends it (TdMove.StopMove: AbortLookAtTarget).
        if let Some(mut la) = self.look_at {
            la.left -= dt;
            if la.left < 0.0 || !matches!(self.state, State::WallRun { .. }) {
                self.look_at = None;
            } else {
                let d = (la.yaw - self.yaw + PI).rem_euclid(2.0 * PI) - PI;
                self.yaw += d * (dt / la.interp).min(1.0);
                self.look_at = Some(la);
            }
        }
        self.yaw_rate = if dt > 0.0 { (self.yaw - yaw_before).abs() / dt } else { 0.0 };
        self.against_wall = if self.state == State::Ground { self.check_against_wall(world) } else { AgainstWall::default() };

        self.update_view(dt);

        if self.tuning.kill_y.is_some_and(|y| self.feet.y < y) {
            self.events.push(Event::Death);
            if self.tuning.respawn_on_death {
                self.respawn();
            }
        }
    }

    /// ATdPawn::CheckAgainstWall (0x12b4f10), run from TdPawn.Tick while the move allows it
    /// (bEnableAgainstWall: walking, crouching, ledge walking). Two thin traces (2x2x5 uu)
    /// straight ahead from 68 uu above the pawn's centre (26 crouched), 14 uu to either side,
    /// reaching max(40, 0.28 x forward speed) uu; a hand goes up on a wall facing you
    /// (normal . forward < -0.5).
    fn check_against_wall(&self, world: &dyn World) -> AgainstWall {
        let fwd = forward(self.yaw);
        let right = right(self.yaw);
        let centre = self.feet + Vec3::Y * (self.height() * 0.5);
        let up = if self.crouched { uu(26.0) } else { uu(68.0) };
        let reach = uu(40.0).max(self.vel.dot(fwd) * 0.7 * 0.4);
        let half = Vec3::new(uu(2.0), uu(5.0), uu(2.0));
        let hand = |side: f32| {
            let start = centre + Vec3::Y * up + right * (uu(14.0) * side);
            trace_box(world, start, fwd, reach, half).filter(|(_, n)| n.dot(fwd) < -0.5).map(|(at, _)| at)
        };
        AgainstWall { left: hand(-1.0), right: hand(1.0) }
    }

    /// Where Q (180 turn) works. TdMove_180Turn: only walking on the ground (not crouched or
    /// sliding); TdMove_180TurnInAir: in the air while moving the way you face. Wallclimbs,
    /// ledges and swing poles have their own turn moves. Anywhere else the view would swing
    /// round while the body stays put (a slide, a wallrun), looking out through the body.
    fn can_turn(&self) -> bool {
        match self.state {
            State::Ground => !self.crouched,
            State::Air => horiz(self.vel).normalize_or_zero().dot(forward(self.yaw)) >= 0.2,
            State::WallClimb { .. } | State::LedgeHang { .. } | State::Swing { .. } | State::WallRun { .. } => true,
            _ => false,
        }
    }

    fn start_turn(&mut self) {
        // TdMove_WallRun.HandleMoveAction(MA_Turn): on a wallrun the turn button doesn't turn
        // you round; it swings the view to look straight out from the wall in 0.15 s, and you
        // keep running. Jumping then is the full push-off to the opposite wall.
        // SetLookAtTargetAngle(rotator(Floor), 0.15, 1.5).
        if let State::WallRun { normal, .. } = self.state {
            let want = (-normal.x).atan2(-normal.z);
            self.look_at = Some(LookAt { yaw: want, interp: 0.15, left: 1.5 });
            self.wall_turned = true;
            self.events.push(Event::WallRunTurn);
            return;
        }
        // Every 180 turns right, by its clip. TdMove_180Turn: RunTurn180 unless you're standing
        // (CurrentWalkingState idle), then StandTurn180Right.
        let kind = match self.state {
            State::Air => TurnKind::Air,
            State::WallClimb { .. } => TurnKind::WallClimb,
            State::Swing { .. } => TurnKind::Swing,
            _ if self.horizontal_speed() > uu_per_s(10.0) => TurnKind::Run,
            _ => TurnKind::Stand,
        };
        self.turn = Some(Turn { from: self.yaw, t: 0.0, kind });
        self.events.push(Event::Turn180);
        if self.state == State::Air {
            self.turned_in_air = true;
        }
        match self.state {
            State::WallClimb { normal, .. } => {
                self.state = State::WallClimbTurned { normal, t: 0.0 };
            }
            // TdMove_Grab.HandleMoveAction(MA_Turn): swings the view round; the hang follows it.
            State::LedgeHang { .. } => {}
            State::Swing { a, b, at, dir, angle, rate } => {
                self.state = State::Swing { a, b, at, dir: -dir, angle: -angle, rate: -rate };
            }
            _ => {}
        }
    }

    fn start_melee(&mut self) {
        if self.state == State::Ground && !self.crouched && self.try_barge() {
            return;
        }
        if self.state == State::Air && self.turned_in_air {
            self.events.push(Event::Taunt);
            return;
        }
        // PlayerWalking.HandleMoveAction: in the air, the air barge before the air kick.
        if self.state == State::Air && self.try_air_barge() {
            return;
        }
        let kind = match self.state {
            State::Ground if self.crouched => MeleeKind::Crouch,
            State::Ground => MeleeKind::Punch,
            // TdMove_MeleeAir.CanDoMove: not in the first 0.1 s of a jump.
            State::Air if self.jumped_since_ground && self.air_time < 0.1 => return,
            State::Air => MeleeKind::AirKick,
            State::Slide { .. } => MeleeKind::SlideKick,
            State::WallRun { .. } => MeleeKind::WallRunKick,
            // TdMove_SpeedVault.HandleMoveAction: the vault ends in the kick (vault_step).
            State::Vault(mut v) => {
                v.kick = true;
                self.state = State::Vault(v);
                return;
            }
            _ => return,
        };
        self.melee_left = !self.melee_left;
        let left = match self.state {
            // bLeft: the wallrun was along a wall on her left.
            State::WallRun { normal, .. } => normal.dot(right(self.yaw)) > 0.0,
            _ => self.melee_left,
        };
        let mut m = Melee {
            kind,
            t: 0.0,
            left,
            phase: MeleePhase::Start,
            target: self.melee_target(kind),
            air_type: 0,
            detecting: false,
            detect_in: None,
            queued: 0,
            combo: 0,
            // TdMove_Melee.TriggerMove: OpenWindow(0.33).
            window: if kind == MeleeKind::Punch { 0.33 } else { 0.0 },
            momentum: Vec3::ZERO,
        };
        match kind {
            MeleeKind::AirKick => self.start_air_kick(&mut m),
            MeleeKind::WallRunKick => self.start_wallrun_kick(&mut m),
            // TdMove_MeleeSlide.TriggerMove: SetTimer(0.2542) turns the hit detection on.
            MeleeKind::SlideKick => m.detect_in = Some(0.2542),
            MeleeKind::Punch | MeleeKind::Crouch | MeleeKind::VaultKick => {}
        }
        self.melee = Some(m);
        self.events.push(Event::Melee { kind, left });
    }

    /// In the game sprinting isn't a separate meter, it's the speed you've built up; so when a
    /// move takes speed away, take the sprint charge down to match.
    fn sync_sprint_charge(&mut self) {
        let tu = &self.tuning;
        let frac = ((self.horizontal_speed() - tu.run_speed) / (tu.sprint_speed - tu.run_speed)).clamp(0.0, 1.0);
        self.sprint_charge = self.sprint_charge.min(frac);
        self.run.energy = self.run.energy.min((self.horizontal_speed() - tu.run_speed).max(0.0));
    }

    /// How far up the body can move, up to `max` (the scripts' upward MovementTrace before a
    /// jump-off, so a low ceiling shortens it).
    fn headroom(&self, world: &dyn World, max: f32) -> f32 {
        let mut p = self.feet;
        move_axis(world, self.body(), &mut p, 1, max);
        p.y - self.feet.y
    }

    pub(crate) fn grounded(&self, world: &dyn World) -> bool {
        let hw = self.tuning.half_width - 0.01;
        let region = Aabb::new(
            self.feet + Vec3::new(-hw, -0.05, -hw),
            self.feet + Vec3::new(hw, 0.02, hw),
        );
        !world.is_free(&region)
    }

    fn try_stand(&mut self, world: &dyn World) -> bool {
        if !self.crouched {
            return true;
        }
        let stand = Body { half_width: self.tuning.half_width, height: self.tuning.stand_height };
        if world.is_free(&stand.aabb(self.feet)) {
            self.crouched = false;
            true
        } else {
            false
        }
    }

    /// In the air, crouch tucks the legs up (head stays put); releasing drops them.
    fn update_coil(&mut self, input: &Input, world: &dyn World) {
        let stand = Body { half_width: self.tuning.half_width, height: self.tuning.stand_height };
        let lift = self.tuning.coil_lift;
        // TdMove_Coil.CanDoMove: not from a plain fall (only out of a jump of some kind), and
        // moving forward along the view at CoilMinTriggerSpeed or more.
        // It's started by a crouch press in the air (a held crouch, say from jumping out of a
        // slide, doesn't tuck the legs), and lasts while it's held.
        let can_coil = self.jumped_since_ground && self.vel.dot(forward(self.yaw)) >= self.tuning.coil_min_speed;
        if input.crouch_pressed && input.crouch_held && !self.coiled && !self.crouched && can_coil {
            self.coiled = true;
            self.feet.y += lift;
        } else if !input.crouch_held && self.coiled {
            let lowered = self.feet - Vec3::Y * lift;
            if world.is_free(&stand.aabb(lowered)) {
                self.feet = lowered;
                self.coiled = false;
            }
        } else if !input.crouch_held && self.crouched {
            // Came off a slide still crouched: stand up by growing downward.
            let diff = self.tuning.stand_height - self.tuning.crouch_height;
            let lowered = self.feet - Vec3::Y * diff;
            if world.is_free(&stand.aabb(lowered)) {
                self.feet = lowered;
                self.crouched = false;
            }
        }
    }

    /// Straighten tucked legs on touchdown (grow upward, crouch if there's no room).
    fn uncoil_on_ground(&mut self, world: &dyn World) {
        if self.coiled {
            self.coiled = false;
            let stand = Body { half_width: self.tuning.half_width, height: self.tuning.stand_height };
            if !world.is_free(&stand.aabb(self.feet)) {
                self.crouched = true;
            }
        }
    }

    fn substep(&mut self, dt: f32, input: &Input, world: &dyn World) {
        self.jump_buffer = (self.jump_buffer - dt).max(0.0);
        self.roll_trigger_age += dt;
        self.regrab_cooldown = (self.regrab_cooldown - dt).max(0.0);
        self.low_grav = (self.low_grav - dt).max(0.0);
        self.fixture_cooldown = (self.fixture_cooldown - dt).max(0.0);
        self.rump_redo = (self.rump_redo - dt).max(0.0);
        self.vertigo_redo = (self.vertigo_redo - dt).max(0.0);
        self.climb_redo = (self.climb_redo - dt).max(0.0);
        if self.dodging && self.state != State::Air {
            self.end_dodge();
        }
        if !self.dodging {
            self.dodge_redo = (self.dodge_redo - dt).max(0.0);
        }

        match self.state {
            State::Ground => self.ground(dt, input, world),
            State::Air => self.air(dt, input, world),
            State::Slide { t } => self.slide(dt, t, input, world),
            State::Roll { t } => self.roll_state(dt, t, world),
            State::Stunned { t } => self.stunned(dt, t, world),
            State::WallRun { normal, t } => self.wallrun(dt, normal, t, input, world),
            State::WallClimb { normal, t } => self.wallclimb(dt, normal, t, input, world),
            State::WallClimbTurned { normal, t } => self.wallclimb_turned(dt, normal, t, input, world),
            State::LedgeHang { normal, ledge_y, turned } => {
                self.hang(dt, normal, ledge_y, turned, input, world)
            }
            State::Traverse(tr) => self.traverse(dt, tr, world),
            State::Vault(v) => self.vault_step(dt, v, world),
            State::LayOnGround { t, getting_up, back_roll } => self.lay_on_ground(dt, t, getting_up, back_roll, input, world),
            State::Barge { t, door, hands, hit, dir, rate } => self.barge(dt, t, door, hands, hit, dir, rate, world),
            State::AirBarge { t, door, phase, boost } => self.air_barge(dt, t, door, phase, boost, world),
            State::Stumble { t, forward, dir } => self.stumble(dt, t, forward, dir, world),
            State::SoftLand { t } => self.soft_land(dt, t, world),
            State::Balance { a, b, lean, danger, t } => self.balance(dt, a, b, lean, danger, t, input, world),
            State::ZipLine { a, b, s, speed } => self.zipline(dt, a, b, s, speed, input, world),
            State::Swing { a, b, at, dir, angle, rate } => self.swing(dt, a, b, at, dir, angle, rate, input, world),
            State::Takedown { t, len, target, from, to, face } => self.takedown(dt, t, len, target, from, to, face),
            State::StepUp { t, from, to, dur, saved } => self.step_up(dt, t, from, to, dur, saved),
            State::RumpSlide { t, air, face } => self.rump_slide(dt, t, air, face, input.move_axis.x, world),
            State::Vertigo { t, edge } => self.vertigo(dt, t, edge, input),
            State::SwingJump { t, to, speed } => self.swing_jump(dt, t, to, speed, world),
            State::IntoClimb { t, ladder, from, to, dur, start, entering } => self.into_climb(dt, t, ladder, from, to, dur, start, entering),
            State::Climb { ladder, step, mv, left, fast, hold } => self.climb(dt, ladder, step, mv, left, fast, hold, input, world),
            State::ClimbExit { ladder, t, from, left } => self.climb_exit(dt, ladder, t, from, left, input, world),
            State::GrabTransfer { t, from, to, dur, normal, ledge_y } => self.grab_transfer(dt, t, from, to, dur, normal, ledge_y),
        }

        self.check_barbed_wire(world);
        self.update_soft_brace(world);

        let y = self.body_y();
        self.fall_peak = if matches!(self.state, State::Air) { self.fall_peak.max(y) } else { y };
        if !matches!(self.state, State::Air) {
            self.takeoff = Takeoff::Other;
        }
        if self.state != State::Ground {
            self.springboard = None;
        }
    }

    // ---------------------------------------------------------------- ground

    fn ground(&mut self, dt: f32, input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        if !self.grounded(world) {
            self.state = State::Air;
            self.air_time = 0.0;
            self.jumped_since_ground = false;
            return;
        }
        self.air_time = 0.0;
        self.climbed_this_air = false;
        self.last_wall_normal = None;
        if self.try_into_climb(input, world) {
            return;
        }

        // On ground too steep to stand on: sliding down it.
        if self.try_rump_slide(world) {
            return;
        }
        if !self.crouched && self.try_balance(world) {
            return;
        }

        // Slide: TdMove_Slide.CanDoMove wants 350 uu/s along the way you face.
        let forward_speed = self.vel.dot(forward(self.yaw));
        if input.crouch_pressed && forward_speed >= tu.slide_min_speed && !self.crouched {
            self.crouched = true;
            self.slide_yaw = self.yaw;
            self.state = State::Slide { t: 0.0 };
            self.events.push(Event::Slide);
            return;
        }
        if input.crouch_held {
            self.crouched = true;
        } else {
            self.try_stand(world);
        }
        // Walking into something knee-high: step up onto it (when the host turns it on).
        if tu.auto_step_up && !self.crouched && input.move_axis.y > 0.2 && self.try_auto_step_up(world) {
            return;
        }

        if self.springboard.is_some() {
            self.springboard_run_in(dt, world);
            return;
        }

        // Dodge / springboard / jump, in TdPlayerMoveManager's order (moves 33, 7, 11).
        // TdMove_DodgeJump.CanDoMove: the stick's hint is a strafe (|aStrafe| > 0.3 wins over
        // forward) at full deflection (bMoveActionMax, > 0.96). A key is a full 1.0, W or not.
        // Vaults aren't here: the game jumps, and the jump vaults you (see vault.rs).
        if self.jump_buffer > 0.0 {
            if input.strafe_raw.abs() > tu.dodge_strafe_threshold && self.dodge_redo <= 0.0 && !self.crouched && self.try_stand(world) {
                self.dodge_jump(input.strafe_raw.signum());
                self.integrate(dt, world, false);
                return;
            }
            if input.move_axis.y > 0.3 && self.try_springboard(world) {
                self.jump_buffer = 0.0;
                return;
            }
            if self.try_stand(world) {
                self.ground_jump();
                // Don't lose this substep's movement on takeoff.
                self.integrate(dt, world, false);
                return;
            }
        } else if input.jump_held && input.move_axis.y > 0.3 && self.try_springboard(world) {
            // Holding jump while running springboards by itself when you reach the obstacle.
            return;
        }

        // Running: Mirror's Edge's own ground movement (locomotion.rs, from the game's native
        // code). The controller asks for sprint or walk acceleration, CalcVelocity applies it
        // with friction or braking, capped at GroundSpeed x the move's SpeedModifier.
        let attacking = self.melee.is_some_and(|m| matches!(m.kind, MeleeKind::Punch | MeleeKind::Crouch));
        let speed_mod = if self.crouched {
            tu.crouch_speed_modifier
        } else if attacking {
            tu.melee_speed
        } else {
            1.0
        };
        let ri = RunInput {
            forward: forward(self.yaw),
            right: right(self.yaw),
            a_forward: input.move_axis.y,
            a_strafe: input.move_axis.x,
            turn: self.frame_turn,
        };
        let mut h = horiz(self.vel);
        let accel = tu.loco.controller_accel(&mut self.run, &ri, &mut h, dt);
        let h = tu.loco.calc_velocity(h, accel, dt, speed_mod, tu.loco.ground_friction);
        self.vel = Vec3::new(h.x, 0.0, h.z);
        self.sprint_charge = (self.run.energy / (tu.loco.ground_speed - tu.loco.max_base)).clamp(0.0, 1.0);

        // The walking edge check: a long drop ahead may stop her (TdMove_Vertigo).
        if self.try_vertigo(world) {
            return;
        }
        self.integrate(dt, world, true);
    }

    /// Jump off the ground (TdMove_Jump.StartMove): remember the speed you jumped with, and
    /// add JumpAddXY along the way you face if you're already moving that way.
    fn ground_jump(&mut self) {
        let fwd = forward(self.yaw);
        self.pre_jump_momentum = self.horizontal_speed();
        if fwd.dot(self.vel) > uu_per_s(10.0) {
            self.vel += fwd * self.tuning.jump_add_forward;
        }
        self.jump(self.tuning.jump_speed);
        self.takeoff = Takeoff::Jump;
    }

    /// TdMove_DodgeJump.StartMove: JumpAddXY (600) straight out to the side of where you face,
    /// plus DodgeJumpInertiaConservation (0.3) of your velocity, BaseJumpZ (300) up. Looking
    /// 90 degrees off your run and dodging back along it is how you hit top speed at once.
    /// The move then flies on its own (ControllerState PlayerGrabbing, Acceleration =
    /// Normal(Velocity): no steering, no grabs or wall moves) until you fall faster than
    /// ExitToFallingZSpeed (-190), when the falling move takes over.
    fn dodge_jump(&mut self, side: f32) {
        let tu = self.tuning.clone();
        let dir = right(self.yaw) * side;
        self.dodge(dir, tu.dodge_side, tu.dodge_up, tu.dodge_inertia);
        self.dodging = true;
        self.events.push(Event::Dodge { dir });
    }

    /// Out of TdMove_DodgeJump; RedoMoveTime (0.3 s) starts.
    fn end_dodge(&mut self) {
        self.dodging = false;
        self.dodge_redo = self.tuning.dodge_redo;
    }

    /// ME's dodge jumps: keep a fraction of the old velocity, add a sideways
    /// launch.
    fn dodge(&mut self, dir: Vec3, side: f32, up: f32, inertia: f32) {
        self.last_jump_at = self.feet;
        let keep = horiz(self.vel) * inertia;
        let h = keep + dir * side;
        self.vel = Vec3::new(h.x, up, h.z);
        self.state = State::Air;
        self.air_time = 0.0;
        self.jumped_since_ground = true;
        self.jump_buffer = 0.0;
    }

    fn jump(&mut self, speed: f32) {
        self.last_jump_at = self.feet;
        self.vel.y = speed;
        self.state = State::Air;
        self.air_time = 0.0;
        self.jumped_since_ground = true;
        self.jump_buffer = 0.0;
        self.events.push(Event::Jump);
    }

    /// TdMove_SpringBoard.CanDoMove: a low step ahead that you'd reach within a second, with
    /// something 0.8-1.48 m high just behind it. If it fits, commit: run in, then plant a foot
    /// on each and launch.
    fn try_springboard(&mut self, world: &dyn World) -> bool {
        let tu = self.tuning.clone();
        let fwd = forward(self.yaw);
        let body = self.body();
        let speed = self.horizontal_speed();
        // Centre-to-step distance you can cover in CheckDistanceTime.
        let reach = speed * tu.springboard_check_time;
        if reach <= tu.half_width {
            return false;
        }
        let step_hi = tu.springboard_step_height + tu.springboard_slack;
        // probe_wall only looks at the far end of its reach, so sweep it out to find the
        // nearest face.
        let mut hit = None;
        let mut r = 0.05;
        while r <= reach - tu.half_width {
            hit = probe_wall(world, body, self.feet, fwd, r, 0.05, step_hi);
            if hit.is_some() {
                break;
            }
            r += 0.1;
        }
        let Some((n, gap)) = hit else { return false };
        if fwd.dot(-n) < 0.6 {
            return false;
        }
        // The face straight in front of us along its normal.
        let face = self.feet - n * (gap + body.reach_toward(n));
        let dist = (self.feet - face).dot(n);
        if dist < uu_per_s(20.0) {
            return false;
        }
        // The step: IntermediateFootPlantHeight, give or take 20 uu.
        let step_top = column_top(world, face - n * 0.05, self.feet.y, self.feet.y + tu.springboard_hi + 0.1);
        if (step_top - self.feet.y - tu.springboard_step_height).abs() >= tu.springboard_slack {
            return false;
        }
        // Behind it, within 112 x 1.414 uu: the first thing that rises above the step.
        let max_look = tu.springboard_step_spacing * std::f32::consts::SQRT_2;
        let mut found = None;
        let mut d = 0.02;
        while d <= max_look {
            let top = column_top(world, face - n * d, self.feet.y, self.feet.y + tu.springboard_hi + 0.1);
            if top > step_top + 0.05 {
                found = Some((d, top));
                break;
            }
            d += 0.02;
        }
        let Some((spacing, top)) = found else { return false };
        let rise = top - self.feet.y;
        if (spacing - tu.springboard_step_spacing).abs() >= tu.springboard_slack
            || rise < tu.springboard_lo
            || rise > tu.springboard_hi
        {
            return false;
        }
        // Room to stand on top of it.
        let plant = face - n * (spacing + tu.half_width + 0.02);
        let plant = Vec3::new(plant.x, top + 0.01, plant.z);
        let stand = Body { half_width: tu.half_width, height: tu.stand_height };
        if !world.is_free(&stand.aabb(plant)) {
            return false;
        }
        // StartMove / ReachedPreciseLocation: SavedInitialSpeed (at least 200 uu/s), launched
        // along the view at max(SpringBoardJumpXYMin, that + SpringBoardJumpXYAdd), and
        // SpringBoardJumpZ up.
        let saved = speed.max(uu_per_s(200.0));
        let launch = fwd * (saved + tu.springboard_xy_add).max(tu.springboard_xy_min) + Vec3::Y * tu.springboard_up;
        self.springboard = Some(SpringRunIn { face, n, plant, launch, speed: saved, t: 0.0 });
        self.springboard_run_in(0.0, world);
        true
    }

    /// Run in to 120 uu before the step at 1.2 x your speed (input ignored), then the two
    /// foot plants (StepTime1 + StepTime2) as one traverse, then launch.
    fn springboard_run_in(&mut self, dt: f32, world: &dyn World) {
        let Some(mut sb) = self.springboard else { return };
        let tu = self.tuning.clone();
        sb.t += dt;
        let dist = (self.feet - sb.face).dot(sb.n);
        if dist <= tu.springboard_run_in + 0.01 {
            self.springboard = None;
            self.state = State::Traverse(Traverse {
                kind: TraverseKind::SpringBoard,
                from: self.feet,
                to: sb.plant,
                t: 0.0,
                dur: tu.springboard_step_time,
                arc: 0.05,
                exit_vel: sb.launch,
            });
            self.last_jump_at = sb.plant;
            self.events.push(Event::SpringBoard);
            return;
        }
        if sb.t > 1.5 || !self.grounded(world) {
            // Blocked, or ran off something on the way: give up.
            self.springboard = None;
            return;
        }
        let run_speed = sb.speed * 1.2;
        self.vel = -sb.n * run_speed;
        self.springboard = Some(sb);
        if dt > 0.0 {
            // Don't overshoot the run-in point.
            let step = (run_speed * dt).min(dist - tu.springboard_run_in);
            let before = self.feet;
            self.integrate(step / run_speed, world, true);
            if horiz(self.feet - before).length() < step * 0.5 {
                self.springboard = None; // something in the way
            }
        }
    }

    // ---------------------------------------------------------------- air

    fn air(&mut self, dt: f32, input: &Input, world: &dyn World) {
        // TdLadderVolume.PawnUpdate.
        if self.try_into_climb(input, world) {
            return;
        }
        // Coming down on ground too steep to stand on: the rump slide (TdMove_Falling hands over
        // to it as the walk would).
        if self.vel.y <= 0.0 && self.try_rump_slide(world) {
            return;
        }
        let tu = self.tuning.clone();
        self.air_time += dt;

        if self.dodging {
            if self.vel.y < -tu.dodge_exit_fall_speed {
                self.end_dodge();
            } else {
                // TdMove_DodgeJump: falling physics with no air control and nothing to catch.
                self.vel.y -= tu.gravity * dt;
                self.integrate(dt, world, false);
                return;
            }
        }

        // Coyote jump.
        if self.jump_buffer > 0.0 && !self.jumped_since_ground && self.air_time < tu.coyote_time {
            if !self.crouched && !self.coiled {
                self.ground_jump();
            }
        }

        self.update_coil(input, world);

        if self.air_transitions(input, world) {
            return;
        }

        // Air control, as the game does it: the controller still asks for sprint acceleration
        // (TdPlayerController in PHYS_Falling), and APawn::physFalling (0xf07b30) clamps it to
        // AccelRate x AirControl. Below 10 uu/s it may make up the difference to that; at or past
        // GroundSpeed with AirControl under 0.05 it allows only 1 uu/s². (It also drops air
        // control for a tick when a trace says you'd steer into something; the collision here
        // stops you at the wall instead.)
        let ri = RunInput {
            forward: forward(self.yaw),
            right: right(self.yaw),
            a_forward: input.move_axis.y,
            a_strafe: input.move_axis.x,
            turn: self.frame_turn,
        };
        let h = horiz(self.vel);
        let want = horiz(tu.loco.sprint_accel(&mut self.run, &ri, h, dt, true));
        let speed = h.length();
        let mut max = tu.air_accel;
        if speed < 0.1 && tu.air_accel > 0.0 && dt > 0.0 {
            max += (0.1 - speed) / dt;
        } else if speed >= tu.loco.ground_speed && tu.air_accel <= 0.05 * tu.loco.accel_rate {
            max = 0.01;
        }
        let accel = want.clamp_length_max(max);
        let nh = h + accel * dt;
        let g = if self.low_grav > 0.0 { tu.gravity * self.low_grav_k } else { tu.gravity };
        self.vel = Vec3::new(nh.x, self.vel.y - g * dt, nh.z);

        self.integrate(dt, world, false);
    }

    /// TdMove_SoftLanding (the falling move's bCheckForSoftLanding): falling faster than
    /// SoftLandingZSpeedThreshold with a soft landing object below, brace for it.
    fn update_soft_brace(&mut self, world: &dyn World) {
        self.soft_brace = self.state == State::Air && self.vel.y < -self.tuning.soft_brace_speed && self.soft_below(world, 50.0);
    }

    /// Checks for grabbing, climbing and wallrunning from the air.
    fn air_transitions(&mut self, input: &Input, world: &dyn World) -> bool {
        let tu = self.tuning.clone();
        let body = self.body();
        let fwd = forward(self.yaw);
        let h = horiz(self.vel);
        let speed = h.length();

        if self.try_grab_fixture(world) {
            return true;
        }

        // TdMove_Jump / TdMove_Falling bCheckForVaultOver: holding forward (MoveActionHint "up",
        // the stick past 0.8), vault or step up onto whatever's ahead.
        if input.move_axis.y > 0.8 {
            let view = fwd * self.pitch.cos() + Vec3::Y * self.pitch.sin();
            if let Some(v) = vault::plan(world, &tu, body, self.feet, self.vel, fwd, view) {
                self.coiled = false;
                self.crouched = false;
                self.jump_buffer = 0.0;
                self.events.push(if v.onto() { Event::Mantle } else { Event::Vault });
                self.state = State::Vault(v);
                return true;
            }
        }

        // Wallclimb: TdMove_WallClimb's native start check (0x11f5000), which the jump's auto-move
        // check (0x1206f60) tries before vaults and grabs. Forward past 0.8 (MoveActionHint up),
        // still rising, moving forward, facing the wall within WallClimbingVerticalStartAngle,
        // the wall at least MinWallHeight (180) above the feet, and wall right in front of the
        // face: a trace from 64 units under the top of the head, CollisionRadius + 8 forward.
        if input.move_axis.y > 0.8
            && !self.climbed_this_air
            && !self.coiled
            && self.jumped_since_ground
            && self.vel.y > 0.0
            && h.dot(fwd) >= 0.0
        {
            if let Some(hit) = trace_wall(world, body, self.feet, fwd, uu(8.0), tu.stand_height - uu(64.0)) {
                let (n, gap) = (hit.n, hit.gap);
                // The same wall carries on up to WallClimbingMinWallHeight.
                let tall = trace_wall(world, body, self.feet, -n, gap + 0.1, tu.wallclimb_min_wall_height - 0.02)
                    .is_some_and(|h| h.n.dot(n) > 0.9);
                if fwd.dot(-n) >= tu.wallclimb_max_angle_deg.to_radians().cos() && tall {
                    // TdMove_WallClimb.ReachedWall: the boost grows with how far past run speed
                    // you came in (less the jump's JumpAddXY) and how fast you were still rising.
                    let into = speed - tu.jump_add_forward;
                    let xy = ((into - tu.run_speed).max(0.0) / (tu.wallclimb_add_xy_max_speed - tu.run_speed)).clamp(0.0, 1.0);
                    let z = (self.vel.y / tu.wallclimb_add_z_max_speed).clamp(0.0, 1.0);
                    let height = tu.wallclimb_add_xy_height * xy + tu.wallclimb_add_z_height * z;
                    self.vel = Vec3::Y * (4.0 * height * tu.wallclimb_gravity).sqrt();
                    self.feet += -n * gap;
                    self.climbed_this_air = true;
                    self.state = State::WallClimb { normal: n, t: 0.0 };
                    self.jump_buffer = 0.0;
                    self.events.push(Event::WallClimbStart);
                    return true;
                }
            }
        }

        // Wall directly ahead?
        if let Some((n, gap)) = probe_wall(world, body, self.feet, fwd, 0.35, 0.2, body.height - 0.1) {
            let facing = fwd.dot(-n);
            if facing > 0.5 {

                // Ledge grab.
                if self.regrab_cooldown <= 0.0 && self.vel.y < 2.5 {
                    let lift = if self.coiled { tu.coil_lift } else { 0.0 };
                    let hi = tu.hang_hands_above_feet + tu.ledge_grab_hi_extra - lift;
                    if let Some(top) =
                        find_ledge(world, body, self.feet, n, tu.mantle_hi - lift, hi, tu.crouch_height)
                    {
                        self.grab_ledge(n, gap, top, world);
                        return true;
                    }
                }

            }
        }

        // Wallrun (TdMove_WallRun.CanDoMove): at least WallRunningMinSpeed, not moving
        // backwards relative to the view, and a wall from FindWallSide/FindWallForward.
        if speed >= tu.wallrun_min_speed
            && h.dot(fwd) >= 0.0
            && input.move_axis.y > 0.3
            && self.vel.y > -tu.wallrun_stop_fall_speed
            && !self.crouched
            && !self.coiled
        {
            if let Some(WallHit { n, gap, .. }) = self.find_wallrun(input, world) {
                self.feet += -n * gap;
                // TdMove_WallRun.StartMove. A wallrun straight out of the last one's jump-off
                // is chained: it gets no lift, and pulls harder the more of them in a row.
                let chained = self.takeoff == Takeoff::WallJump;
                self.chain_wallruns = if chained { self.chain_wallruns + 1 } else { 0 };
                let accel = tu.wallrun_accel * (1 + self.chain_wallruns) as f32;
                // Entry speed is what you came in with (ReachedWall), at least
                // WallRunningVelocityStartLimit if you're still rising.
                let begin = if self.vel.y > 0.0 { speed.max(tu.wallrun_max_start_up) } else { speed };
                // Rising: lift you to WallRunningHorisontalInitialZHeight above where you jumped
                // (less any ceiling), with the wallrun's own gravity. Falling: keep falling.
                let vy = if self.vel.y > 0.0 && !chained {
                    let rise = self.headroom(world, tu.wallrun_rise_height) - (self.feet.y - self.last_jump_at.y);
                    (2.0 * rise.max(0.0) * accel).sqrt()
                } else {
                    self.vel.y
                };
                let along = (h - n * h.dot(n)).normalize_or_zero();
                self.vel = along * begin + Vec3::Y * vy;
                self.state = State::WallRun { normal: n, t: 0.0 };
                self.wall_turned = false;
                self.jump_buffer = 0.0;
                self.last_wall_normal = Some(n);
                // TdMove_WallRun.StartMove: a box (the cylinder's radius, 10 uu high) from 30 uu
                // over her head, WallRunningStrafeCheckDistance (50 uu) into the wall.
                let over = self.feet + Vec3::Y * (self.height() + uu(30.0) + uu(10.0));
                let half = Vec3::new(tu.half_width, uu(10.0), tu.half_width);
                self.wallrun_wall_above = world.sweep(half, over, -n * uu(50.0)).is_some();
                self.events.push(Event::WallRunStart);
                self.integrate(SUBSTEP, world, false);
                return true;
            }
        }
        false
    }

    /// The wall search in TdMove_WallRun.CanDoMove: the native FindWallSide (0x11f5a10) when
    /// you're strafing toward a wall right as you jump, otherwise FindWallForward (0x11f67b0).
    fn find_wallrun(&self, input: &Input, world: &dyn World) -> Option<WallHit> {
        let tu = &self.tuning;
        let body = self.body();
        let fwd = forward(self.yaw);
        let h = horiz(self.vel);
        let speed = h.length();
        // Both look for wall at MinWallHeight - 2 above the feet.
        let top = tu.wallrun_min_wall_height - uu(2.0);
        let same_wall = |n: Vec3| self.last_wall_normal.is_some_and(|l| l.dot(n) > 0.9);
        // Which way along the wall you'd run: the tangent on the side you face.
        let along = |n: Vec3| {
            let t = Vec3::new(n.z, 0.0, -n.x);
            if t.dot(fwd) < 0.0 { -t } else { t }
        };

        // MoveActionHint left/right only counts within 30 units of LastJumpLocation.
        let side_hint = (self.feet - self.last_jump_at).length() < uu(30.0) && input.move_axis.x.abs() > 0.3;
        let side = || -> Option<WallHit> {
            let dir = right(self.yaw) * input.move_axis.x.signum();
            let hi = trace_wall(world, body, self.feet, dir, tu.wallrun_reach, top)?;
            if hi.n.dot(dir).abs() < tu.wallrun_strafe_start_deg.to_radians().cos() || same_wall(hi.n) {
                return None;
            }
            if self.vel.dot(along(hi.n)) < 0.0 {
                return None;
            }
            // The same wall again just above step height, and near enough vertical between
            // the two (the direction from one hit to the other at least 0.96 up).
            let lo = trace_wall(world, body, self.feet, dir, tu.wallrun_reach, tu.step_height + uu(2.0))?;
            let rise = top - tu.step_height - uu(2.0);
            let off = (hi.at - lo.at).dot(hi.n).abs();
            (lo.n.dot(hi.n) > 0.999 && rise / (rise * rise + off * off).sqrt() >= 0.96).then_some(hi)
        };
        let ahead = || -> Option<WallHit> {
            // The reach grows from WallRunningForwardCheckDistance at SpeedMaxBase to
            // ContextMoveDistanceMultiplier times it at GroundSpeed.
            let l = &tu.loco;
            let frac = (speed - l.max_base).max(0.0) / (l.ground_speed - l.max_base);
            let reach = ((tu.context_move_distance_multiplier - 1.0) * frac + 1.0) * tu.wallrun_reach;
            let mid = trace_wall(world, body, self.feet, fwd, reach, body.height * 0.5)?;
            // Facing at most WallRunningForwardMaxStartAngle into the wall (MinStartAngle is 0).
            let d = -mid.n.dot(fwd);
            if d > (90.0 - tu.wallrun_max_approach_deg).to_radians().cos() || same_wall(mid.n) {
                return None;
            }
            // Tall enough: the wall again up at MinWallHeight, 50 units lower the squarer you face it.
            trace_wall(world, body, self.feet, fwd, reach, top - (1.0 - d) * uu(50.0))?;
            if self.vel.dot(along(mid.n)) < 0.0 {
                return None;
            }
            Some(mid)
        };
        let hit = if side_hint { side().or_else(ahead) } else { ahead() }?;

        // Back in CanDoMove: you have to be moving into the wall.
        if h.dot(hit.n) >= 0.0 {
            return None;
        }
        // Falling: no wallrun if there's ground just under where the run would take you
        // (a cylinder height below the feet, half a second of travel ahead, 45 units down).
        if self.vel.y < 0.0 {
            let start = Vec3::new(hit.at.x, self.feet.y - body.height * 0.5, hit.at.z) + hit.n * body.half_width;
            let end = start + along(hit.n) * speed * 0.5 - Vec3::Y * uu(45.0);
            let steps = ((end - start).length() / 0.05).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let p = start.lerp(end, i as f32 / steps as f32);
                if !world.is_free(&Aabb::from_center_size(p, Vec3::splat(uu(2.0)))) {
                    return None;
                }
            }
        }
        Some(hit)
    }

    fn grab_ledge(&mut self, n: Vec3, gap: f32, top: f32, world: &dyn World) {
        let tu = self.tuning.clone();
        // Hang with full body height.
        self.crouched = false;
        self.coiled = false;
        self.feet += -n * gap;
        self.feet.y = top - tu.hang_hands_above_feet;
        // Don't hang inside something below us.
        let body = self.body();
        if !world.is_free(&body.aabb(self.feet)) {
            self.feet.y = top - tu.hang_hands_above_feet + 0.4;
            self.crouched = true;
        }
        self.grab_speed = self.vel.y;
        self.vel = Vec3::ZERO;
        self.state = State::LedgeHang { normal: n, ledge_y: top, turned: false };
        self.hang_time = 0.0;
        self.shimmy = None;
        self.jump_buffer = 0.0;
        self.events.push(Event::LedgeGrab);
    }

    fn land(&mut self, impact: f32, world: &dyn World) {
        let tu = self.tuning.clone();
        self.climbed_this_air = false;
        self.last_wall_normal = None;
        let fall = self.fall_peak - self.body_y();
        self.uncoil_on_ground(world);

        if fall >= tu.lethal_fall_height {
            self.events.push(Event::Death);
            if tu.respawn_on_death {
                self.respawn();
                return;
            }
        }
        // TdMove_Landing.StartMove: out of a 180 in the air you're moving backwards, so no roll,
        // and LandBackwards beats a hard landing: dead stop, on your back (TdMove_LayOnGround).
        if self.turned_in_air {
            self.turned_in_air = false;
            self.vel = Vec3::ZERO;
            self.sprint_charge = 0.0;
            self.run = Default::default();
            self.crouched = false;
            self.state = State::LayOnGround { t: 0.0, getting_up: None, back_roll: false };
            self.events.push(Event::LandOnBack);
            return;
        }
        // TdPawn.CanSkillRoll: crouch pressed in the last 0.2 s, and not out of a turn in the air
        // (TdMove_180TurnInAir) or an air kick (TdMove_MeleeAir).
        let air_kick = self.melee.is_some_and(|m| m.kind == MeleeKind::AirKick);
        let can_roll = self.roll_trigger_age < tu.roll_window && self.turn.is_none() && !air_kick;
        if can_roll && fall >= tu.roll_height {
            self.crouched = true;
            let dir = horiz(self.vel).normalize_or(forward(self.yaw));
            let s = self.horizontal_speed().max(tu.run_speed);
            self.vel = dir * s;
            self.state = State::Roll { t: 0.0 };
            self.roll_trigger_age = f32::INFINITY;
            self.events.push(Event::Roll);
            return;
        }
        if fall >= tu.hard_land_height && self.soft_below(world, 0.1) {
            // TdMove_Landing.StartMove: a hard landing on something soft is LandOnSoftObject.
            self.vel = Vec3::ZERO;
            self.sprint_charge = 0.0;
            self.state = State::SoftLand { t: 0.0 };
            self.events.push(Event::SoftLand);
            return;
        }
        if fall >= tu.hard_land_height {
            // TdMove_Landing.LandHard: dead stop, no input until the animation ends.
            self.vel = Vec3::ZERO;
            self.sprint_charge = 0.0;
            self.state = State::Stunned { t: 0.0 };
            self.events.push(Event::HardLand);
            return;
        }
        // TdMove_Landing.SubtractLandingSpeed: landing a plain jump caps your speed at what you
        // jumped with, less LandingSpeedReduction. (The game doesn't floor that at zero, which
        // would push a slow landing backwards; we do.)
        if self.takeoff == Takeoff::Jump {
            let cap = (self.pre_jump_momentum - tu.landing_speed_reduction).max(0.0);
            let h = horiz(self.vel);
            if h.length() > cap {
                let h = h.normalize_or_zero() * cap;
                self.vel = Vec3::new(h.x, self.vel.y, h.z);
                self.sync_sprint_charge();
            }
        }
        self.events.push(Event::Land { impact, fall });
        self.state = State::Ground;
    }

    // ---------------------------------------------------------------- slide / roll / stun

    /// TdMove_Slide, rebuilt from the game: the script (TdMove_Slide), its native tick and abort
    /// check (UTdMove_Slide 0x11fdde0 / 0x11f8640), the walking physics' slide friction
    /// (ATdPawn 0x12bef70) and the player controller's slide input (PlayerWalking, case 16).
    ///
    /// - Move input is ignored for the whole slide (DisableMovementTime -1): no acceleration.
    /// - The body (pawn rotation) turns toward the view at SlideLookTurn x the angle between them
    ///   per second, A/D (MoveActionHint left/right) turn it 2000 units/s, and the velocity is
    ///   pointed along the body at the same speed.
    /// - CalcVelocity brakes it with GroundFriction x FrictionModifier (0.1) x the slope term.
    /// - It aborts into a crouch below SlideAbortSpeed or when you pull back (hint down).
    /// - Letting go of crouch ends it into a walk (or a crouch with no room), but not inside the
    ///   first 0.5 s: the request waits for StartMove's timer (bGoingInto / bRequestUncrouch).
    /// - Jump does nothing: the slide's input case only handles stop-crouch and melee.
    /// - However it ends, StopMove halves the velocity.
    fn slide(&mut self, dt: f32, t: f32, input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        let t = t + dt;
        self.crouched = true;
        self.jump_buffer = 0.0;
        if !self.grounded(world) {
            self.vel *= tu.slide_exit_keep;
            self.state = State::Air;
            self.air_time = 0.0;
            self.jumped_since_ground = false;
            return;
        }
        let hint_side = if input.move_axis.x > 0.3 { 1.0 } else if input.move_axis.x < -0.3 { -1.0 } else { 0.0 };
        let hint_down = hint_side == 0.0 && input.move_axis.y < -0.8;
        let abort = hint_down || self.vel.length() < tu.slide_abort_speed;
        let d = (self.yaw - self.slide_yaw + PI).rem_euclid(2.0 * PI) - PI;
        self.slide_yaw += dt * tu.slide_look_turn * d - hint_side * tu.slide_strafe_turn * dt;
        let h = forward(self.slide_yaw) * horiz(self.vel).length();
        let friction = tu.loco.ground_friction * tu.slide_friction_modifier;
        self.vel = tu.loco.calc_velocity(h, Vec3::ZERO, dt, 1.0, friction);
        let uncrouch = !input.crouch_held && t >= tu.slide_min_time;
        if abort || uncrouch {
            if uncrouch {
                self.try_stand(world);
            }
            self.vel *= tu.slide_exit_keep;
            self.sync_sprint_charge();
            self.state = State::Ground;
            self.events.push(Event::SlideEnd);
        } else {
            self.state = State::Slide { t };
        }
        self.integrate(dt, world, true);
    }

    fn roll_state(&mut self, dt: f32, t: f32, world: &dyn World) {
        let t = t + dt;
        self.crouched = true;
        self.vel.y = 0.0;
        if t > self.tuning.roll_time {
            self.try_stand(world);
            self.state = State::Ground;
        } else {
            self.state = State::Roll { t };
        }
        self.integrate(dt, world, true);
        if !self.grounded(world) {
            self.state = State::Air;
        }
    }

    /// TdMove_LayOnGround: lie there until you ask to get up: jump, or forward once the fall onto
    /// your back has played out, gets you up (JumpTurnLandingStand); pulling back (S, the
    /// MoveActionHint down, also checked as you land) rolls you backwards (GetUpBack, EvadeRoll)
    /// into a crouch. Then walking (or crouching with no room).
    fn lay_on_ground(&mut self, dt: f32, t: f32, getting_up: Option<f32>, back_roll: bool, input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        let t = t + dt;
        self.vel = Vec3::new(0.0, self.vel.y.min(0.0), 0.0);
        let hint_down = input.move_axis.x.abs() <= 0.3 && input.move_axis.y < -0.8;
        let (getting_up, back_roll) = match getting_up {
            Some(g) => (Some(g + dt), back_roll),
            None if hint_down => {
                self.events.push(Event::BackRoll);
                (Some(0.0), true)
            }
            None if self.jump_buffer > 0.0 || (input.move_axis.y > 0.8 && t >= tu.lay_on_ground_fall_time) => {
                self.jump_buffer = 0.0;
                (Some(0.0), false)
            }
            None => (None, false),
        };
        let done = if back_roll { tu.back_roll_time } else { tu.lay_on_ground_get_up_time };
        match getting_up {
            Some(g) if g >= done => {
                if back_roll {
                    // EvadeRoll's root motion: you end up behind where you lay.
                    let back = -forward(self.yaw) * tu.back_roll_distance;
                    self.move_by(back, world);
                    self.crouched = !self.try_stand(world);
                }
                self.state = State::Ground;
            }
            _ => self.state = State::LayOnGround { t, getting_up, back_roll },
        }
        self.integrate(dt, world, true);
    }

    /// Moves the feet by `d` horizontally, stopping at walls (root-motion moves land here at
    /// the end of their animation).
    fn move_by(&mut self, d: Vec3, world: &dyn World) {
        let body = self.body();
        slide_move(world, body, &mut self.feet, d);
    }

    /// TdMove_Barge.CanDoMove / StartBargin: melee with a door ahead, within BargeMinTraceDistance
    /// (90) or, running at it, BargeTraceTime (0.5 s) of the barge speed. Faster than
    /// BargeKickThresholdSpeed you shoulder through it at min(BargeMaxSpeed, speed +
    /// BargeAddOnSpeed); slower you kick it (MeleeKickObject).
    fn try_barge(&mut self) -> bool {
        let tu = self.tuning.clone();
        let fwd = forward(self.yaw);
        let h = horiz(self.vel);
        let speed = h.length();
        let forward_moving = speed > 0.01 && fwd.dot(h / speed) > 0.707;
        let barge_speed = (speed + tu.barge_add_speed).min(tu.barge_max_speed);
        let Some((door, dist)) = self.door_ahead(tu.barge_min_trace) else { return false };
        let hands = speed > tu.barge_kick_threshold && forward_moving;
        let dir = if hands { h / speed } else { fwd };
        // StartBargin: BargeInLeft at BargeAnimTime / TimeToDoor, clamped to 0.7..1.3.
        let rate = if hands { (tu.barge_anim_time / (dist / barge_speed).max(1e-3)).clamp(0.7, 1.3) } else { 1.0 };
        if hands {
            self.vel = dir * barge_speed;
        }
        self.state = State::Barge { t: 0.0, door, hands, hit: false, dir, rate };
        self.events.push(Event::Barge { hands });
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn barge(&mut self, dt: f32, t: f32, door: usize, hands: bool, mut hit: bool, dir: Vec3, rate: f32, world: &dyn World) {
        let tu = self.tuning.clone();
        let t = t + dt;
        if hands {
            // SetPreciseLocation along the barge for BargeTraceTime, at the barge speed; the door
            // gives as you hit it (TryGiveBargeDamage on bumping it), then BargeOutLeft plays out.
            let speed = horiz(self.vel).length().max(tu.barge_kick_threshold);
            self.vel = dir * speed;
            if !hit {
                let body = self.body();
                let touching = self.doors.get(door).copied().flatten().is_some_and(|b| {
                    let mut me = body.aabb(self.feet).translated(dir * 0.1);
                    me.min.y += 0.2;
                    me.overlaps(&b)
                });
                if touching {
                    hit = true;
                    self.open_door(door);
                }
            }
            self.integrate(dt, world, true);
            let done = t >= tu.barge_trace_time + 0.3 || (hit && t >= tu.barge_trace_time);
            self.state = if done { State::Ground } else { State::Barge { t, door, hands, hit, dir, rate } };
        } else {
            // MeleeKickObject: move input ignored for 0.6 s; the kick lands on its notify.
            self.vel = Vec3::new(0.0, self.vel.y.min(0.0), 0.0);
            if !hit && t >= tu.barge_kick_hit {
                hit = true;
                self.open_door(door);
            }
            self.integrate(dt, world, true);
            self.state = if t >= tu.barge_kick_time { State::Ground } else { State::Barge { t, door, hands, hit, dir, rate } };
        }
    }

    pub(crate) fn open_door(&mut self, door: usize) {
        if let Some(o) = self.doors_open.get_mut(door) {
            if *o == 0.0 {
                *o = 1e-3;
                self.events.push(Event::DoorOpened { door });
            }
        }
    }

    /// TdBarbedWireVolume.Touch: walking into barbed wire trips you (TdMove_Stumble), forward
    /// over it if it's in front of you, back off it if it's behind.
    fn check_barbed_wire(&mut self, world: &dyn World) {
        if !matches!(self.state, State::Ground | State::Slide { .. }) {
            return;
        }
        let me = self.body().aabb(self.feet);
        let wire = world.fixtures().iter().find_map(|f| match f {
            Fixture::BarbedWire { b } if b.overlaps(&me) => Some(*b),
            _ => None,
        });
        let Some(b) = wire else { return };
        let fwd = forward(self.yaw);
        let forward_hit = horiz(b.center() - self.feet).dot(fwd) > 0.0;
        if matches!(self.state, State::Slide { .. }) {
            self.vel *= self.tuning.slide_exit_keep;
        }
        self.vel = Vec3::ZERO;
        self.crouched = false;
        self.state = State::Stumble { t: 0.0, forward: forward_hit, dir: fwd };
        self.events.push(Event::Stumble { forward: forward_hit });
    }

    /// TdMove_Stumble: input ignored while the stumble plays; its root motion carries you
    /// (StumbleFwd ~2.6 m forward over 1 s; GetHitStumbleBwd ~1.25 m back, ended at 0.5 s).
    fn stumble(&mut self, dt: f32, t: f32, forward_hit: bool, dir: Vec3, world: &dyn World) {
        let tu = self.tuning.clone();
        let t = t + dt;
        self.vel = Vec3::new(0.0, self.vel.y.min(0.0), 0.0);
        let (len, dist) = if forward_hit { (tu.stumble_fwd_time, tu.stumble_fwd_distance) } else { (tu.stumble_bwd_time, -tu.stumble_bwd_distance) };
        if t >= len {
            self.move_by(dir * dist, world);
            self.state = State::Ground;
        } else {
            self.state = State::Stumble { t, forward: forward_hit, dir };
        }
        self.integrate(dt, world, true);
    }

    /// TdMove_Landing.LandOnSoftObject: FallingLandSoftLanding, input ignored, then walking.
    fn soft_land(&mut self, dt: f32, t: f32, world: &dyn World) {
        let t = t + dt;
        self.vel = Vec3::new(0.0, self.vel.y.min(0.0), 0.0);
        self.state = if t >= self.tuning.soft_land_time { State::Ground } else { State::SoftLand { t } };
        self.integrate(dt, world, true);
    }

    /// The soft landing object right under the feet, if any.
    fn soft_below(&self, world: &dyn World, depth: f32) -> bool {
        let me = self.body().aabb(self.feet);
        let probe = Aabb::new(Vec3::new(me.min.x, self.feet.y - depth, me.min.z), Vec3::new(me.max.x, self.feet.y + 0.05, me.max.z));
        world.fixtures().iter().any(|f| matches!(f, Fixture::SoftPad { b } if b.overlaps(&probe)))
    }

    fn stunned(&mut self, dt: f32, t: f32, world: &dyn World) {
        let t = t + dt;
        // LandHard zeroes the velocity and ignores move input until the animation ends.
        self.vel = Vec3::new(0.0, self.vel.y.min(0.0), 0.0);
        self.state = if t > self.tuning.hard_land_stun { State::Ground } else { State::Stunned { t } };
        self.integrate(dt, world, true);
    }

    // ---------------------------------------------------------------- walls

    fn wallrun(&mut self, dt: f32, n: Vec3, t: f32, input: &Input, world: &dyn World) {
        if self.try_into_climb(input, world) {
            return;
        }
        let tu = self.tuning.clone();
        let t = t + dt;
        let body = self.body();
        let h = horiz(self.vel);
        let along_dir = (h - n * h.dot(n)).normalize_or_zero();
        let speed = h.dot(along_dir);

        let strafe_dir = right(self.yaw) * input.move_axis.x.signum();
        if self.jump_buffer > 0.0
            && t > 0.1
            && input.move_axis.x.abs() >= tu.wallrun_dodge_threshold
            && strafe_dir.dot(n) > 0.0
        {
            self.dodge(strafe_dir, tu.wallrun_dodge_side, tu.wallrun_dodge_up, tu.wallrun_dodge_inertia);
            self.last_wall_normal = Some(n);
            self.events.push(Event::WallRunDodge { dir: strafe_dir });
            return;
        }
        let chain = (1 + self.chain_wallruns) as f32;
        if self.jump_buffer > 0.0 && t > 0.1 {
            // TdMove_WallrunJump.StartMove. `push` is how far you look away from the wall: it
            // pushes you out harder and higher, and trades away along-wall speed (looking
            // straight out keeps WallRunningPushForwardSpeedMin of it). The game's script calls
            // FMax(0, push) but drops the result; its look limits keep push >= 0 anyway. After
            // Q (bTurned90FromWall) it takes the wall normal itself, however far the view has
            // come round: the full push-off.
            let push = if self.wall_turned { 1.0 } else { forward(self.yaw).dot(n).max(0.0) };
            let height = self.headroom(world, tu.wallrun_jump_height + tu.wallrun_jump_height_turned_add * push);
            let up = tu.speed_for_height(height) / chain;
            let out = tu.wallrun_jump_out + tu.wallrun_jump_out_look_add * push;
            let keep = tu.wallrun_jump_forward_min + (1.0 - tu.wallrun_jump_forward_min) * (1.0 - push);
            let along = h - n * h.dot(n);
            self.vel = n * out + along * keep + Vec3::Y * up;
            self.last_jump_at = self.feet;
            self.state = State::Air;
            self.air_time = 0.0;
            self.jumped_since_ground = true;
            self.jump_buffer = 0.0;
            self.takeoff = Takeoff::WallJump;
            self.events.push(Event::WallJump);
            return;
        }

        let still_wall = probe_wall(world, body, self.feet, -n, 0.12, 0.3, 1.5)
            .is_some_and(|(wn, _)| wn.dot(n) > 0.9);
        let falling_too_fast = self.vel.y < -tu.wallrun_stop_fall_speed;
        if falling_too_fast || !still_wall || input.crouch_pressed || input.move_axis.y < -0.3 {
            self.state = State::Air;
            return;
        }

        // UTdMove_WallRun's per-frame update (0x12073c0) and ATdPawn::physWallRunning (0x12bba30):
        // the controller asks for no acceleration, so holding forward doesn't speed you up. Your
        // velocity is kept along the wall at its current speed, with a slight drag
        // (WallRunningHorisontalFriction, applied by CalcVelocity to the horizontal part only).
        // Vertically you're pulled down at WallRunningHorisontalAcceleration while rising and
        // WallRunningHorisontalDeceleration while falling, both x (1 + ConsequtiveWallruns).
        let speed = speed - speed * tu.wallrun_friction * dt;
        if speed < 0.1 {
            self.state = State::Air;
            return;
        }
        let pull = if self.vel.y > 0.0 { tu.wallrun_accel } else { tu.wallrun_decel } * chain;
        self.vel = along_dir * speed - n * 0.5 + Vec3::Y * (self.vel.y - pull * dt);
        self.state = State::WallRun { normal: n, t };
        self.integrate(dt, world, false);
    }

    fn wallclimb(&mut self, dt: f32, n: Vec3, t: f32, input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        let t = t + dt;
        let body = self.body();

        if self.jump_buffer > 0.0 && input.move_axis.x.abs() >= tu.wallclimb_dodge_threshold {
            // Sideways along the wall face.
            let along = Vec3::new(n.z, 0.0, -n.x) * input.move_axis.x.signum();
            let along = if along.dot(right(self.yaw) * input.move_axis.x.signum()) < 0.0 { -along } else { along };
            self.dodge(along, tu.wallclimb_dodge_side, tu.wallclimb_dodge_up, tu.wallclimb_dodge_inertia);
            self.vel += n * 0.5; // off the wall slightly
            self.last_wall_normal = Some(n);
            self.events.push(Event::WallClimbDodge { dir: along });
            return;
        }

        let hi = tu.hang_hands_above_feet + tu.ledge_grab_hi_extra;
        if let Some(top) = find_ledge(world, body, self.feet, n, 0.9, hi, tu.crouch_height) {
            self.grab_ledge(n, 0.0, top, world);
            return;
        }
        let still_wall = probe_wall(world, body, self.feet, -n, 0.12, 0.2, body.height - 0.1).is_some();
        if self.vel.y <= 0.0 || !still_wall {
            self.state = State::Air;
            return;
        }
        // UTdMove_WallClimb's per-frame update (0x12071f0): the pawn's acceleration is just
        // (0, 0, -WallClimbingGravity), with no drag on the climb itself
        // (WallClimbingVerticalFriction only slows sideways motion along the wall face).
        let vy = self.vel.y - tu.wallclimb_gravity * dt;
        self.vel = -n * 0.3 + Vec3::Y * vy;
        self.state = State::WallClimb { normal: n, t };
        self.integrate(dt, world, false);
    }

    fn wallclimb_turned(&mut self, dt: f32, n: Vec3, t: f32, _input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        let t = t + dt;
        if self.jump_buffer > 0.0 {
            // TdMove_WallClimb180TurnJump.JumpFromWall (a ceiling cuts the height).
            self.vel = n * tu.wallkick_out + Vec3::Y * tu.speed_for_height(self.headroom(world, tu.wallkick_height));
            self.last_jump_at = self.feet;
            self.state = State::Air;
            self.air_time = 0.0;
            self.jumped_since_ground = true;
            self.jump_buffer = 0.0;
            self.last_wall_normal = Some(n);
            self.events.push(Event::WallKick);
            return;
        }
        if t > tu.wallkick_window {
            self.state = State::Air;
            return;
        }
        // Slowly slide down the wall while we decide.
        self.vel = -n * 0.3 + Vec3::Y * (self.vel.y.min(0.5) - tu.gravity * 0.25 * dt);
        self.state = State::WallClimbTurned { normal: n, t };
        self.integrate(dt, world, false);
    }

    fn hang(&mut self, dt: f32, n: Vec3, ledge_y: f32, _turned: bool, input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        self.vel = Vec3::ZERO;
        self.hang_time += dt;
        // TdMove_Grab.UpdateViewRotation: the body stays facing the wall (bDisableFaceRotation)
        // and you're "turned" once the view is more than StartTurningAngle (90°) off it. That's
        // when the hang-turn animation reaches out, and when jump kicks you off backwards.
        let turned = forward(self.yaw).dot(-n) < 0.0;
        if self.shimmy.is_none() {
            self.state = State::LedgeHang { normal: n, ledge_y, turned };
        }

        if turned {
            if self.jump_buffer > 0.0 {
                self.vel = n * tu.hang_jump_out + Vec3::Y * tu.speed_for_height(tu.hang_jump_height);
                self.state = State::Air;
                self.air_time = 0.0;
                self.jumped_since_ground = true;
                self.jump_buffer = 0.0;
                self.regrab_cooldown = 0.4;
                self.last_wall_normal = Some(n);
                self.events.push(Event::WallKick);
            }
            return;
        }

        if input.crouch_pressed {
            self.feet += n * 0.05;
            self.state = State::Air;
            self.air_time = 1.0;
            self.regrab_cooldown = 0.35;
            return;
        }

        // Let the grab land first (the game's grab animation settles before
        // you can heave up).
        let settled = self.hang_time >= tu.hang_settle_time;
        // TdMove_Grab's MA_Jump: a transfer to a ledge above first (pushing up), then the pull-up.
        if settled && self.jump_buffer > 0.0 && input.move_axis.y > 0.5 && self.try_grab_transfer(n, ledge_y, world) {
            self.jump_buffer = 0.0;
            return;
        }
        if settled && (self.jump_buffer > 0.0 || input.move_axis.y > 0.5) {
            self.jump_buffer = 0.0;
            let stand = Body { half_width: tu.half_width, height: tu.stand_height };
            let crouch = Body { half_width: tu.half_width, height: tu.crouch_height };
            let to = self.feet + -n * (2.0 * tu.half_width + 0.15);
            // Onto whatever's up there: a roof that slopes up from the lip (up to 0.9 m higher
            // under her) as well as a flat top.
            let surface = crate::world::tops_below(world, Vec3::new(to.x, 0.0, to.z), tu.half_width, ledge_y + 0.9, ledge_y - 0.1)
                .first()
                .copied()
                .unwrap_or(ledge_y);
            let to = Vec3::new(to.x, surface.max(ledge_y) + 0.01, to.z);
            let end_crouched = !world.is_free(&stand.aabb(to));
            if end_crouched && !world.is_free(&crouch.aabb(to)) {
                return;
            }
            self.crouched = end_crouched;
            self.state = State::Traverse(Traverse {
                kind: TraverseKind::PullUp,
                from: self.feet,
                to,
                t: 0.0,
                dur: tu.pullup_time,
                arc: 0.15,
                exit_vel: Vec3::ZERO,
            });
            self.events.push(Event::PullUp);
            return;
        }

        self.shimmy_update(dt, n, ledge_y, input, world);
    }

    /// TdMove_Grab's shimmy. Holding A or D (past MoveActionHint's 0.3) steps hand over hand
    /// along the ledge, a 60-unit step per 1.07 s cycle of the clip, once DisableShimmyTime
    /// has passed and only while you look within 90° of the wall (bIsWithinForwardView). A step
    /// always finishes (AbortShimmy only stops it between steps), and you keep stepping for as
    /// long as you hold the key. Where the ledge runs out at an outside corner you shimmy round
    /// it onto the next face (CanShimmyAroundCorner).
    fn shimmy_update(&mut self, dt: f32, n: Vec3, ledge_y: f32, input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        if let Some(mut sh) = self.shimmy {
            sh.t += dt;
            let len = if sh.corner.is_some() { tu.shimmy_corner_time } else { tu.shimmy_step_time };
            let u = (sh.t / len).min(1.0);
            let k = if sh.corner.is_some() { smoothstep(u) } else { shimmy_progress(u) };
            self.feet = sh.from.lerp(sh.to, k);
            if let Some(new_n) = sh.corner {
                // Moving right round the corner turns you left to face the new face.
                let to_yaw = sh.yaw_from + sh.dir * PI * 0.5;
                self.yaw = sh.yaw_from + (to_yaw - sh.yaw_from) * smoothstep(u);
                if u >= 1.0 {
                    self.state = State::LedgeHang { normal: new_n, ledge_y, turned: false };
                }
            }
            if u < 1.0 {
                self.shimmy = Some(sh);
                return;
            }
            self.shimmy = None;
        }

        let dir = if input.move_axis.x > 0.3 {
            1.0
        } else if input.move_axis.x < -0.3 {
            -1.0
        } else {
            return;
        };
        let facing_wall = forward(self.yaw).dot(-n) > 0.0;
        if self.hang_time < tu.shimmy_delay || !facing_wall {
            return;
        }
        let side = Vec3::new(n.z, 0.0, -n.x) * dir;
        let body = self.body();
        // The hands, at the body's centre line, have to stay on the ledge: solid wall just
        // in front of them, under the lip.
        let hangs_at = |p: Vec3, n: Vec3| {
            let lip = Vec3::new(p.x, ledge_y - 0.05, p.z) - n * (tu.half_width + 0.05);
            world.is_free(&body.aabb(p))
                && !world.is_free(&Aabb::from_center_size(lip, Vec3::splat(0.02)))
                && find_ledge(world, body, p, n, tu.hang_hands_above_feet - 0.1, tu.hang_hands_above_feet + 0.1, tu.crouch_height)
                    .is_some_and(|y| (y - ledge_y).abs() <= 0.05)
        };
        // As far along as the ledge carries on, up to a full step.
        let mut reach = 0.0;
        while reach < tu.shimmy_step - 1e-4 {
            let next = (reach + 0.05).min(tu.shimmy_step);
            if !hangs_at(self.feet + side * next, n) {
                break;
            }
            reach = next;
        }
        if reach >= 0.1 {
            let to = self.feet + side * reach;
            self.shimmy = Some(Shimmy { dir, t: 0.0, from: self.feet, to, corner: None, yaw_from: self.yaw });
            return;
        }
        // Round an outside corner: the wall ends just beside you and its side face carries
        // the same ledge.
        let Some(hit) = trace_wall(world, body, self.feet, -n, 0.6, ledge_y - self.feet.y - 0.1) else {
            return;
        };
        // Where the wall ends beside us: step a thin probe along it until it stops finding it.
        let probe_y = ledge_y - self.feet.y - 0.1;
        let thin = Body { half_width: 0.01, height: body.height };
        let face = Vec3::new(hit.at.x, self.feet.y, hit.at.z);
        let mut end = None;
        let mut s = 0.0;
        while s <= 0.6 {
            let from = face + side * s + n * 0.05;
            if trace_wall(world, thin, from, -n, 0.15, probe_y).is_none_or(|h| h.n.dot(n) < 0.9) {
                end = Some(s);
                break;
            }
            s += 0.02;
        }
        let Some(s) = end else { return };
        let edge = face + side * s;
        let off = horiz(self.feet - hit.at).dot(n);
        if (edge - self.feet).dot(side) > tu.half_width + 0.15 {
            return;
        }
        let to = edge + side * off - n * (tu.half_width + 0.05);
        let around = edge + side * off + n * off;
        if hangs_at(to, side) && world.is_free(&body.aabb(around)) {
            self.shimmy = Some(Shimmy { dir, t: 0.0, from: self.feet, to, corner: Some(side), yaw_from: self.yaw });
            self.events.push(Event::ShimmyCorner);
        }
    }

    /// TdMove_SpeedVault.UpdateVaultMovement: up to the hand-plant, over, down to the end; then
    /// walking (onto, or over onto the floor) or falling, at the speed of the last leg.
    fn vault_step(&mut self, dt: f32, mut v: Vault, world: &dyn World) {
        let k = v.kind();
        let down_at = k.time_up + k.time_over;
        let was = v.t;
        v.t += dt;
        self.vel = Vec3::ZERO;
        // bEndMoveInMelee: the way down is TdMove_MeleeVault's (the same SetPreciseLocation to
        // VaultEndPosition over VaultTimeDown), its kick 0.3 s in (StartMove's SetTimer).
        if v.kick && was < down_at && v.t >= down_at && self.melee.is_none() {
            self.start_vault_kick();
        }
        let kicking = v.kick && self.melee.is_some_and(|m| m.kind == MeleeKind::VaultKick);
        if v.t < v.duration() {
            self.feet = v.position(v.t);
            self.state = State::Vault(v);
            return;
        }
        if kicking && (v.held || !self.melee.is_some_and(|m| m.detecting || m.detect_in.is_none())) {
            // Reached the end before the kick: still there (PHYS_Flying) until it's over.
            v.held = true;
            self.feet = v.end;
            self.state = State::Vault(v);
            return;
        }
        self.feet = v.end;
        // SetStoredVelocity: the way down's speed, no falling speed kept (Z = max(0, Z)).
        self.vel = if v.held { Vec3::ZERO } else if kicking { Vec3::new(v.exit_vel.x, v.exit_vel.y.max(0.0), v.exit_vel.z) } else { v.exit_vel };
        self.air_time = 0.0;
        self.fall_peak = self.body_y();
        self.state = if v.falling || !self.grounded(world) { State::Air } else { State::Ground };
        self.sync_sprint_charge();
    }

    fn traverse(&mut self, dt: f32, mut tr: Traverse, world: &dyn World) {
        tr.t += dt / tr.dur;
        let s = smoothstep(tr.t);
        // Rise first, then move over: feels more like hands-on-ledge.
        let rise = smoothstep((tr.t * 1.6).min(1.0));
        let mut p = tr.from.lerp(tr.to, s);
        p.y = tr.from.y + (tr.to.y - tr.from.y) * rise + tr.arc * (PI * tr.t.min(1.0)).sin();
        self.feet = p;
        if tr.t >= 1.0 {
            self.feet = tr.to;
            self.vel = tr.exit_vel;
            self.air_time = 0.0;
            self.jumped_since_ground = true;
            self.state = if tr.kind == TraverseKind::SpringBoard || !self.grounded(world) { State::Air } else { State::Ground };
        } else {
            self.state = State::Traverse(tr);
        }
    }

    // ---------------------------------------------------------------- fixtures

    /// Step onto a balance beam you're standing on.
    fn try_balance(&mut self, world: &dyn World) -> bool {
        for f in world.fixtures() {
            let Fixture::Beam { a, b } = *f else { continue };
            let len = (b - a).length();
            let (cp, s) = closest_on_segment(a, b, self.feet);
            if horiz(self.feet - cp).length() < 0.2 && (self.feet.y - cp.y).abs() < 0.08 && s > 0.05 && s < len - 0.05 {
                let u = (b - a) / len;
                self.feet = Vec3::new(cp.x, self.feet.y, cp.z);
                let along = self.vel.dot(u).clamp(-self.tuning.balance_speed, self.tuning.balance_speed);
                self.vel = u * along;
                self.sprint_charge = 0.0;
                // TdMove_Balance.StartMove: stepping on at an angle starts you leaning that way,
                // BalanceFactor = 0.2 x how far across the beam you were heading.
                let fwd = forward(self.yaw);
                let facing = if fwd.dot(u) >= 0.0 { u } else { -u };
                let right_of_beam = horiz(facing.cross(Vec3::Y)).normalize_or_zero();
                let lean = 0.2 * fwd.dot(right_of_beam);
                self.state = State::Balance { a, b, lean, danger: -1.0, t: 0.0 };
                self.events.push(Event::BalanceStart);
                return true;
            }
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn balance(&mut self, dt: f32, a: Vec3, b: Vec3, mut lean: f32, mut danger: f32, t: f32, input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        if !self.grounded(world) {
            self.state = State::Air;
            self.air_time = 0.0;
            self.jumped_since_ground = false;
            return;
        }
        if self.jump_buffer > 0.0 {
            self.ground_jump();
            self.integrate(dt, world, false);
            return;
        }
        let len = (b - a).length();
        let u = (b - a) / len;
        // PlayerBalanceWalk: the walk acceleration along the way the pawn faces (the beam),
        // through CalcVelocity at TdMove_Balance.SpeedModifier (0.34 x GroundSpeed).
        let facing = if forward(self.yaw).dot(u) >= 0.0 { u } else { -u };
        let ri = RunInput { forward: facing, right: facing.cross(Vec3::Y), a_forward: input.move_axis.y, a_strafe: 0.0, turn: 0.0 };
        let h = horiz(self.vel);
        let accel = tu.loco.walk_accel(&mut self.run, &ri, h, dt, false);
        let accel = facing * accel.dot(facing);
        let h = tu.loco.calc_velocity(h, accel, dt, tu.balance_speed_modifier, tu.loco.ground_friction);
        let h = u * h.dot(u);
        self.vel = Vec3::new(h.x, 0.0, h.z);

        // UTdMove_Balance's tick (0x120a280): BalanceFactor moves by
        //   CameraInfluence x clamp(view yaw off the beam / 45 deg)
        //   + GravityInfluence x sin(BalanceFactor x pi/2)   (it tips over by itself)
        //   + ControlInfluence x the strafe input           (PlayerBalanceWalk: aStrafe)
        //   + ExternalForce (0)
        // times (1 + SpeedInfluence x clamp(speed / 300 uu/s)), clamped to -1..1. At the
        // edge (|BalanceFactor| = 1) you're in danger: get back under 0.9 within TimeToCounter
        // (0.8 s) or fall off.
        let t = t + dt;
        let facing = if forward(self.yaw).dot(u) >= 0.0 { u } else { -u };
        let beam_yaw = (-facing.x).atan2(-facing.z);
        let off = (self.yaw - beam_yaw + PI).rem_euclid(2.0 * PI) - PI;
        // Our yaw is positive to the left; the game's is positive to the right.
        let camera = (-off / (PI / 4.0)).clamp(-1.0, 1.0);
        let speed = (self.horizontal_speed() / uu(300.0)).clamp(0.0, 1.0);
        let control = input.strafe_raw.clamp(-1.0, 1.0);
        let push = tu.balance_camera * camera + tu.balance_gravity * (lean * PI / 2.0).sin() + tu.balance_control * control;
        lean = (lean + push * (tu.balance_speed_influence * speed + 1.0) * dt).clamp(-1.0, 1.0);
        if danger < 0.0 {
            if lean.abs() >= 1.0 {
                danger = 0.0;
            }
        } else if lean.abs() <= tu.balance_recover {
            danger = -1.0;
        } else {
            danger += dt;
        }

        if danger > tu.balance_counter_time {
            let r = right(self.yaw);
            let side = (r - u * r.dot(u)).normalize_or_zero() * lean.signum();
            self.feet += side * 0.5;
            self.vel += side * 1.5;
            self.state = State::Air;
            self.air_time = 0.0;
            self.jumped_since_ground = true;
            self.events.push(Event::BalanceFall);
            return;
        }

        self.integrate(dt, world, true);
        let s = (self.feet - a).dot(u);
        if s < 0.0 || s > len {
            self.state = State::Ground;
            return;
        }
        let on = a + u * s;
        self.feet = Vec3::new(on.x, self.feet.y, on.z);
        self.state = State::Balance { a, b, lean, danger, t };
    }

    /// Catch a zipline or swing pole from the air.
    pub(crate) fn try_grab_fixture(&mut self, world: &dyn World) -> bool {
        if self.fixture_cooldown > 0.0 {
            return false;
        }
        let tu = self.tuning.clone();
        let root = self.root();
        let fwd = forward(self.yaw);
        for f in world.fixtures() {
            match *f {
                Fixture::ZipLine { a, b } => {
                    if self.vel.y < -tu.zip_max_fall {
                        continue;
                    }
                    let len = (b - a).length();
                    let u = (b - a) / len;
                    let hands = root + Vec3::Y * tu.zip_hang;
                    let (cp, s) = closest_on_segment(a, b, hands);
                    if cp.distance(hands) > tu.grab_reach || s > len - 1.0 || horiz(u).normalize_or_zero().dot(fwd) < 0.2 {
                        continue;
                    }
                    let feet = cp - Vec3::Y * tu.zip_hang;
                    let stand = Body { half_width: tu.half_width, height: tu.stand_height };
                    if !world.is_free(&stand.aabb(feet)) {
                        continue;
                    }
                    // TdMove_IntoZipLine.StartMove: to the line 100 units on, at
                    // max(speed, 400); ZiplineStart at 0.3 / that time (0.2..2). Coming at it
                    // from its left starts the ZipLine loop half way (ReachedPreciseLocation).
                    let approach = horiz(self.vel).length().max(uu(400.0));
                    let into = ((cp + u * uu(100.0)).distance(hands) / approach).max(1e-3);
                    self.zip_start_rate = (0.3 / into).clamp(0.2, 2.0);
                    let heading = if horiz(self.vel).length() > 0.5 { horiz(self.vel).normalize() } else { fwd };
                    self.zip_from_left = heading.dot(Vec3::new(-u.z, 0.0, u.x)) > 0.0;
                    self.zip_braced = false;
                    self.coiled = false;
                    self.crouched = false;
                    self.feet = feet;
                    let speed = self.vel.dot(u).max(tu.zip_min_speed);
                    self.vel = u * speed;
                    self.state = State::ZipLine { a, b, s, speed };
                    self.events.push(Event::ZipStart);
                    return true;
                }
                Fixture::SwingPole { a, b } => {
                    let hands = root + Vec3::Y * (tu.stand_height + 0.25);
                    let (cp, _) = closest_on_segment(a, b, hands);
                    if cp.distance(hands) > tu.grab_reach || self.vel.y > 4.0 {
                        continue;
                    }
                    let u = (b - a).normalize_or_zero();
                    let mut dir = Vec3::new(-u.z, 0.0, u.x).normalize_or_zero();
                    let heading = if horiz(self.vel).length() > 0.5 { horiz(self.vel).normalize() } else { fwd };
                    if dir.dot(heading) < 0.0 {
                        dir = -dir;
                    }
                    let com = root + Vec3::Y * 0.9;
                    let off = com - cp;
                    let angle = off.dot(dir).atan2(-off.y).clamp(-1.0, 1.0);
                    let rate = (self.vel.dot(dir) / tu.swing_length).clamp(-tu.swing_max_rate, tu.swing_max_rate);
                    let feet = swing_feet(cp, dir, angle, tu.swing_length);
                    let stand = Body { half_width: tu.half_width, height: tu.stand_height };
                    if !world.is_free(&stand.aabb(feet)) {
                        continue;
                    }
                    self.coiled = false;
                    self.crouched = false;
                    self.feet = feet;
                    self.state = State::Swing { a, b, at: cp, dir, angle, rate };
                    self.events.push(Event::SwingStart);
                    return true;
                }
                Fixture::Beam { .. } | Fixture::Door { .. } | Fixture::BarbedWire { .. } | Fixture::SoftPad { .. } | Fixture::Ladder(_) => {}
            }
        }
        false
    }

    fn let_go(&mut self) {
        self.state = State::Air;
        self.air_time = 0.0;
        self.jumped_since_ground = true;
        self.jump_buffer = 0.0;
        self.fixture_cooldown = 0.6;
    }

    #[allow(clippy::too_many_arguments)]
    fn zipline(&mut self, dt: f32, a: Vec3, b: Vec3, s: f32, speed: f32, input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        let len = (b - a).length();
        let u = (b - a) / len;
        if self.jump_buffer > 0.0 {
            self.vel = u * speed;
            self.vel.y = self.vel.y.max(0.0) + tu.jump_speed;
            self.let_go();
            self.events.push(Event::Jump);
            return;
        }
        if input.crouch_pressed {
            self.vel = u * speed;
            self.let_go();
            self.events.push(Event::ZipEnd { hit_wall: false });
            return;
        }
        // ME: at least MinZipAcceleration, plus gravity down the slope.
        let speed = (speed + (tu.zip_accel + tu.gravity * (-u.y).max(0.0)) * dt).min(tu.zip_max_speed);
        let ns = s + speed * dt;
        let feet = a + u * ns - Vec3::Y * tu.zip_hang;
        let stand = Body { half_width: tu.half_width, height: tu.stand_height };
        // The native zipline tick: a box (the cylinder, half its height) from 40 below its
        // centre along the ride, 600 units while moving (a hit braces for it:
        // PrepareForForwardImpact), 20 once braced (a hit is the impact).
        let half = Vec3::new(tu.half_width, tu.stand_height * 0.25, tu.half_width);
        let centre = self.feet + Vec3::Y * (tu.stand_height * 0.5 - uu(40.0));
        let braced = self.zip_braced;
        let ahead = world.sweep(half, centre, u * if braced { uu(20.0) } else { uu(600.0) }).is_some();
        if ahead && !braced {
            self.zip_braced = true;
            self.events.push(Event::ZipBrace);
        }
        if (ahead && braced) || !world.is_free(&stand.aabb(feet)) {
            self.vel = -u * 0.5;
            self.let_go();
            self.events.push(Event::ZipEnd { hit_wall: true });
            return;
        }
        self.feet = feet;
        self.vel = u * speed;
        if ns >= len - 0.2 {
            self.let_go();
            self.events.push(Event::ZipEnd { hit_wall: false });
        } else {
            self.state = State::ZipLine { a, b, s: ns, speed };
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn swing(&mut self, dt: f32, a: Vec3, b: Vec3, at: Vec3, dir: Vec3, mut angle: f32, mut rate: f32, input: &Input, world: &dyn World) {
        let tu = self.tuning.clone();
        let l = tu.swing_length;
        let tangent = |angle: f32, rate: f32| (dir * angle.cos() + Vec3::Y * angle.sin()) * rate * l;
        // Jump off on the forward swing.
        if self.jump_buffer > 0.0 && rate > 0.5 {
            // TdMove_Swing.JumpOff: another bar ahead, and it's the swing-to-swing jump.
            if let Some(target) = self.swing_target(a, b, at, dir, rate, world) {
                self.start_swing_jump(target);
                return;
            }
            let sp = tu.swing_exit_speed.max(rate * l * angle.cos().max(0.0));
            self.vel = dir * sp + Vec3::Y * (tu.jump_speed * 0.5 + 3.0 * angle.sin().max(0.0));
            self.low_grav = tu.swing_exit_gravity_time;
            self.low_grav_k = tu.swing_exit_gravity;
            self.let_go();
            self.events.push(Event::SwingJump);
            return;
        }
        if input.crouch_pressed {
            self.vel = tangent(angle, rate);
            self.let_go();
            return;
        }
        // Pendulum; holding forward pumps it up to the game's top swing speed.
        let max_amp = (1.0 - tu.swing_max_rate * tu.swing_max_rate * l / (2.0 * tu.gravity)).clamp(-1.0, 1.0).acos();
        let mut acc = -(tu.gravity / l) * angle.sin();
        if input.move_axis.y > 0.3 {
            let energy = 0.5 * rate * rate * l + tu.gravity * (1.0 - angle.cos());
            if energy < tu.gravity * (1.0 - max_amp.cos()) && angle.abs() < 0.6 {
                acc += rate.signum() * 4.0;
            }
        } else {
            rate *= 1.0 - 0.4 * dt;
        }
        rate += acc * dt;
        let new_angle = (angle + rate * dt).clamp(-max_amp - 0.1, max_amp + 0.1);
        let feet = swing_feet(at, dir, new_angle, l);
        let stand = Body { half_width: tu.half_width, height: tu.stand_height };
        if world.is_free(&stand.aabb(feet)) {
            angle = new_angle;
            self.feet = feet;
        } else {
            rate = -rate * 0.3;
        }
        self.vel = tangent(angle, rate);
        self.state = State::Swing { a, b, at, dir, angle, rate };
    }

    // ---------------------------------------------------------------- integration

    fn integrate(&mut self, dt: f32, world: &dyn World, on_ground: bool) {
        let step_height = self.tuning.step_height;
        let body = self.body();
        let d = self.vel * dt;
        let start = self.feet;

        // Horizontal, sliding along what we run into, with step-up when on the ground.
        let mut a = start;
        let mut hit = slide_move(world, body, &mut a, d);
        if on_ground && !hit.is_empty() {
            let mut s = start;
            move_axis(world, body, &mut s, 1, step_height);
            let stepped = slide_move(world, body, &mut s, d);
            move_axis(world, body, &mut s, 1, -step_height - 0.01);
            let da = horiz(a - start).length_squared();
            let ds = horiz(s - start).length_squared();
            if ds > da + 1e-6 && s.y >= start.y - 1e-3 {
                // A step up: the mesh stays where it was and follows (OffsetMeshZ).
                self.offset_mesh(start.y - s.y);
                a = s;
                hit = stepped;
            }
        }
        self.feet = a;
        // What we ran into takes the velocity going into it.
        for n in hit {
            let into = self.vel.dot(n);
            if into < 0.0 {
                self.vel -= n * into;
            }
        }

        // Vertical.
        let vy_before = self.vel.y;
        let hy = move_axis(world, body, &mut self.feet, 1, d.y);
        if hy.blocked {
            self.vel.y = 0.0;
            if d.y < 0.0 && !on_ground {
                self.land(-vy_before, world);
            }
        }

        // Walked into a step with no front to it (some games' stairs are only their tops) or
        // over a seam: onto its top, as the step-up would have, if there's room.
        if on_ground {
            let hw = body.half_width - 0.02;
            // Where she is, and where she was going if something stopped her.
            let want = start + Vec3::new(d.x, 0.0, d.z) + horiz(d).normalize_or_zero() * 0.03;
            let short = horiz(want - self.feet).length() > 1e-4;
            for at in [Some(self.feet), short.then_some(want)].into_iter().flatten() {
                let Some(&top) = tops_below(world, at, hw, start.y + step_height, self.feet.y + 0.005).first() else { continue };
                let up = Vec3::new(at.x, top, at.z);
                if world.is_free(&body.aabb(up + Vec3::Y * 0.001)) {
                    self.offset_mesh(self.feet.y - top);
                    self.feet = up;
                    break;
                }
            }
        }

        // Stick to slopes/stairs going down.
        if on_ground && !self.grounded(world) {
            let mut s = self.feet;
            let h = move_axis(world, body, &mut s, 1, -step_height);
            if h.blocked {
                self.offset_mesh(self.feet.y - s.y);
                self.feet = s;
            }
        }
    }

    /// ATdPawn::OffsetMeshZ (0x12ba240): the mesh shifted by `d`, kept within 24 uu of where it
    /// belongs.
    fn offset_mesh(&mut self, d: f32) {
        self.mesh_offset = (self.mesh_offset + d).clamp(-uu(24.0), uu(24.0));
    }

    /// ATdPawn's mesh smoothing tick (0x12ba2e0): the mesh's height comes back to the pawn at
    /// ten times the gap a second, never slower than 1 uu a tick.
    fn smooth_mesh(&mut self, dt: f32) {
        let gap = self.mesh_offset.abs();
        if gap > 0.0 {
            let step = (gap * dt * 10.0).max(uu(1.0)).min(gap);
            self.mesh_offset -= self.mesh_offset.signum() * step;
        }
    }

    // ---------------------------------------------------------------- view

    fn update_view(&mut self, dt: f32) {
        let tu = &self.tuning;
        // In the air with legs tucked, the head didn't move.
        let target_eye = if self.coiled {
            tu.stand_height - tu.eye_from_top - tu.coil_lift
        } else {
            self.height() - tu.eye_from_top
        };
        self.eye_height += (target_eye - self.eye_height) * (1.0 - (-14.0 * dt).exp());
    }

    /// The un-embellished camera: eye position and look angles. Bob, shake,
    /// tilt and FOV are layered on by [`crate::CameraFx`].
    pub fn view(&self) -> View {
        View { eye: self.feet + Vec3::Y * self.eye_height, yaw: self.yaw, pitch: self.pitch, roll: 0.0, fov_deg: 90.0 }
    }

    /// Where the feet would be with the legs straight (the body's root):
    /// `feet` minus the coil lift while the legs are tucked in the air.
    pub fn root(&self) -> Vec3 {
        Vec3::new(self.feet.x, self.body_y(), self.feet.z)
    }

    /// True while the legs are tucked in the air.
    pub fn is_coiled(&self) -> bool {
        self.coiled
    }
}

/// Feet position hanging from a swing pole at `at`: the body's centre swings
/// on a pendulum of length `l`; the collision box stays upright.
fn swing_feet(at: Vec3, dir: Vec3, angle: f32, l: f32) -> Vec3 {
    let com = at + (dir * angle.sin() - Vec3::Y * angle.cos()) * l;
    com - Vec3::Y * 0.9
}

