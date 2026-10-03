//! The disarm's victim side: as Faith plays a takedown, the person she takes down plays the
//! clip Mirror's Edge's enemy plays with it (TdAIController.TriggerCannedAnim, the same name as
//! hers), the patrol cop's, retargeted onto the host's skeleton.

use std::ffi::c_char;

use glam::{Quat, Vec3};
use faith_anim::retarget::{self, Placement, Retarget};
use me_assets::FaithArms;

use crate::{bones_from, guard, handle, recentred, set_error, Faith, FaithVec3, FaithXform};

#[derive(Default)]
pub struct Victims {
    /// The cop (loaded on the first bind), or None if it couldn't be.
    cop: Option<FaithArms>,
    tried: bool,
    skeletons: Vec<Retarget>,
}

/// Bind a host skeleton (as faith_bind_skeleton) to the disarm's victim. Returns its id for
/// faith_pose_victim, or -1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_bind_victim(h: *mut Faith, count: u32, names: *const *const c_char, parents: *const i32, rest: *const FaithXform) -> i32 {
    let Some(f) = (unsafe { handle(h) }) else { return -1 };
    guard(-1, || {
        if !f.victims.tried {
            f.victims.tried = true;
            let Some((arms, _)) = &f.anim else {
                set_error("no Mirror's Edge animation loaded");
                return -1;
            };
            use me_assets::*;
            match FaithArms::load_character(&arms.cooked_pc, VICTIM_PACKAGE, VICTIM_MESH, VICTIM_ANIMS, VICTIM_SET) {
                Ok(cop) => f.victims.cop = Some(cop),
                Err(e) => set_error(format!("the disarm's victim: {e}")),
            }
        }
        let Some(cop) = &f.victims.cop else { return -1 };
        let Some(bones) = (unsafe { bones_from(count, names, parents, rest) }) else { return -1 };
        let r = Retarget::new(bones, f.frame, cop, &retarget::skyrim_npc_links(), &retarget::skyrim_npc_anchors());
        if r.mapped() == 0 {
            set_error("no bones matched");
            return -1;
        }
        f.victims.skeletons.push(r);
        (f.victims.skeletons.len() - 1) as i32
    })
}

/// The victim's pose for takedown `anim` (FaithTakedown.anim) `time` seconds in, standing with
/// its feet at `feet` facing `heading` (host frame, as FaithFrame.heading): local transforms for
/// all its bones into `out`, as faith_pose_skeleton. Returns 1 if posed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_pose_victim(
    h: *mut Faith,
    skeleton: i32,
    anim: u32,
    time: f32,
    feet: FaithVec3,
    heading: f32,
    root_parent: FaithXform,
    out: *mut FaithXform,
) -> u8 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        let (Some(cop), Some(r)) = (&f.victims.cop, f.victims.skeletons.get(skeleton as usize)) else { return 0 };
        if out.is_null() {
            return 0;
        }
        let seq = faith_move::TAKEDOWN_ANIMS[(anim as usize).min(3)];
        let mut me = vec![];
        if !retarget::clip_globals(cop, seq, time, &mut me) {
            return 0;
        }
        let d = f.frame.point_back(Vec3::new(heading.sin(), heading.cos(), 0.0));
        let turn = Quat::from_rotation_y((-d.x).atan2(-d.z));
        let place = Placement { origin: f.local_point(feet.into()), body_rot: turn, legs_rot: turn };
        let mut local = vec![];
        r.pose(&me, &place, recentred(f, root_parent), &mut local);
        let out = unsafe { std::slice::from_raw_parts_mut(out, local.len()) };
        for (o, l) in out.iter_mut().zip(&local) {
            *o = (*l).into();
        }
        1
    })
}
