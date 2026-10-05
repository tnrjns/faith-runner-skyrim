//! Every number that shapes how movement feels lives here.
//!
//! Values marked `ME:` come straight from Mirror's Edge's own class defaults
//! (TdGame.u), converted from Unreal units: 1 uu = 1 cm, so divide by 100.
//! Values marked `guess` aren't stored as plain defaults in the scripts
//! (they live in native code, config, or animation data) and are hand-tuned.

use crate::locomotion::Locomotion;

#[derive(Clone, Debug)]
pub struct Tuning {
    // ---------------------------------------------------------------- body
    /// ME: TdPawn CollisionCylinder.CollisionRadius = 30
    pub half_width: f32,
    /// ME: CollisionCylinder.CollisionHeight = 90 (half-height) → 1.8 m
    pub stand_height: f32,
    /// ME: TdMove.ShrinkCollision: crouching and sliding the cylinder is 122 units tall.
    pub crouch_height: f32,
    /// ME: TdPawn.BaseEyeHeight = 76 above the cylinder centre → eye at 1.66 m
    pub eye_from_top: f32,
    /// ME: TdPawn.MaxWallStepHeight = 35
    pub step_height: f32,

    // ---------------------------------------------------------------- ground
    /// ME: TdPawn.SpeedMaxBaseVelocity = 400: running speed; above it is sprint
    pub run_speed: f32,
    /// ME: TdPawn.GroundSpeed = 720: full sprint
    pub sprint_speed: f32,
    /// ME: the ground movement itself (sprint/walk acceleration, CalcVelocity), ported from
    /// the game's native code: see locomotion.rs
    pub loco: Locomotion,
    /// ME: TdMove_Crouch.SpeedModifier = 0.2 (x GroundSpeed)
    pub crouch_speed_modifier: f32,
    /// ME: TdPawn.AccelRate = 6144
    pub ground_accel: f32,
    /// guess
    pub ground_decel: f32,

    // ---------------------------------------------------------------- air
    /// ME: the gravity Faith actually falls under. DefaultGame.ini sets the
    /// world's DefaultGravityZ = -800, but its own comment on TdPawn says
    /// "1600 is the downward speed after falling 780 cm", i.e.
    /// 1600² / (2 × 780) ≈ 1641 uu/s²: the pawn falls at about twice the world
    /// value (the doubling happens in native code). We use the measured one.
    pub gravity: f32,
    /// ME: TdMove_Jump.BaseJumpZ = 630
    pub jump_speed: f32,
    /// ME: TdMove_Jump.JumpAddXY = 100: extra forward speed on takeoff
    pub jump_add_forward: f32,
    /// ME: TdPawn.AirControl (0.025) × AccelRate (6144)
    pub air_accel: f32,
    /// guess
    pub coyote_time: f32,
    /// ME: TdPlayerController.JumpTapTime = 0.15
    pub jump_buffer: f32,
    /// ME: TdMove_Coil.TotalHeightBoost = 60: how far the legs tuck up
    pub coil_lift: f32,
    /// ME: TdMove_Coil.CoilMinTriggerSpeed = 100: forward speed (along the view) needed to coil
    pub coil_min_speed: f32,

    // ---------------------------------------------------------------- wallrun
    /// ME: TdMove_WallRun.WallRunningMinSpeed = 200
    pub wallrun_min_speed: f32,
    /// ME: WallRunningVelocityStartLimit = 300: StartMove's floor on the entry speed when
    /// you hit the wall still rising (it doesn't limit starting).
    pub wallrun_max_start_up: f32,
    /// ME: WallRunningHorisontalInitialZHeight = 170: the wallrun lifts you this high
    pub wallrun_rise_height: f32,
    /// ME: WallRunningVelocityStopLimit = -500: wallrun ends once falling this fast
    pub wallrun_stop_fall_speed: f32,
    /// ME: WallRunningHorisontalAcceleration = 820. Despite the name, the game's native wallrun
    /// update (0x12073c0) uses it as the wallrun's gravity while you're rising (and StartMove
    /// launches you with sqrt(2 × rise × this)). x (1 + chained wallruns).
    pub wallrun_accel: f32,
    /// ME: WallRunningHorisontalDeceleration = 500: the wallrun's gravity once you're falling.
    pub wallrun_decel: f32,
    /// ME: WallRunningHorisontalFriction = 0.05: the wallrun move's FrictionModifier, a drag on
    /// your speed along the wall (per second).
    pub wallrun_friction: f32,
    /// ME: WallRunningForwardMaxStartAngle = 57°: how steeply you may face into the wall
    /// (from along it) and still wallrun (FindWallForward); past it it's a wallclimb.
    pub wallrun_max_approach_deg: f32,
    /// ME: WallRunningStrafeStartAngle = 60°: FindWallSide takes a wall up to this far
    /// off square to your side.
    pub wallrun_strafe_start_deg: f32,
    /// ME: WallRunningForwardCheckDistance / StrafeCheckDistance = 50
    pub wallrun_reach: f32,
    /// ME: TdPhysicsMove.ContextMoveDistanceMultiplier = 1.8: FindWallForward looks this much
    /// further ahead at full sprint (scaled from SpeedMaxBase up to GroundSpeed).
    pub context_move_distance_multiplier: f32,
    /// ME: WallRunningMinWallHeight = 192
    pub wallrun_min_wall_height: f32,
    /// ME: TdMove_WallrunJump.WallRunningPushAwaySpeedNoob = 120
    pub wallrun_jump_out: f32,
    /// ME: WallRunningPushAwaySpeedProAdd = 400: extra push when looking away from the wall
    pub wallrun_jump_out_look_add: f32,
    /// ME: WallRunningPushForwardSpeedMin = 0.1: fraction of the along-wall speed kept when
    /// jumping off looking straight out (looking along the wall keeps all of it)
    pub wallrun_jump_forward_min: f32,
    /// ME: WallRunningJumpOffZHeightForward = 100
    pub wallrun_jump_height: f32,
    /// ME: WallRunningJumpOffZHeightMaxAddTurned = 60
    pub wallrun_jump_height_turned_add: f32,
    /// guess: camera tilt away from the wall
    pub wallrun_roll_deg: f32,

    // ---------------------------------------------------------------- wallclimb
    /// ME: TdMove_WallClimb.WallClimbingGravity = 800: the climb's gravity, exactly (the native
    /// update, 0x12071f0, sets the pawn's acceleration to -this). WallClimbingVerticalFriction = 6
    /// only applies to sideways motion along the wall.
    pub wallclimb_gravity: f32,
    /// ME: TdMove_WallClimb.ReachedWall: the climb boost is sqrt(4 × height × WallClimbingGravity),
    /// where height = AddOnSpeed2DHeight (60) × how far your run speed was from
    /// SpeedMaxBaseVelocity (400) to AddOnSpeed2DMaxLimit (650), plus AddOnSpeedZHeight (130)
    /// × your upward speed over AddOnSpeedZMaxLimit (320).
    pub wallclimb_add_xy_height: f32,
    pub wallclimb_add_xy_max_speed: f32,
    pub wallclimb_add_z_height: f32,
    pub wallclimb_add_z_max_speed: f32,
    /// ME: WallClimbingVerticalStartAngle = 33°: must face the wall within this
    pub wallclimb_max_angle_deg: f32,
    /// ME: MinWallHeight = 180
    pub wallclimb_min_wall_height: f32,
    /// ME: TdMove_WallClimb180TurnJump.JumpOffZHeight = 250
    pub wallkick_height: f32,
    /// ME: TdMove_WallClimb180TurnJump.JumpPushAwaySpeed = 400
    pub wallkick_out: f32,
    /// ME: TdMove_WallClimb180TurnJump.JumpTimeWindow = 0.6: how long after turning you can kick off
    pub wallkick_window: f32,

    // ---------------------------------------------------------------- ledges
    // (Vaults and step-ups use the game's own VaultTypes table: see vault.rs.)
    /// ME: StepUpHighMaxHeight = 148: the lowest ledge you grab from the air
    pub mantle_hi: f32,
    /// ME: TdMove_Grab.GrabDesiredLedgeOffset.Z = 92.8 above cylinder centre
    /// → ledge top is 1.83 m above the feet while hanging
    pub hang_hands_above_feet: f32,
    /// ME: TdMove_IntoGrab.HangFoldedUpperDeltaDistance = 35: highest reachable above hang
    pub ledge_grab_hi_extra: f32,
    /// guess (animation-driven in ME)
    pub pullup_time: f32,
    /// guess: how long a fresh grab holds before you can pull up (the grab
    /// animation landing on the lip)
    pub hang_settle_time: f32,
    /// ME: one hand-over-hand step of `HangStrafeLeft`/`HangStrafeRight` (the clip is 1.07 s
    /// and carries the body 60 units; TdMove_Grab.ShimmyVelocity is 60 units/s).
    pub shimmy_step: f32,
    pub shimmy_step_time: f32,
    /// ME: TdMove_Grab.DisableShimmyTime = 0.6: no shimmying this soon after grabbing.
    pub shimmy_delay: f32,
    /// ME: `HangCornerOutSideLeft`/`Right` length: shimmying round an outside corner.
    pub shimmy_corner_time: f32,
    /// ME: TdMove_GrabJump.GrabJumpOffZHeight = 160
    pub hang_jump_height: f32,
    /// ME: GrabJumpPushAwayMaxSpeed = 400
    pub hang_jump_out: f32,

    // ---------------------------------------------------------------- slide
    /// ME: TdMove_Slide.CanDoMove: Velocity · facing >= 350 (forward speed, not total)
    pub slide_min_speed: f32,
    /// ME: TdMove_Slide.StartMove sets a 0.5 s timer; letting go of crouch earlier only ends the
    /// slide once it fires
    pub slide_min_time: f32,
    /// ME: TdMove_Slide.StopMove halves the velocity, however the slide ends (jumping out included)
    pub slide_exit_keep: f32,
    /// ME: SlideAbortSpeed = 250: the native abort check (0x11f8640) ends the slide below this
    /// (total speed), or when you pull back (MoveActionHint down)
    pub slide_abort_speed: f32,
    /// ME: TdMove_Slide.FrictionModifier = 0.1: the walking physics (ATdPawn s1fc, 0x12bef70) brakes
    /// the slide with GroundFriction x this (x the slope term, 0 on flat ground). Move input is
    /// ignored for the whole slide (DisableMovementTime = -1), so it only ever brakes.
    pub slide_friction_modifier: f32,
    /// ME: the slide's native tick (UTdMove_Slide 0x11fdde0): each frame the body turns toward
    /// the view by dt x this x the angle between them (0.2 per second)...
    pub slide_look_turn: f32,
    /// ...and A/D (MoveActionHint left/right) turn it at 2000 units/s (11 degrees/s).
    pub slide_strafe_turn: f32,

    // ---------------------------------------------------------------- landing
    /// ME: TdMove_Landing.SkillRollLandingHeight = 200: falls this big can be rolled
    pub roll_height: f32,
    /// ME: HardLandingHeight = 530: falls this big hurt unless you roll
    pub hard_land_height: f32,
    /// ME: TdPawn.FallingUncontrolledHeight = 1000: past this you're done
    pub lethal_fall_height: f32,
    /// Falling below this height (m) is death: the maps' floor. None: no floor (a host game
    /// with its own world, where heights can be anything).
    pub kill_y: Option<f32>,
    /// Dying puts you back at the spawn point. Off, Death is only reported (the host decides).
    pub respawn_on_death: bool,
    /// ME: TdPawn.CanSkillRoll: RollTriggerTime + 0.2 > now, i.e. crouch pressed in the last
    /// 0.2 s before touching down
    pub roll_window: f32,
    /// ME: TdPawn.Tick only takes a new roll press 0.6 s after the last one, so mashing crouch
    /// on the way down doesn't work
    pub roll_retrigger: f32,
    /// ME: TdMove_Landing.LandingSpeedReduction = 65 uu/s. After a plain jump, a normal landing
    /// caps your horizontal speed at the speed you jumped with minus this (SubtractLandingSpeed),
    /// so the jump's forward boost is handed back. Dodges and wall jumps don't pay it.
    pub landing_speed_reduction: f32,
    /// ME: how long a hard landing holds you. TdMove_Landing.LandHard zeroes your velocity and
    /// ignores move and look input until `FallingLandHard` (2 s, blend out 0.2) ends
    /// (OnCustomAnimEnd -> walking).
    pub hard_land_stun: f32,
    /// ME: `JumpTurnLanding` (1.13 s): falling onto your back after a 180 in the air.
    pub lay_on_ground_fall_time: f32,
    /// ME: `JumpTurnLandingStand` (1.33 s): getting up off your back.
    pub lay_on_ground_get_up_time: f32,
    /// ME: TdMove_LayOnGround.GetUpBack: `EvadeRoll` (1 s), rolling you back about 3.6 m (its
    /// root motion) into a crouch.
    pub back_roll_time: f32,
    pub back_roll_distance: f32,
    /// ME: TdMove_Barge: BargeKickThresholdSpeed 250 (faster: shoulder through; slower: kick),
    /// BargeMinTraceDistance 90, BargeTraceTime 0.5, BargeAddOnSpeed 200, BargeMaxSpeed 500,
    /// BargeAnimTime 0.3.
    pub barge_kick_threshold: f32,
    pub barge_min_trace: f32,
    pub barge_trace_time: f32,
    pub barge_add_speed: f32,
    pub barge_max_speed: f32,
    pub barge_anim_time: f32,
    /// ME: `MeleeKickObject` (the slow barge): move input ignored 0.6 s; the kick lands about
    /// 0.36 s in (BargeHitNotify, with the clip's impact footstep).
    pub barge_kick_time: f32,
    pub barge_kick_hit: f32,
    /// ME: TdMove_Stumble with the barbed wire in front: `StumbleFwd` (1 s, about 2.6 m of root
    /// motion); behind you: `GetHitStumbleBwd`, cut off after 0.5 s (about 1.25 m back).
    pub stumble_fwd_time: f32,
    pub stumble_fwd_distance: f32,
    pub stumble_bwd_time: f32,
    pub stumble_bwd_distance: f32,
    /// ME: TdMove_Landing.LandOnSoftObject: `FallingLandSoftLanding` (1.5 s), input ignored.
    pub soft_land_time: f32,
    /// ME: TdPhysicsMove.SoftLandingZSpeedThreshold = -400: brace for a soft landing once
    /// falling faster than this toward one (TdMove_SoftLanding).
    pub soft_brace_speed: f32,
    /// guess: how long a landing roll lasts (the game plays `fallinglandroll`)
    pub roll_time: f32,

    // ---------------------------------------------------------------- dodge jumps
    /// ME: TdPlayerController.SetInputHint bMoveActionMax: |aStrafe| > 0.96 (a key is 1.0)
    pub dodge_strafe_threshold: f32,
    /// ME: TdMove_DodgeJump.ExitToFallingZSpeed = -190: the falling move takes over below it
    pub dodge_exit_fall_speed: f32,
    /// ME: TdMove_DodgeJump.RedoMoveTime = 0.3
    pub dodge_redo: f32,
    /// ME: TdMove_DodgeJump.BaseJumpZ = 300
    pub dodge_up: f32,
    /// ME: TdMove_DodgeJump.JumpAddXY = 600: sideways launch speed
    pub dodge_side: f32,
    /// ME: TdMove_DodgeJump.DodgeJumpInertiaConservation = 0.3: fraction of old velocity kept
    pub dodge_inertia: f32,
    /// ME: TdPlayerController.WallRunningDodgeJumpThreshold = 0.8
    pub wallrun_dodge_threshold: f32,
    /// ME: TdMove_WallrunDodgeJump BaseJumpZ 300 / JumpAddXY 600 / inertia 0.3
    pub wallrun_dodge_up: f32,
    pub wallrun_dodge_side: f32,
    pub wallrun_dodge_inertia: f32,
    /// ME: TdPlayerController.WallClimbingDodgeJumpThreshold = 0.8
    pub wallclimb_dodge_threshold: f32,
    /// ME: TdMove_WallClimbDodgeJump BaseJumpZ 700 / JumpAddXY 150 / inertia 1.0
    pub wallclimb_dodge_up: f32,
    pub wallclimb_dodge_side: f32,
    pub wallclimb_dodge_inertia: f32,

    // ---------------------------------------------------------------- turning
    /// Without the game's animations: how long a 180 takes (the clips take 0.17-0.6 s).
    pub turn180_time: f32,
    /// The game's turn clips' root yaw (set from the animations when the game is installed).
    pub turn_curves: Option<std::sync::Arc<crate::controller::TurnCurves>>,

    // ---------------------------------------------------------------- springboard
    /// A springboard (TdMove_SpringBoard.CanDoMove) takes two things in a row: a low step,
    /// IntermediateFootPlantHeight = 64 (±20) above the floor, and behind it something
    /// SpringBoardMinHeight = 80 to MaxHeight = 148 high whose front is
    /// IntermediateFootPlantDistance = 112 (±20) behind the step's. One foot on each, then launch.
    pub springboard_step_height: f32,
    pub springboard_step_spacing: f32,
    /// ME: the ±20 uu slack on both of the above
    pub springboard_slack: f32,
    pub springboard_lo: f32,
    pub springboard_hi: f32,
    /// ME: CheckDistanceTime = 1.0: jump when you'd reach the step within this many seconds
    pub springboard_check_time: f32,
    /// ME: StartMove runs you in (at 1.2 × your speed, ignoring input) to 120 uu before the
    /// step, then takes the two steps
    pub springboard_run_in: f32,
    /// ME: SpringBoardJumpZ = 950
    pub springboard_up: f32,
    /// ME: SpringBoardJumpXYAdd = -100, SpringBoardJumpXYMin = 400
    pub springboard_xy_add: f32,
    pub springboard_xy_min: f32,
    /// ME: StepTime1 + StepTime2 = 0.2 + 0.2: onto the step, then onto the top
    pub springboard_step_time: f32,

    // ---------------------------------------------------------------- balance
    /// ME: TdMove_Balance.SpeedModifier = 0.34 (x GroundSpeed)
    pub balance_speed_modifier: f32,
    /// The top speed on a beam: GroundSpeed x SpeedModifier.
    pub balance_speed: f32,
    /// ME: GravityInfluence 0.3, ControlInfluence 1.5, SpeedInfluence 2.5,
    /// CameraInfluence 0.3: how the lean grows and how you counter it.
    pub balance_gravity: f32,
    pub balance_control: f32,
    pub balance_speed_influence: f32,
    pub balance_camera: f32,
    /// ME: TdMove_Balance.TimeToCounter = 0.8: time at the edge before you fall
    pub balance_counter_time: f32,
    /// UTdMove_Balance tick: back under this lean you're out of danger (0.9)
    pub balance_recover: f32,

    // ---------------------------------------------------------------- zipline
    /// ME: TdMove_ZipLine.HangOffset.Z = -90: the pawn hangs this far below the
    /// cable, from its centre (so feet are 0.9 + 0.9 m down).
    pub zip_hang: f32,
    /// ME: MinZipVelocity = 300 / MinZipAcceleration = 400
    pub zip_min_speed: f32,
    pub zip_accel: f32,
    /// guess
    pub zip_max_speed: f32,
    /// ME: TdMove_IntoZipLine.ZVelocityFallLimit = -600: can't catch it falling faster
    pub zip_max_fall: f32,
    /// guess: how close the hands must pass to grab
    pub grab_reach: f32,

    // ---------------------------------------------------------------- swing pole
    /// ME: TdMove_Swing.SwingPendulumLength = 120
    pub swing_length: f32,
    /// ME: MaxSwingVelocity = 4.25 (rad/s at the bottom)
    pub swing_max_rate: f32,
    /// ME: ExitVelocityModifier = 600
    pub swing_exit_speed: f32,
    /// ME: SwingExitGravityModifier = 0.75 for SwingExitGravityModifierTime = 0.7 s
    pub swing_exit_gravity: f32,
    pub swing_exit_gravity_time: f32,

    // ---------------------------------------------------------------- melee
    /// ME: the attack animations' lengths (MeleeStart + MeleeMissed ≈ 0.7 s);
    /// the app sets these from the real animations.
    pub melee_clips: MeleeClips,
    /// The takedown clips' lengths (takedown::TAKEDOWN_ANIMS): each takedown lasts its clip.
    pub takedown_clips: [f32; 4],
    /// TdMove_AirBarge's clips: AirBargeIdle, AirBargeImpact, AirBargeLand.
    pub air_barge_clips: [f32; 3],
    /// The ladder clips' root motion (ExitAtTop, LadderEnterTop), when the host has them.
    pub climb_curves: Option<std::sync::Arc<crate::climb::ClimbCurves>>,
    /// TdMove_AutoStepUp: walking into something 35-48 uu high steps up onto it. Mirror's Edge
    /// ships it switched off (its levels ramp their stairs); for hosts whose stairs aren't.
    pub auto_step_up: bool,
    /// The tallest step it takes (StepUpHighMaxHeight, 48 uu). Hosts whose world is built of
    /// half-metre steps raise it a hair over them.
    pub auto_step_up_max: f32,
    /// ME: TdMove_MeleeCrouch.SpeedModifier = 0.2 / guess for standing
    pub melee_speed: f32,
}

/// Unreal units (cm) to metres.
const fn uu(v: f32) -> f32 {
    v / 100.0
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            half_width: uu(30.0),
            stand_height: uu(180.0),
            crouch_height: uu(122.0),
            eye_from_top: uu(180.0 - (90.0 + 76.0)),
            step_height: uu(35.0),

            run_speed: uu(400.0),
            sprint_speed: uu(720.0),
            loco: Locomotion::default(),
            crouch_speed_modifier: 0.2,
            ground_accel: uu(6144.0),
            ground_decel: 22.0,

            gravity: uu(1600.0 * 1600.0 / (2.0 * 780.0)),
            jump_speed: uu(630.0),
            jump_add_forward: uu(100.0),
            air_accel: 0.025 * uu(6144.0),
            coyote_time: 0.12,
            jump_buffer: 0.15,
            coil_lift: uu(60.0),
            coil_min_speed: uu(100.0),

            wallrun_min_speed: uu(200.0),
            wallrun_max_start_up: uu(300.0),
            wallrun_rise_height: uu(170.0),
            wallrun_stop_fall_speed: uu(500.0),
            wallrun_accel: uu(820.0),
            wallrun_decel: uu(500.0),
            wallrun_friction: 0.05,
            wallrun_max_approach_deg: 57.0,
            wallrun_strafe_start_deg: 60.0,
            wallrun_reach: uu(50.0),
            context_move_distance_multiplier: 1.8,
            wallrun_min_wall_height: uu(192.0),
            wallrun_jump_out: uu(120.0),
            wallrun_jump_out_look_add: uu(400.0),
            wallrun_jump_forward_min: 0.1,
            wallrun_jump_height: uu(100.0),
            wallrun_jump_height_turned_add: uu(60.0),
            wallrun_roll_deg: 12.0,

            wallclimb_gravity: uu(800.0),
            wallclimb_add_xy_height: uu(60.0),
            wallclimb_add_xy_max_speed: uu(650.0),
            wallclimb_add_z_height: uu(130.0),
            wallclimb_add_z_max_speed: uu(320.0),
            wallclimb_max_angle_deg: 33.0,
            wallclimb_min_wall_height: uu(180.0),
            wallkick_height: uu(250.0),
            wallkick_out: uu(400.0),
            wallkick_window: 0.6,

            mantle_hi: uu(148.0),
            hang_hands_above_feet: uu(90.0 + 92.8),
            ledge_grab_hi_extra: uu(35.0),
            pullup_time: 0.55,
            hang_settle_time: 0.35,
            shimmy_step: uu(60.0),
            shimmy_step_time: 1.07,
            shimmy_delay: 0.6,
            shimmy_corner_time: 1.37,
            hang_jump_height: uu(160.0),
            hang_jump_out: uu(400.0),

            slide_min_speed: uu(350.0),
            slide_min_time: 0.5,
            slide_exit_keep: 0.5,
            slide_abort_speed: uu(250.0),
            slide_friction_modifier: 0.1,
            slide_look_turn: 0.2,
            slide_strafe_turn: 2000.0 / 65536.0 * std::f32::consts::TAU,

            roll_height: uu(200.0),
            hard_land_height: uu(530.0),
            lethal_fall_height: uu(1000.0),
            kill_y: Some(-40.0),
            respawn_on_death: true,
            roll_window: 0.2,
            roll_retrigger: 0.6,
            landing_speed_reduction: uu(65.0),
            hard_land_stun: 1.8,
            lay_on_ground_fall_time: 1.13,
            lay_on_ground_get_up_time: 1.33,
            back_roll_time: 1.0,
            back_roll_distance: uu(359.0),
            barge_kick_threshold: uu(250.0),
            barge_min_trace: uu(90.0),
            barge_trace_time: 0.5,
            barge_add_speed: uu(200.0),
            barge_max_speed: uu(500.0),
            barge_anim_time: 0.3,
            barge_kick_time: 0.6,
            barge_kick_hit: 0.36,
            stumble_fwd_time: 1.0,
            stumble_fwd_distance: uu(256.0),
            stumble_bwd_time: 0.5,
            stumble_bwd_distance: uu(125.0),
            soft_land_time: 1.5,
            soft_brace_speed: uu(400.0),
            roll_time: 0.5,

            dodge_strafe_threshold: 0.96,
            dodge_exit_fall_speed: uu(190.0),
            dodge_redo: 0.3,
            dodge_up: uu(300.0),
            dodge_side: uu(600.0),
            dodge_inertia: 0.3,
            wallrun_dodge_threshold: 0.6,
            wallrun_dodge_up: uu(300.0),
            wallrun_dodge_side: uu(600.0),
            wallrun_dodge_inertia: 0.3,
            wallclimb_dodge_threshold: 0.6,
            wallclimb_dodge_up: uu(700.0),
            wallclimb_dodge_side: uu(150.0),
            wallclimb_dodge_inertia: 1.0,

            turn180_time: 0.2,
            turn_curves: None,

            springboard_step_height: uu(64.0),
            springboard_step_spacing: uu(112.0),
            springboard_slack: uu(20.0),
            springboard_lo: uu(80.0),
            springboard_hi: uu(148.0),
            springboard_check_time: 1.0,
            springboard_run_in: uu(120.0),
            springboard_up: uu(950.0),
            springboard_xy_add: uu(-100.0),
            springboard_xy_min: uu(400.0),
            springboard_step_time: 0.4,

            balance_speed_modifier: 0.34,
            balance_speed: 0.34 * uu(720.0),
            balance_gravity: 0.3,
            balance_control: 1.5,
            balance_speed_influence: 2.5,
            balance_camera: 0.3,
            balance_counter_time: 0.8,
            balance_recover: 0.9,

            zip_hang: uu(90.0 + 90.0),
            zip_min_speed: uu(300.0),
            zip_accel: uu(400.0),
            zip_max_speed: 13.0,
            zip_max_fall: uu(600.0),
            grab_reach: 0.5,

            swing_length: uu(120.0),
            swing_max_rate: 4.25,
            swing_exit_speed: uu(600.0),
            swing_exit_gravity: 0.75,
            swing_exit_gravity_time: 0.7,

            melee_clips: MeleeClips::default(),
            takedown_clips: [2.53, 2.10, 2.03, 1.97],
            air_barge_clips: [1.0, 0.6, 0.8],
            climb_curves: None,
            auto_step_up: false,
            auto_step_up_max: uu(48.0),
            melee_speed: 0.4,
        }
    }
}

impl Tuning {
    /// Launch speed needed to rise `height` metres under normal gravity.
    pub fn speed_for_height(&self, height: f32) -> f32 {
        (2.0 * self.gravity * height.max(0.0)).sqrt()
    }
}

/// How long each attack's clips play (seconds, at the rate the move plays them). Set from the
/// animations when they're loaded (faith_anim's `Rig::tune`); these stand in without them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeleeClips {
    /// TdMove_Melee: MeleeStart, MeleeHit, MeleeMissed (Left), at 1.5 x.
    pub punch_start: f32,
    pub punch_hit: f32,
    pub punch_missed: f32,
    /// TdMove_MeleeCrouch: MeleeCrouchStart, MeleeCrouchHit.
    pub crouch_start: f32,
    pub crouch_hit: f32,
    /// TdMove_MeleeAir: MeleeInAir, MeleeInAirStill, MeleeFromAbove, MeleeInAirHit.
    pub air: f32,
    pub air_still: f32,
    pub air_from_above: f32,
    pub air_hit: f32,
    /// TdMove_MeleeSlide: MeleeSlide. TdMove_MeleeWallrun: MeleeWallRunLeft.
    pub slide: f32,
    pub wallrun: f32,
    /// TdMove_MeleeVault: MeleeVaultOver.
    pub vault_kick: f32,
}

impl Default for MeleeClips {
    fn default() -> Self {
        MeleeClips {
            punch_start: 0.25,
            punch_hit: 0.45,
            punch_missed: 0.45,
            crouch_start: 0.4,
            crouch_hit: 0.5,
            air: 0.8,
            air_still: 0.8,
            air_from_above: 0.8,
            air_hit: 0.6,
            slide: 0.9,
            wallrun: 0.7,
            vault_kick: 0.8,
        }
    }
}
