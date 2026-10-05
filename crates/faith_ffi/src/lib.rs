//! A C interface to faith_move (and, with a Mirror's Edge install, faith_anim), for running
//! Faith's movement inside another game: see `include/faith.h`. Everything crossing it is in
//! the host's own world frame and units (the Skyrim plugin: Z up, 70 units a metre, headings
//! clockwise from north).

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use faith_anim::retarget::{self, BoneRest, HostFrame, Placement, Retarget, Xform};
use faith_anim::{Rig, RigFrame};
use faith_move::{CameraFx, CameraFxSettings, Controller, Event, Input, LookLimiter, MeshWorld, Shot, SpeedBlur, Tuning};
use glam::{Quat, Vec2, Vec3};
use me_assets::FaithArms;

pub const API_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<Vec3> for FaithVec3 {
    fn from(v: Vec3) -> Self {
        FaithVec3 { x: v.x, y: v.y, z: v.z }
    }
}

impl From<FaithVec3> for Vec3 {
    fn from(v: FaithVec3) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}

/// One frame's controls.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithInput {
    /// Strafe right, forward: -1..1 each.
    pub move_x: f32,
    pub move_y: f32,
    /// How far the view turned this frame, radians: right, up.
    pub look_right: f32,
    pub look_up: f32,
    pub jump_pressed: u8,
    pub jump_held: u8,
    pub crouch_pressed: u8,
    pub crouch_held: u8,
    /// The 180 turn.
    pub turn_pressed: u8,
    /// Attack / barge.
    pub melee_pressed: u8,
    /// A takedown on whoever's in front of her.
    pub takedown_pressed: u8,
    /// Reaction Time (AttemptReactionTime): starts it when the meter's full.
    pub reaction_pressed: u8,
}

/// What happened this frame (bits of `FaithFrame::events`).
pub mod events {
    pub const JUMP: u64 = 1 << 0;
    pub const LAND: u64 = 1 << 1;
    pub const HARD_LAND: u64 = 1 << 2;
    pub const ROLL: u64 = 1 << 3;
    pub const SLIDE: u64 = 1 << 4;
    pub const WALLRUN: u64 = 1 << 5;
    pub const WALL_JUMP: u64 = 1 << 6;
    pub const WALLCLIMB: u64 = 1 << 7;
    pub const WALL_KICK: u64 = 1 << 8;
    pub const LEDGE_GRAB: u64 = 1 << 9;
    pub const PULL_UP: u64 = 1 << 10;
    pub const VAULT: u64 = 1 << 11;
    pub const MANTLE: u64 = 1 << 12;
    pub const TURN_180: u64 = 1 << 13;
    pub const DODGE: u64 = 1 << 14;
    pub const DEATH: u64 = 1 << 15;
    pub const SPRINGBOARD: u64 = 1 << 16;
    pub const MELEE: u64 = 1 << 17;
    pub const BARGE: u64 = 1 << 18;
    /// An attack landed (faith_melee_hits).
    pub const MELEE_HIT: u64 = 1 << 19;
    pub const OTHER: u64 = 1 << 31;
}

/// Where everything is after a step.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithFrame {
    pub feet: FaithVec3,
    pub velocity: FaithVec3,
    /// Where you look (heading clockwise from the host's north, radians) and how far up.
    pub heading: f32,
    pub pitch: f32,
    /// Which way the body faces (moves like the wallrun lock it).
    pub body_heading: f32,
    /// The camera: position and its forward, up and right directions.
    pub cam_pos: FaithVec3,
    pub cam_forward: FaithVec3,
    pub cam_up: FaithVec3,
    pub cam_right: FaithVec3,
    /// Horizontal field of view, degrees.
    pub fov_deg: f32,
    /// Landing impact speed this frame (host units / s), if `events` has LAND.
    pub land_impact: f32,
    pub events: u64,
    pub on_ground: u8,
    /// The body is animated (a Mirror's Edge install was found).
    pub animated: u8,
    /// The arms draw in the world's depth this frame (swinging) rather than over it.
    pub intermediate: u8,
    /// Low to the ground: crouched or sliding (the host's sneaking).
    pub low: u8,
    /// Mirror's Edge's speed blur this frame (TdMotionBlurShader.usf's MotionPacked.r): 0 still,
    /// about 0.5 at full speed straight ahead.
    pub speed_blur: f32,
    /// Reaction Time: the meter (0..100), and the game speed the host should run at (1 normally,
    /// down to 0.25 while it's on). The host's `dt` is game time (slowed by it).
    pub reaction_energy: f32,
    pub game_speed: f32,
}

/// A transform as `faith.h` passes them: rotation quaternion (x, y, z, w), translation, scale.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithXform {
    pub rot: [f32; 4],
    pub pos: [f32; 3],
    pub scale: f32,
}

impl From<FaithXform> for Xform {
    fn from(x: FaithXform) -> Self {
        Xform { rot: Quat::from_array(x.rot).normalize(), pos: Vec3::from_array(x.pos), scale: x.scale }
    }
}

impl From<Xform> for FaithXform {
    fn from(x: Xform) -> Self {
        FaithXform { rot: x.rot.to_array(), pos: x.pos.to_array(), scale: x.scale }
    }
}

pub struct Faith {
    frame: HostFrame,
    /// Where faith_move's origin is in the host's world (host units). Kept near the player, so
    /// its floats stay precise however far out the host's world goes (Skyrim's spans kilometres).
    origin: [f64; 3],
    /// The collision as the host gave it (host frame), for re-centring.
    host_tris: Vec<f32>,
    /// Thin capsules and boxes in it that may be ziplines, swing poles or beams (host frame).
    host_candidates: Vec<world_fixtures::FaithFixtureCandidate>,
    /// The host's own fixtures (faith_set_host_fixtures), and where they start in the world's
    /// fixture list; the doors she burst open this step (host indices).
    pub(crate) host_fixtures: Vec<host_fixtures::FaithHostFixture>,
    host_fixtures_base: usize,
    /// Each host fixture in the world's list (from host_fixtures_base on): its index in the host's.
    host_fixture_index: Vec<u32>,
    pub(crate) doors_opened: Vec<u32>,
    ctrl: Controller,
    fx: CameraFx,
    look: LookLimiter,
    shot: Shot,
    speed_blur: SpeedBlur,
    reaction: faith_move::reaction::ReactionTime,
    world: MeshWorld,
    /// What moves in the host's world (doors, gates), given again every frame (faith_set_moving).
    moving: MeshWorld,
    /// One of the app's maps, played instead of the host's world.
    course: Option<course::Course>,
    /// What her feet and hands touch, as the host says (faith_set_surfaces).
    surfaces: (faith_move::greybox::Surface, faith_move::greybox::Surface),
    /// faith_set_screen_scale: how much narrower than her arms' the view a body is drawn with is.
    screen_scale: f32,
    /// The attacks that landed in the last step (faith_melee_hits).
    hits: Vec<melee::FaithHit>,
    /// The takedowns started or finished in the last step (faith_takedowns).
    takedowns: Vec<melee::FaithTakedown>,
    anim: Option<(FaithArms, Rig)>,
    last: Option<RigFrame>,
    skeletons: Vec<Retarget>,
    /// The disarm's victim side (faith_bind_victim / faith_pose_victim).
    victims: victim::Victims,
    /// Faith's own first-person body, for the host to draw.
    parts: Vec<body::Part>,
    /// Faith's sounds (with Mirror's Edge's sound packages and a sound device).
    audio: Option<audio::Audio>,
    names: Vec<CString>,
    state_name: CString,
    anim_name: CString,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(e: impl std::fmt::Display) {
    LAST_ERROR.with(|l| *l.borrow_mut() = CString::new(e.to_string().replace('\0', " ")).unwrap_or_default());
}

/// Run `f`, never letting a panic cross into the host.
pub(crate) fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(e) => {
            let msg = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
            set_error(format!("faith: {msg}"));
            fallback
        }
    }
}

/// Where Mirror's Edge usually is.
const DEFAULT_PATHS: [&str; 6] = [
    r"C:\Games\Mirror's Edge",
    r"C:\Program Files (x86)\Steam\steamapps\common\mirrors edge",
    r"C:\Program Files\Steam\steamapps\common\mirrors edge",
    r"C:\Program Files\EA Games\Mirror's Edge",
    r"C:\Program Files (x86)\EA Games\Mirror's Edge",
    r"C:\Program Files (x86)\Origin Games\Mirror's Edge",
];

impl Faith {
    fn new(me_dir: Option<&Path>, units_per_meter: f32) -> Faith {
        let frame = HostFrame { units_per_meter, ..HostFrame::skyrim() };
        // The host's world has no floor to fall off and its own idea of dying: Faith only
        // reports a deadly fall (FAITH_EV_DEATH).
        let tuning = Tuning { kill_y: None, respawn_on_death: false, ..Tuning::default() };
        let mut ctrl = Controller::new(tuning, Vec3::ZERO, 0.0);
        let mut fx = CameraFx::default();
        let dirs: Vec<PathBuf> = match me_dir {
            Some(d) => vec![d.to_path_buf()],
            None => DEFAULT_PATHS.iter().map(PathBuf::from).collect(),
        };
        let mut anim = None;
        for d in dirs {
            if me_assets::cooked_pc(&d).is_none() {
                continue;
            }
            // Full-size textures: the body is drawn, not just animated.
            match FaithArms::load(&d, 1024) {
                Ok(arms) => {
                    let rig = Rig::new(&arms, ctrl.yaw);
                    rig.tune(&arms, &mut ctrl.tuning);
                    fx.settings = CameraFxSettings::animation_driven();
                    anim = Some((arms, rig));
                    break;
                }
                Err(e) => set_error(format!("Mirror's Edge at {}: {e}", d.display())),
            }
        }
        let parts = anim.as_ref().map_or(vec![], |(arms, _)| body::parts(arms));
        let audio = anim.as_ref().and_then(|(arms, _)| match audio::Audio::load(arms) {
            Ok(a) => Some(a),
            Err(e) => {
                set_error(format!("sound: {e}"));
                None
            }
        });
        Faith {
            frame,
            origin: [0.0; 3],
            host_tris: vec![],
            host_candidates: vec![],
            host_fixtures: vec![],
            host_fixtures_base: 0,
            host_fixture_index: vec![],
            doors_opened: vec![],
            ctrl,
            fx,
            look: LookLimiter::default(),
            shot: Shot::default(),
            speed_blur: SpeedBlur::default(),
            reaction: Default::default(),
            world: MeshWorld::new(vec![], vec![]),
            course: None,
            moving: MeshWorld::new(vec![], vec![]),
            hits: vec![],
            takedowns: vec![],
            screen_scale: 1.0,
            surfaces: (faith_move::greybox::Surface::Concrete, faith_move::greybox::Surface::Concrete),
            anim,
            last: None,
            skeletons: vec![],
            victims: Default::default(),
            parts,
            audio,
            names: vec![],
            state_name: CString::default(),
            anim_name: CString::default(),
        }
    }

    /// A host world point in faith_move's frame (and back).
    fn local(&self, p: Vec3) -> Vec3 {
        let o = self.origin;
        self.frame.point_back(Vec3::new((p.x as f64 - o[0]) as f32, (p.y as f64 - o[1]) as f32, (p.z as f64 - o[2]) as f32))
    }

    pub(crate) fn local_point(&self, p: Vec3) -> Vec3 {
        self.local(p)
    }

    pub(crate) fn host(&self, p: Vec3) -> Vec3 {
        let h = self.frame.point(p);
        let o = self.origin;
        Vec3::new((h.x as f64 + o[0]) as f32, (h.y as f64 + o[1]) as f32, (h.z as f64 + o[2]) as f32)
    }

    fn rebuild_world(&mut self) {
        let tris = self
            .host_tris
            .chunks_exact(9)
            .map(|t| [0, 3, 6].map(|i| self.local(Vec3::new(t[i], t[i + 1], t[i + 2]))))
            .filter(|t| t.iter().all(|p| p.is_finite()) && (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-12)
            .collect();
        self.world = MeshWorld::new(tris, vec![]);
        // Which of the thin pieces work as ziplines, swing poles and balance beams.
        let k = 1.0 / self.frame.units_per_meter;
        let cands: Vec<faith_move::fixtures::Candidate> = self
            .host_candidates
            .iter()
            .map(|c| faith_move::fixtures::Candidate {
                a: self.local(c.a.into()),
                b: self.local(c.b.into()),
                thickness: c.thickness * k,
                capsule: c.capsule != 0,
            })
            .collect();
        self.world.fixtures = faith_move::fixtures::classify(&self.world, &cands);
        self.host_fixtures_base = self.world.fixtures.len();
        let (index, own): (Vec<u32>, Vec<_>) = self.host_fixtures_local().into_iter().unzip();
        self.host_fixture_index = index;
        self.world.fixtures.extend(own);
        // Doors are known by their place in the list, which has just been rebuilt.
        self.ctrl.doors_open.clear();
    }

    /// Re-centre faith_move's origin on the player once they've gone far from it (standing on
    /// the ground, where nothing else holds a position).
    fn recentre(&mut self, far: f32) {
        let feet = self.ctrl.feet;
        if self.course.is_some() || feet.length() < far || self.ctrl.state != faith_move::State::Ground {
            return;
        }
        let h = self.frame.point(feet);
        if self.ctrl.rebase(-feet) {
            self.speed_blur.reset();
            self.origin = [self.origin[0] + h.x as f64, self.origin[1] + h.y as f64, self.origin[2] + h.z as f64];
            self.rebuild_world();
        }
    }

    /// Host heading (clockwise from north) <-> faith_move yaw.
    fn yaw_of_heading(&self, heading: f32) -> f32 {
        let d = self.frame.point_back(Vec3::new(heading.sin(), heading.cos(), 0.0));
        (-d.x).atan2(-d.z)
    }

    fn heading_of_yaw(&self, yaw: f32) -> f32 {
        let d = self.frame.point(Vec3::new(-yaw.sin(), 0.0, -yaw.cos()));
        d.x.atan2(d.y)
    }

    fn step(&mut self, dt: f32, i: &FaithInput) -> FaithFrame {
        let dt = dt.clamp(0.0, 0.1);
        let mv = Vec2::new(i.move_x, i.move_y);
        let input = Input {
            move_axis: mv.clamp_length_max(1.0),
            look: Vec2::new(-i.look_right, i.look_up),
            jump_pressed: i.jump_pressed != 0,
            jump_held: i.jump_held != 0,
            crouch_pressed: i.crouch_pressed != 0,
            crouch_held: i.crouch_held != 0,
            turn_pressed: i.turn_pressed != 0,
            melee_pressed: i.melee_pressed != 0,
            takedown_pressed: i.takedown_pressed != 0,
            strafe_raw: i.move_x.clamp(-1.0, 1.0),
        };
        // The attacking limb, where the animation last put it (TdMove_MeleeBase's sweep).
        self.ctrl.hit_bone = match (self.ctrl.melee_bone(), &self.anim, self.last) {
            (Some(bone), Some((arms, rig)), Some(r)) => {
                let place = Placement { origin: r.origin, body_rot: r.body_rot, legs_rot: r.legs_rot };
                retarget::bone_position(arms, &rig.driver.globals, &place, bone)
            }
            _ => None,
        };
        // TdPlayerController.UpdateReactionTime (PlayerTick, before the pawn moves).
        if i.reaction_pressed != 0 {
            self.reaction.attempt();
        }
        self.reaction.update(dt, self.ctrl.vel.length());
        if self.course.is_none() && !self.moving.tris.is_empty() {
            let both = faith_move::world::Layered { still: &self.world, moving: &self.moving };
            self.ctrl.step(dt, &input, &both);
        } else {
            self.ctrl.step(dt, &input, &self.world);
        }
        self.hits.clear();
        self.doors_opened.clear();
        if self.course.is_none() {
            for e in &self.ctrl.events {
                if let faith_move::Event::DoorOpened { door } = *e {
                    if let Some(&i) = door.checked_sub(self.host_fixtures_base).and_then(|d| self.host_fixture_index.get(d)) {
                        self.doors_opened.push(i);
                    }
                }
            }
        }
        self.takedowns.clear();
        for e in &self.ctrl.events {
            match *e {
                faith_move::Event::Takedown { target, anim, enemy_at, enemy_dir, clip_at, clip_dir } => self.takedowns.push(melee::FaithTakedown {
                    target,
                    anim: anim as u32,
                    done: 0,
                    enemy_at: self.host(enemy_at).into(),
                    enemy_dir: (self.frame.axes * enemy_dir).into(),
                    clip_at: self.host(clip_at).into(),
                    clip_dir: (self.frame.axes * clip_dir).into(),
                }),
                faith_move::Event::TakedownDone { target } => {
                    self.takedowns.push(melee::FaithTakedown { target, done: 1, ..Default::default() })
                }
                _ => {}
            }
            if let faith_move::Event::MeleeHit { target, damage, momentum, kind } = *e {
                self.hits.push(melee::FaithHit {
                    target,
                    damage,
                    momentum: (self.frame.axes * momentum * self.frame.units_per_meter).into(),
                    kind: kind as u32,
                });
            }
        }
        if let Some(c) = &mut self.course {
            c.update(dt, &mut self.ctrl);
        }
        self.recentre(200.0);
        self.look.apply(&mut self.ctrl, dt);
        self.shot = self.fx.update(dt, &self.ctrl, &input);
        let f = &self.frame;
        let c = &self.ctrl;

        let (cam_pos, cam_rot, body_yaw) = match &mut self.anim {
            Some((arms, rig)) => {
                let r = rig.update(dt, c, &self.shot, arms);
                self.last = Some(r);
                let fwd = r.body_rot * Vec3::NEG_Z;
                let body_yaw = (-fwd.x).atan2(-fwd.z);
                (r.cam_pos, r.cam_rot, body_yaw)
            }
            None => {
                let v = self.shot.view;
                let rot = Quat::from_euler(glam::EulerRot::YXZ, v.yaw, v.pitch.clamp(-1.55, 1.55), -v.roll);
                (v.eye, rot, c.yaw)
            }
        };

        let speed_blur = self.speed_blur.update(dt, cam_pos, cam_rot * Vec3::NEG_Z);
        let mut ev = 0u64;
        let mut land_impact = 0.0;
        for e in &c.events {
            ev |= match e {
                Event::Jump => events::JUMP,
                Event::Land { impact, .. } => {
                    land_impact = impact * f.units_per_meter;
                    events::LAND
                }
                Event::HardLand => events::HARD_LAND,
                Event::Roll => events::ROLL,
                Event::Slide => events::SLIDE,
                Event::WallRunStart => events::WALLRUN,
                Event::WallJump => events::WALL_JUMP,
                Event::WallClimbStart => events::WALLCLIMB,
                Event::WallKick => events::WALL_KICK,
                Event::LedgeGrab => events::LEDGE_GRAB,
                Event::PullUp => events::PULL_UP,
                Event::Vault => events::VAULT,
                Event::Mantle => events::MANTLE,
                Event::Turn180 => events::TURN_180,
                Event::Dodge { .. } | Event::WallRunDodge { .. } | Event::WallClimbDodge { .. } => events::DODGE,
                Event::Death => events::DEATH,
                Event::SpringBoard => events::SPRINGBOARD,
                Event::Melee { .. } => events::MELEE,
                Event::MeleeHit { .. } => events::MELEE_HIT,
                Event::Barge { .. } => events::BARGE,
                _ => events::OTHER,
            };
        }
        self.state_name = CString::new(c.state.name()).unwrap_or_default();
        if let Some((_, rig)) = &self.anim {
            self.anim_name = CString::new(rig.driver.current()).unwrap_or_default();
        }
        if let Some(audio) = &mut self.audio {
            let notifies = self.anim.as_ref().map(|(_, rig)| rig.driver.notifies().to_vec()).unwrap_or_default();
            let mut out = vec![];
            // Skyrim's ground doesn't say what it's made of the way Mirror's Edge's does: concrete.
            // What she steps on: on a course its boxes (as the app), else what the host says.
            let (foot, hand) = self.surfaces;
            let course_level = self.course.as_ref().map(|k| &k.level);
            let feet = c.feet;
            let surface = |is_hand: bool| match course_level {
                Some(l) => l.surface_near(feet, if is_hand { 0.9 } else { 0.3 }).unwrap_or(faith_move::greybox::Surface::Concrete),
                None if is_hand => hand,
                None => foot,
            };
            audio.director.update(dt, c, &notifies, self.anim.is_some(), self.shot.step_phase, &surface, &mut out);
            for cmd in out {
                audio.run(cmd);
            }
        }
        FaithFrame {
            feet: self.host(c.feet).into(),
            velocity: f.point(c.vel).into(),
            heading: self.heading_of_yaw(c.yaw),
            pitch: c.pitch,
            body_heading: self.heading_of_yaw(body_yaw),
            cam_pos: self.host(cam_pos).into(),
            cam_forward: (f.axes * (cam_rot * Vec3::NEG_Z)).into(),
            cam_up: (f.axes * (cam_rot * Vec3::Y)).into(),
            cam_right: (f.axes * (cam_rot * Vec3::X)).into(),
            fov_deg: self.shot.view.fov_deg,
            land_impact,
            events: ev,
            on_ground: matches!(c.state, faith_move::State::Ground) as u8,
            animated: self.anim.is_some() as u8,
            intermediate: self.last.is_some_and(|r| r.intermediate) as u8,
            low: (c.crouched || matches!(c.state, faith_move::State::Slide { .. })) as u8,
            reaction_energy: self.reaction.energy,
            game_speed: self.reaction.game_speed,
            speed_blur,
        }
    }
}

mod audio;
pub mod body;
pub mod course;
pub mod host_fixtures;
pub mod melee;
pub mod moving;
pub mod surfaces;
pub mod victim;
pub mod world_fixtures;

pub(crate) unsafe fn handle<'a>(h: *mut Faith) -> Option<&'a mut Faith> {
    unsafe { h.as_mut() }
}

/// The API version this library implements (`FAITH_API_VERSION`).
#[unsafe(no_mangle)]
pub extern "C" fn faith_api_version() -> u32 {
    API_VERSION
}

/// The last error on this thread, or "" (valid until the next call that fails).
#[unsafe(no_mangle)]
pub extern "C" fn faith_last_error() -> *const c_char {
    LAST_ERROR.with(|l| l.borrow().as_ptr())
}

/// Create a player. `me_install` is the Mirror's Edge folder (UTF-8), or null to look in the
/// usual places; without one the movement still works, unanimated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_create(me_install: *const c_char, units_per_meter: f32) -> *mut Faith {
    guard(std::ptr::null_mut(), || {
        let dir = (!me_install.is_null()).then(|| PathBuf::from(unsafe { CStr::from_ptr(me_install) }.to_string_lossy().into_owned()));
        let upm = if units_per_meter > 0.0 { units_per_meter } else { 70.0 };
        Box::into_raw(Box::new(Faith::new(dir.as_deref(), upm)))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_destroy(h: *mut Faith) {
    if !h.is_null() {
        guard((), || drop(unsafe { Box::from_raw(h) }));
    }
}

/// Is the body animated (Mirror's Edge was found)?
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_animated(h: *mut Faith) -> u8 {
    unsafe { handle(h) }.is_some_and(|f| f.anim.is_some()) as u8
}

/// Replace the collision with `count` triangles (9 floats each: three corners, host frame).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_set_world(h: *mut Faith, tris: *const f32, count: u32) {
    let Some(f) = (unsafe { handle(h) }) else { return };
    guard((), || {
        let data: &[f32] = if tris.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(tris, count as usize * 9) } };
        f.host_tris.clear();
        f.host_tris.extend_from_slice(data);
        if f.course.is_none() {
            f.rebuild_world();
        }
    });
}

/// Put the player's feet at `feet`, facing `heading`, standing still.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_teleport(h: *mut Faith, feet: FaithVec3, heading: f32) {
    let Some(f) = (unsafe { handle(h) }) else { return };
    guard((), || {
        // Back in the host's world, off any course.
        f.stop_course();
        let yaw = f.yaw_of_heading(heading);
        // Re-centre on where the player goes.
        f.origin = [feet.x as f64, feet.y as f64, feet.z as f64];
        f.rebuild_world();
        f.ctrl.spawn = f.local(feet.into());
        f.ctrl.spawn_yaw = yaw;
        f.ctrl.respawn();
        f.speed_blur.reset();
    });
}

/// Advance `dt` seconds with `input`; fills `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_step(h: *mut Faith, dt: f32, input: *const FaithInput, out: *mut FaithFrame) {
    let Some(f) = (unsafe { handle(h) }) else { return };
    guard((), || {
        let i = unsafe { input.as_ref() }.copied().unwrap_or_default();
        let frame = f.step(dt, &i);
        if let Some(o) = unsafe { out.as_mut() } {
            *o = frame;
        }
    });
}

/// Play one of Faith's idles now (standing still on the ground only; moving or looking round
/// ends it). Mirror's Edge also plays them by itself after 30-40 s standing still.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_play_idle(h: *mut Faith) -> u8 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || match &mut f.anim {
        Some((arms, rig)) => rig.driver.play_idle(&f.ctrl, arms) as u8,
        None => 0,
    })
}

/// How many sound cues were loaded (0: no sound).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_sound_cues(h: *mut Faith) -> u32 {
    unsafe { handle(h) }.and_then(|f| f.audio.as_ref()).map_or(0, |a| a.cues() as u32)
}

/// Faith's sound volume (0..1, default 0.8).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_sound_volume(h: *mut Faith, gain: f32) {
    if let Some(a) = unsafe { handle(h) }.and_then(|f| f.audio.as_mut()) {
        a.set_gain(gain);
    }
}

/// Pause Faith's sounds (menus, Faith switched off) or let them play.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_sound_pause(h: *mut Faith, paused: u8) {
    if let Some(a) = unsafe { handle(h) }.and_then(|f| f.audio.as_mut()) {
        a.set_paused(paused != 0);
    }
}

/// The movement state's name ("Ground", "WallRun", ...), valid until the next step.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_state_name(h: *mut Faith) -> *const c_char {
    unsafe { handle(h) }.map_or(c"".as_ptr(), |f| f.state_name.as_ptr())
}

/// The animation playing, or "".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_anim_name(h: *mut Faith) -> *const c_char {
    unsafe { handle(h) }.map_or(c"".as_ptr(), |f| f.anim_name.as_ptr())
}

/// Bind a host skeleton: `count` bones in parent order (`parents[i] < i`, or -1), each with its
/// rest local transform. `kind` 0 = the whole body (third person), 1 = first-person arms.
/// Returns its id for `faith_pose_skeleton`, or -1 (no animation, or nothing matched).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_bind_skeleton(
    h: *mut Faith,
    kind: u32,
    count: u32,
    names: *const *const c_char,
    parents: *const i32,
    rest: *const FaithXform,
) -> i32 {
    let Some(f) = (unsafe { handle(h) }) else { return -1 };
    guard(-1, || {
        let Some((arms, _)) = &f.anim else {
            set_error("no Mirror's Edge animation loaded");
            return -1;
        };
        let Some(bones) = (unsafe { bones_from(count, names, parents, rest) }) else { return -1 };
        let anchors = if kind == 1 { retarget::skyrim_arms_anchors() } else { retarget::skyrim_body_anchors() };
        let r = Retarget::new(bones, f.frame, arms, &retarget::skyrim_links(), &anchors).with_arm_ik(arms, &retarget::skyrim_arm_ik());
        if r.mapped() == 0 {
            set_error("no bones matched");
            return -1;
        }
        f.skeletons.push(r);
        (f.skeletons.len() - 1) as i32
    })
}

/// A host skeleton as faith_bind_skeleton takes it, or None (error set).
pub(crate) unsafe fn bones_from(count: u32, names: *const *const c_char, parents: *const i32, rest: *const FaithXform) -> Option<Vec<BoneRest>> {
    if names.is_null() || parents.is_null() || rest.is_null() {
        set_error("null skeleton");
        return None;
    }
    let n = count as usize;
    let (names, parents, rest) = unsafe { (std::slice::from_raw_parts(names, n), std::slice::from_raw_parts(parents, n), std::slice::from_raw_parts(rest, n)) };
    let mut bones = Vec::with_capacity(n);
    for i in 0..n {
        let name = if names[i].is_null() { String::new() } else { unsafe { CStr::from_ptr(names[i]) }.to_string_lossy().into_owned() };
        let parent = usize::try_from(parents[i]).ok();
        if parent.is_some_and(|p| p >= i) {
            set_error(format!("bone {i} ({name}) comes before its parent"));
            return None;
        }
        let x: Xform = rest[i].into();
        bones.push(BoneRest { name, parent, rot: x.rot, pos: x.pos, scale: x.scale });
    }
    Some(bones)
}

/// A host world transform in faith_move's re-centred host frame.
pub(crate) fn recentred(f: &Faith, x: FaithXform) -> Xform {
    let mut x: Xform = x.into();
    x.pos = Vec3::new((x.pos.x as f64 - f.origin[0]) as f32, (x.pos.y as f64 - f.origin[1]) as f32, (x.pos.z as f64 - f.origin[2]) as f32);
    x
}

/// How many of a bound skeleton's bones are driven.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_skeleton_mapped(h: *mut Faith, skeleton: i32) -> u32 {
    unsafe { handle(h) }.and_then(|f| f.skeletons.get(skeleton as usize)).map_or(0, |r| r.mapped() as u32)
}

/// This frame's pose for a bound skeleton: local transforms for all its bones into `out`
/// (`count` entries, as bound). `root_parent` is the world transform of the node its root
/// bones hang from. Returns 1 if posed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_pose_skeleton(h: *mut Faith, skeleton: i32, root_parent: FaithXform, out: *mut FaithXform) -> u8 {
    unsafe { faith_pose_skeleton_ex(h, skeleton, root_parent, 0, out) }
}

/// Pin the first-person skeleton's hands onto Faith's (`faith_pose_skeleton_ex` flags).
pub const POSE_PIN_HANDS: u32 = 1;
/// With POSE_PIN_HANDS: the hands placed as far across the screen as Faith's appear, for a body
/// drawn with a narrower view than her arms (faith_set_screen_scale).
pub const POSE_SCREEN_MATCH: u32 = 2;

/// How much narrower the view a body is drawn with (POSE_SCREEN_MATCH) is than her arms':
/// tan(its horizontal half angle) / tan(her arms'). 1: the same.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_set_screen_scale(h: *mut Faith, k: f32) {
    if let Some(f) = unsafe { handle(h) } {
        f.screen_scale = if k.is_finite() && k > 0.05 { k.min(4.0) } else { 1.0 };
    }
}

/// [`faith_pose_skeleton`] with flags: POSE_PIN_HANDS puts the hands exactly where Faith's are
/// (for what they hold, when her own body is drawn and the host's arms are hidden).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_pose_skeleton_ex(h: *mut Faith, skeleton: i32, root_parent: FaithXform, flags: u32, out: *mut FaithXform) -> u8 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        let (Some((_, rig)), Some(r), Some(frame)) = (&f.anim, f.skeletons.get(skeleton as usize), f.last) else { return 0 };
        if out.is_null() {
            return 0;
        }
        let place = Placement { origin: frame.origin, body_rot: frame.body_rot, legs_rot: frame.legs_rot };
        let mut local = vec![];
        // The skeleton's parent, in faith_move's re-centred host frame.
        let mut parent: Xform = root_parent.into();
        parent.pos = Vec3::new(
            (parent.pos.x as f64 - f.origin[0]) as f32,
            (parent.pos.y as f64 - f.origin[1]) as f32,
            (parent.pos.z as f64 - f.origin[2]) as f32,
        );
        let seen = (flags & POSE_SCREEN_MATCH != 0 && f.screen_scale > 0.0).then(|| (frame.cam_pos, frame.cam_rot, f.screen_scale));
        r.pose_seen(&rig.driver.globals, &place, parent, flags & POSE_PIN_HANDS != 0, seen, &mut local);
        let out = unsafe { std::slice::from_raw_parts_mut(out, local.len()) };
        for (o, l) in out.iter_mut().zip(&local) {
            *o = (*l).into();
        }
        1
    })
}

#[cfg(test)]
mod tests;

/// Mirror's Edge's auto step-up (TdMove_AutoStepUp, which the game ships switched off): walking
/// into something 35-48 uu high, she steps up onto it. For a host whose stairs aren't ramped.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_set_auto_step_up(h: *mut Faith, on: u8) {
    if let Some(f) = unsafe { handle(h) } {
        f.ctrl.tuning.auto_step_up = on != 0;
    }
}

/// The tallest step the auto step-up takes (host units; Mirror's Edge's is 48 uu, 0.48 m).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_set_auto_step_up_max(h: *mut Faith, height: f32) {
    if let Some(f) = unsafe { handle(h) } {
        if height.is_finite() && height > 0.0 {
            f.ctrl.tuning.auto_step_up_max = height / f.frame.units_per_meter;
        }
    }
}
