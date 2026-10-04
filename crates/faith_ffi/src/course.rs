//! The app's training maps, played in the host: the map's boxes and fixtures become Faith's
//! whole world (the host's collision is set aside), placed with its origin at a host point.
//! Checkpoints, respawning and the time trial work as in the app; the host draws the boxes
//! (faith_course_mesh).

use std::ffi::{c_char, CString};

use glam::{Quat, Vec2, Vec3};

use faith_move::greybox::{Level, Look};
use faith_move::{Aabb, Fixture, MeshWorld, World};

use crate::{guard, handle, Faith, FaithVec3};

/// The maps, as the app has them.
const MAPS: [fn() -> Level; 4] =
    [faith_move::moves::moves, faith_move::rooftops::rooftops, faith_move::springboard::springboard, faith_move::greybox::greybox];

pub(crate) struct Course {
    pub level: Level,
    /// The checkpoint you respawn at (the furthest reached).
    pub checkpoint: usize,
    /// The time trial: running since you left the start area.
    pub timer: Option<f32>,
    pub last: Option<f32>,
    pub best: Option<f32>,
    /// The latest thing to tell the player (checkpoint, finish), and a count that changes with it.
    pub message: CString,
    pub message_seq: u32,
    names: Vec<CString>,
    name: CString,
    /// The host's settings, put back when the course ends.
    kill_y: Option<f32>,
    respawn_on_death: bool,
}

impl Course {
    fn say(&mut self, text: String) {
        self.message = CString::new(text).unwrap_or_default();
        self.message_seq = self.message_seq.wrapping_add(1);
    }

    fn respawn_at(&mut self, ctrl: &mut faith_move::Controller, i: usize) {
        let i = i.min(self.level.checkpoints.len() - 1);
        self.checkpoint = i;
        let cp = &self.level.checkpoints[i];
        ctrl.spawn = cp.spawn;
        ctrl.spawn_yaw = cp.yaw;
        ctrl.respawn();
        if i == 0 {
            self.timer = None;
        }
    }

    /// After a step: checkpoints reached, the time trial (as the app's `play`).
    pub(crate) fn update(&mut self, dt: f32, ctrl: &mut faith_move::Controller) {
        if let Some(i) = self.level.checkpoint_at(ctrl.feet) {
            if i > self.checkpoint {
                self.checkpoint = i;
                let cp = &self.level.checkpoints[i];
                ctrl.spawn = cp.spawn;
                ctrl.spawn_yaw = cp.yaw;
                let name = cp.name;
                self.say(format!("Checkpoint - {name}"));
            }
        }
        let f = ctrl.feet;
        if self.timer.is_none() && self.checkpoint == 0 && self.level.checkpoint_at(f) != Some(0) && ctrl.state == faith_move::State::Ground {
            self.timer = Some(0.0);
        }
        let at_finish = self.level.in_finish(f);
        if let Some(t) = &mut self.timer {
            *t += dt;
            if at_finish {
                let t = *t;
                self.timer = None;
                self.last = Some(t);
                let pb = self.best.is_none_or(|b| t < b);
                if pb {
                    self.best = Some(t);
                }
                self.say(format!("Finish {}{}", time_text(t), if pb { " - new best" } else { "" }));
                self.checkpoint = 0;
            }
        }
    }
}

fn time_text(t: f32) -> String {
    let m = (t / 60.0) as u32;
    format!("{m}:{:05.2}", t - m as f32 * 60.0)
}

/// The map's collision: its solid boxes as triangles, with its fixtures.
fn world_of(level: &Level) -> MeshWorld {
    let tris = level.solids.iter().flat_map(|(b, _)| MeshWorld::oriented_box((b.min + b.max) * 0.5, (b.max - b.min) * 0.5, 0.0)).collect();
    MeshWorld::new(tris, level.fixtures.clone())
}

impl Faith {
    pub(crate) fn start_course(&mut self, map: usize, anchor: [f64; 3]) -> bool {
        let Some(make) = MAPS.get(map) else { return false };
        let level = make();
        if level.checkpoints.is_empty() {
            return false;
        }
        let (kill_y, respawn_on_death) = match &self.course {
            Some(c) => (c.kill_y, c.respawn_on_death),
            None => (self.ctrl.tuning.kill_y, self.ctrl.tuning.respawn_on_death),
        };
        // Falling off the map or a deadly fall puts her back at the checkpoint, as in the app.
        let app = faith_move::Tuning::default();
        self.ctrl.tuning.kill_y = app.kill_y;
        self.ctrl.tuning.respawn_on_death = true;
        self.origin = anchor;
        self.world = world_of(&level);
        self.ctrl.doors_open.clear();
        let names = level.checkpoints.iter().map(|c| CString::new(c.name).unwrap_or_default()).collect();
        let name = CString::new(level.name).unwrap_or_default();
        let best = self.course.as_ref().filter(|c| c.level.name == level.name).and_then(|c| c.best);
        let mut course = Course {
            level,
            checkpoint: 0,
            timer: None,
            last: None,
            best,
            message: CString::default(),
            message_seq: 0,
            names,
            name,
            kill_y,
            respawn_on_death,
        };
        course.respawn_at(&mut self.ctrl, 0);
        self.ctrl.state = faith_move::State::Ground;
        self.speed_blur.reset();
        self.course = Some(course);
        true
    }

    pub(crate) fn stop_course(&mut self) {
        if let Some(c) = self.course.take() {
            self.ctrl.tuning.kill_y = c.kill_y;
            self.ctrl.tuning.respawn_on_death = c.respawn_on_death;
            self.ctrl.doors_open.clear();
            self.rebuild_world();
        }
    }
}

/// A course vertex, host frame and units. `look`: 0 roof, 1 wall, 2 runner (red), 3 prop,
/// 4 finish, 5 skyline, 6 metal (cables, bars, wire).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithCourseVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub look: u32,
}

/// Where the course is.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithCourse {
    pub active: u8,
    /// The time trial is running.
    pub running: u8,
    pub _pad: [u8; 2],
    pub time: f32,
    /// The last finished run and the best (-1: none yet).
    pub last: f32,
    pub best: f32,
    pub checkpoint: u32,
    pub checkpoints: u32,
    /// Changes whenever there's a new message (faith_course_message).
    pub message_seq: u32,
}

fn look_index(l: Look) -> u32 {
    match l {
        Look::Roof => 0,
        Look::Wall => 1,
        Look::Runner => 2,
        Look::Prop => 3,
        Look::Finish => 4,
        Look::Skyline => 5,
    }
}
const METAL: u32 = 6;


/// A box's faces in faith_move's frame, with the app's uvs (one grid square a metre), turned by
/// `rot` about `pivot`.
fn push_box(out: &mut Vec<(Vec3, Vec3, Vec2, u32)>, b: &Aabb, look: u32, rot: Quat, pivot: Vec3) {
    let (min, max) = (b.min, b.max);
    let faces: [(Vec3, [Vec3; 4]); 6] = [
        (Vec3::X, [Vec3::new(max.x, min.y, max.z), Vec3::new(max.x, min.y, min.z), Vec3::new(max.x, max.y, min.z), Vec3::new(max.x, max.y, max.z)]),
        (Vec3::NEG_X, [Vec3::new(min.x, min.y, min.z), Vec3::new(min.x, min.y, max.z), Vec3::new(min.x, max.y, max.z), Vec3::new(min.x, max.y, min.z)]),
        (Vec3::Y, [Vec3::new(min.x, max.y, max.z), Vec3::new(max.x, max.y, max.z), Vec3::new(max.x, max.y, min.z), Vec3::new(min.x, max.y, min.z)]),
        (Vec3::NEG_Y, [Vec3::new(min.x, min.y, min.z), Vec3::new(max.x, min.y, min.z), Vec3::new(max.x, min.y, max.z), Vec3::new(min.x, min.y, max.z)]),
        (Vec3::Z, [Vec3::new(min.x, min.y, max.z), Vec3::new(max.x, min.y, max.z), Vec3::new(max.x, max.y, max.z), Vec3::new(min.x, max.y, max.z)]),
        (Vec3::NEG_Z, [Vec3::new(max.x, min.y, min.z), Vec3::new(min.x, min.y, min.z), Vec3::new(min.x, max.y, min.z), Vec3::new(max.x, max.y, min.z)]),
    ];
    for (n, c) in faces {
        let uv = |p: Vec3| {
            let t = if n.x.abs() > 0.5 {
                Vec2::new(p.z, p.y)
            } else if n.y.abs() > 0.5 {
                Vec2::new(p.x, p.z)
            } else {
                Vec2::new(p.x, p.y)
            };
            Vec2::new(t.x, -t.y)
        };
        for k in [0, 1, 2, 0, 2, 3] {
            out.push((pivot + rot * (c[k] - pivot), rot * n, uv(c[k]), look));
        }
    }
}

/// A thin box from `a` to `b`, `thick` across (cables and bars).
fn push_rod(out: &mut Vec<(Vec3, Vec3, Vec2, u32)>, a: Vec3, b: Vec3, thick: f32) {
    let len = a.distance(b);
    if len < 1e-4 {
        return;
    }
    let rot = Quat::from_rotation_arc(Vec3::Z, (b - a) / len);
    let mid = (a + b) * 0.5;
    let h = Vec3::new(thick * 0.5, thick * 0.5, len * 0.5);
    let start = out.len();
    push_box(out, &Aabb::new(-h, h), METAL, Quat::IDENTITY, Vec3::ZERO);
    for v in &mut out[start..] {
        v.0 = mid + rot * v.0;
        v.1 = rot * v.1;
    }
}

/// The course's triangles this frame (doors swung as far as they're open), in faith_move's
/// frame, grouped by look.
fn mesh(c: &Course, doors_open: &[f32]) -> Vec<(Vec3, Vec3, Vec2, u32)> {
    let mut out = vec![];
    for (b, look) in c.level.solids.iter().chain(&c.level.decor) {
        push_box(&mut out, b, look_index(*look), Quat::IDENTITY, Vec3::ZERO);
    }
    for (i, f) in c.level.fixtures.iter().enumerate() {
        match *f {
            Fixture::ZipLine { a, b } => push_rod(&mut out, a, b, 0.025),
            Fixture::SwingPole { a, b } => push_rod(&mut out, a, b, 0.06),
            Fixture::Door { b, n } => {
                // As the app swings it: hinged on its -X edge, eased out to 100 degrees, away
                // from the side you came from.
                let open = doors_open.get(i).copied().unwrap_or(0.0);
                let k = 1.0 - (1.0 - open).powi(3);
                let sign = if -n.z < 0.0 { 1.0 } else { -1.0 };
                let hinge = Vec3::new(b.min.x, b.min.y, (b.min.z + b.max.z) * 0.5);
                push_box(&mut out, &b, look_index(Look::Prop), Quat::from_rotation_y(sign * k * 100f32.to_radians()), hinge);
            }
            Fixture::BarbedWire { b } => push_box(&mut out, &b, METAL, Quat::IDENTITY, Vec3::ZERO),
            Fixture::Ladder(l) => {
                for (a, b, thick) in l.rods() {
                    push_rod(&mut out, a, b, thick);
                }
            }
            Fixture::Beam { .. } | Fixture::SoftPad { .. } => {}
        }
    }
    out.sort_by_key(|v| v.3);
    out
}

/// Start one of the app's maps (0 Moves, 1 Rooftops, 2 Springboard, 3 Training) with its origin
/// at `anchor` (host frame): it becomes Faith's whole world, she's put at its start, and
/// faith_set_world is kept aside until faith_course_stop. 0 if there's no such map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_course_start(h: *mut Faith, map: u32, anchor: FaithVec3) -> u8 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || f.start_course(map as usize, [anchor.x as f64, anchor.y as f64, anchor.z as f64]) as u8)
}

/// Leave the course: the host's collision again (put the player back with faith_teleport).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_course_stop(h: *mut Faith) {
    let Some(f) = (unsafe { handle(h) }) else { return };
    guard((), || f.stop_course());
}

/// Back to a checkpoint: `checkpoint` < 0 the current one (0 restarts the time trial).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_course_respawn(h: *mut Faith, checkpoint: i32) {
    let Some(f) = (unsafe { handle(h) }) else { return };
    guard((), || {
        if let Some(c) = &mut f.course {
            let i = if checkpoint < 0 { c.checkpoint } else { checkpoint as usize };
            c.respawn_at(&mut f.ctrl, i);
            f.speed_blur.reset();
        }
    });
}

/// The course's state (all zero when there's none). Returns whether there is one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_course_status(h: *mut Faith, out: *mut FaithCourse) -> u8 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        let s = match &f.course {
            Some(c) => FaithCourse {
                active: 1,
                running: c.timer.is_some() as u8,
                _pad: [0; 2],
                time: c.timer.unwrap_or(0.0),
                last: c.last.unwrap_or(-1.0),
                best: c.best.unwrap_or(-1.0),
                checkpoint: c.checkpoint as u32,
                checkpoints: c.level.checkpoints.len() as u32,
                message_seq: c.message_seq,
            },
            None => FaithCourse::default(),
        };
        if !out.is_null() {
            unsafe { *out = s };
        }
        s.active
    })
}

/// The course's latest message ("Checkpoint - M2 Balance", "Finish 0:41.20 - new best"), its
/// name, and its checkpoints' names. Valid until the course changes; "" when there's none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_course_message(h: *mut Faith) -> *const c_char {
    unsafe { handle(h) }.and_then(|f| f.course.as_ref()).map_or(c"".as_ptr(), |c| c.message.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_course_name(h: *mut Faith) -> *const c_char {
    unsafe { handle(h) }.and_then(|f| f.course.as_ref()).map_or(c"".as_ptr(), |c| c.name.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_course_checkpoint_name(h: *mut Faith, i: u32) -> *const c_char {
    unsafe { handle(h) }
        .and_then(|f| f.course.as_ref())
        .and_then(|c| c.names.get(i as usize))
        .map_or(c"".as_ptr(), |n| n.as_ptr())
}

/// The course's triangles this frame (host frame and units; three vertices each, grouped by
/// look), up to `max` vertices into `out`. Returns how many there are (call with max 0 to size
/// the buffer).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_course_mesh(h: *mut Faith, out: *mut FaithCourseVertex, max: u32) -> u32 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        let Some(c) = &f.course else { return 0 };
        let tris = mesh(c, &f.ctrl.doors_open);
        if !out.is_null() {
            let out = unsafe { std::slice::from_raw_parts_mut(out, max as usize) };
            for (o, (p, n, uv, look)) in out.iter_mut().zip(&tris) {
                *o = FaithCourseVertex { pos: f.host(*p).to_array(), normal: (f.frame.axes * *n).to_array(), uv: uv.to_array(), look: *look };
            }
        }
        tris.len() as u32
    })
}

/// The ground under a host point: the first solid below it (looking from 0.5 m above, up to
/// `drop` host units down) in the world Faith moves in now (a course's own while on one), so
/// the host can stand its people on what only Faith collides with. 0 if there's nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_ground_below(h: *mut Faith, at: FaithVec3, drop: f32, out: *mut FaithVec3) -> u8 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    if out.is_null() {
        return 0;
    }
    guard(0, || {
        let from = f.local_point(at.into()) + Vec3::Y * 0.5;
        let down = drop.max(0.0) / f.frame.units_per_meter + 0.5;
        match f.world.sweep(Vec3::new(0.05, 0.01, 0.05), from, Vec3::new(0.0, -down, 0.0)) {
            Some(hit) => {
                unsafe { *out = f.host(Vec3::new(from.x, hit.point.y, from.z)).into() };
                1
            }
            None => 0,
        }
    })
}
