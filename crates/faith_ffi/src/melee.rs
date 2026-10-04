//! Faith's attacks on the host's actors: the host says who's around (faith_set_targets), and
//! after each step reads what landed (faith_melee_hits). The choosing, hit tests and damage are
//! Mirror's Edge's (faith_move::melee).

use glam::Vec3;

use crate::{guard, handle, Faith, FaithVec3};

/// Someone Faith can hit, host frame and units.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithTarget {
    /// The host's handle for it (given back in FaithHit).
    pub id: u32,
    /// The middle of its collision cylinder (or capsule).
    pub centre: FaithVec3,
    pub radius: f32,
    pub half_height: f32,
    /// Its eyes above the centre.
    pub eye: f32,
    /// Which way it faces.
    pub facing: FaithVec3,
}

/// A takedown (faith_takedowns): started (`done` 0) or finished (`done` 1). The target stays at
/// `enemy_at` facing `enemy_dir` (as it stood); its side of it (faith_pose_victim) is placed at
/// `clip_at` (her spot, feet) facing `clip_dir`. Host frame. `anim` 0-2 a front snatch, 3 from
/// behind.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithTakedown {
    pub target: u32,
    pub anim: u32,
    pub done: u32,
    pub enemy_at: FaithVec3,
    pub enemy_dir: FaithVec3,
    pub clip_at: FaithVec3,
    pub clip_dir: FaithVec3,
}

/// An attack that landed: Mirror's Edge's damage (its hit points), and the momentum the blow
/// carries (host units/s, host frame). `kind`: 0 punch, 1 air kick, 2 slide kick, 3 wallrun
/// kick, 4 crouch attack.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithHit {
    pub target: u32,
    pub damage: f32,
    pub momentum: FaithVec3,
    pub kind: u32,
}

/// Who's around Faith now (replaces the last list; call each frame before faith_step).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_set_targets(h: *mut Faith, targets: *const FaithTarget, count: u32) {
    let Some(f) = (unsafe { handle(h) }) else { return };
    guard((), || {
        let list: &[FaithTarget] = if targets.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(targets, count as usize) } };
        let k = 1.0 / f.frame.units_per_meter;
        f.ctrl.targets = list
            .iter()
            .filter(|t| Vec3::from(t.centre).is_finite())
            .map(|t| faith_move::Target {
                id: t.id,
                centre: f.local_point(t.centre.into()),
                radius: t.radius * k,
                half_height: t.half_height * k,
                eye: t.eye * k,
                facing: {
                    let d = f.frame.point_back(t.facing.into());
                    Vec3::new(d.x, 0.0, d.z).normalize_or_zero()
                },
            })
            .collect();
    });
}

/// The attacks that landed in the last faith_step, up to `max` into `out`. Returns how many.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_melee_hits(h: *mut Faith, out: *mut FaithHit, max: u32) -> u32 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        if out.is_null() {
            return f.hits.len() as u32;
        }
        let out = unsafe { std::slice::from_raw_parts_mut(out, max as usize) };
        let n = f.hits.len().min(out.len());
        out[..n].copy_from_slice(&f.hits[..n]);
        n as u32
    })
}

/// The takedowns started or finished in the last faith_step, up to `max` into `out`. Returns how
/// many.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_takedowns(h: *mut Faith, out: *mut FaithTakedown, max: u32) -> u32 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        if out.is_null() {
            return f.takedowns.len() as u32;
        }
        let out = unsafe { std::slice::from_raw_parts_mut(out, max as usize) };
        let n = f.takedowns.len().min(out.len());
        out[..n].copy_from_slice(&f.takedowns[..n]);
        n as u32
    })
}
