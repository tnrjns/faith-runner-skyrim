//! The C interface in the host's frame (Skyrim's: Z up, 70 units a metre).

use super::*;
use super::course::*;

const U: f32 = 70.0;

/// A box as 12 triangles (host frame).
fn cube(lo: [f32; 3], hi: [f32; 3]) -> Vec<f32> {
    let c = |i: usize| [if i & 1 != 0 { hi[0] } else { lo[0] }, if i & 2 != 0 { hi[1] } else { lo[1] }, if i & 4 != 0 { hi[2] } else { lo[2] }];
    let quads = [[0, 1, 3, 2], [4, 5, 7, 6], [0, 1, 5, 4], [2, 3, 7, 6], [0, 2, 6, 4], [1, 3, 7, 5]];
    let mut v = vec![];
    for q in quads {
        for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
            for i in t {
                v.extend(c(i));
            }
        }
    }
    v
}

/// Ground around (x, y), plus a rail across the way north at `rail_y` (0 for none).
fn world(h: *mut Faith, rail_y: f32) {
    let mut tris = cube([-5000.0, -5000.0, -100.0], [5000.0, 5000.0, 0.0]);
    if rail_y != 0.0 {
        // Waist high (1 m), 15 cm deep, 10 m wide.
        tris.extend(cube([-5.0 * U, rail_y, 0.0], [5.0 * U, rail_y + 0.15 * U, 1.0 * U]));
    }
    unsafe { faith_set_world(h, tris.as_ptr(), (tris.len() / 9) as u32) };
}

fn new() -> *mut Faith {
    // No Mirror's Edge: just the movement (this is about the frame, not the animation).
    let h = unsafe { faith_create(c"Z:\\nowhere".as_ptr(), U) };
    assert!(!h.is_null());
    h
}

fn run(h: *mut Faith, secs: f32, mut input: impl FnMut(&FaithFrame) -> FaithInput) -> (FaithFrame, u64) {
    let mut f = FaithFrame::default();
    let mut ev = 0;
    let mut t = 0.0;
    while t < secs {
        let i = input(&f);
        unsafe { faith_step(h, 1.0 / 60.0, &i, &mut f) };
        ev |= f.events;
        t += 1.0 / 60.0;
    }
    (f, ev)
}

#[test]
fn runs_north_at_heading_zero_and_east_at_ninety() {
    let h = new();
    world(h, 0.0);
    for (heading, dir) in [(0.0f32, Vec3::Y), (std::f32::consts::FRAC_PI_2, Vec3::X)] {
        unsafe { faith_teleport(h, FaithVec3 { x: 100.0, y: 200.0, z: 0.0 }, heading) };
        let (f, _) = run(h, 1.5, |_| FaithInput { move_y: 1.0, ..Default::default() });
        let moved = Vec3::from(f.feet) - Vec3::new(100.0, 200.0, 0.0);
        assert!(moved.normalize().dot(dir) > 0.99 && moved.length() > 3.0 * U, "heading {heading}: moved {moved}");
        assert!(f.feet.z.abs() < 1.0, "stays on the ground: {}", f.feet.z);
        assert!((f.heading - heading).abs() < 1e-3, "heading {} vs {heading}", f.heading);
        let fwd = Vec3::from(f.cam_forward);
        assert!(fwd.dot(dir) > 0.9, "camera looks {fwd}");
        assert!(Vec3::from(f.cam_up).z > 0.9);
        // Right is east of north.
        assert!(Vec3::from(f.cam_right).dot(dir.cross(Vec3::Z)) > 0.9);
        assert!(f.cam_pos.z > 1.4 * U && f.cam_pos.z < 1.9 * U, "eye height {}", f.cam_pos.z);
        assert_eq!(f.on_ground, 1);
        // Mirror's Edge's speed blur: running flat out straight ahead, about TdMotionBlurAmount.
        assert!(f.speed_blur > 0.1 && f.speed_blur <= 0.5, "speed blur {}", f.speed_blur);
    }
    unsafe { faith_destroy(h) };
}

/// Skyrim's world goes far below zero (Riverwood is near z -2900): no floor to die on there.
#[test]
fn runs_far_below_zero() {
    let h = new();
    let z = -2900.0;
    let tris = cube([-5000.0, -5000.0, z - 100.0], [5000.0, 5000.0, z]);
    unsafe { faith_set_world(h, tris.as_ptr(), (tris.len() / 9) as u32) };
    unsafe { faith_teleport(h, FaithVec3 { x: 0.0, y: 0.0, z }, 0.0) };
    let (f, ev) = run(h, 1.5, |_| FaithInput { move_y: 1.0, ..Default::default() });
    assert!(ev & events::DEATH == 0, "died");
    assert!(f.feet.y > 3.0 * U, "ran {}", f.feet.y);
    assert!((f.feet.z - z).abs() < 1.0 && f.on_ground == 1, "on the ground at {}", f.feet.z);
    unsafe { faith_destroy(h) };
}

/// Far out in Skyrim's world (2.5 km from its origin, where a float in metres only resolves a
/// quarter millimetre) Faith runs 300 m on flat ground without sinking: her own origin follows
/// her (re-centred every 200 m) so her collision stays precise.
#[test]
fn runs_far_from_the_origin() {
    let h = new();
    let (x, y, z) = (172_725.0f32, -97_930.0f32, 11_147.0f32);
    let tris = cube([x - 30_000.0, y - 30_000.0, z - 100.0], [x + 30_000.0, y + 30_000.0, z]);
    unsafe { faith_set_world(h, tris.as_ptr(), (tris.len() / 9) as u32) };
    unsafe { faith_teleport(h, FaithVec3 { x, y, z }, 0.3) };
    let mut lowest = f32::MAX;
    let (f, ev) = run(h, 45.0, |f| {
        if f.feet.x != 0.0 {
            lowest = lowest.min(f.feet.z);
        }
        FaithInput { move_y: 1.0, ..Default::default() }
    });
    let moved = Vec3::new(f.feet.x - x, f.feet.y - y, 0.0).length();
    assert!(moved > 300.0 * U, "ran {} m", moved / U);
    assert!(ev & events::DEATH == 0);
    assert!(lowest > z - 1.0 && (f.feet.z - z).abs() < 1.0 && f.on_ground == 1, "feet down to {lowest} (ground {z})");
    let o = unsafe { &*h }.origin;
    assert!(((o[0] - f.feet.x as f64).powi(2) + (o[1] - f.feet.y as f64).powi(2)).sqrt() < 210.0 * U as f64, "origin followed: {o:?}");
    unsafe { faith_destroy(h) };
}

#[test]
fn looking_right_turns_clockwise() {
    let h = new();
    world(h, 0.0);
    unsafe { faith_teleport(h, FaithVec3::default(), 0.0) };
    let (f, _) = run(h, 0.2, |_| FaithInput { look_right: 0.02, ..Default::default() });
    assert!(f.heading > 0.1, "heading {}", f.heading);
    unsafe { faith_destroy(h) };
}

#[test]
fn vaults_a_rail() {
    let h = new();
    let rail = 6.0 * U;
    world(h, rail);
    unsafe { faith_teleport(h, FaithVec3::default(), 0.0) };
    let (f, ev) = run(h, 2.5, |f| FaithInput { move_y: 1.0, jump_pressed: (f.feet.y > rail - 1.6 * U && f.feet.y < rail - 0.5 * U) as u8, ..Default::default() });
    assert!(ev & events::VAULT != 0, "events {ev:#x}");
    assert!(f.feet.y > rail + 1.0 * U, "over the rail: {}", f.feet.y);
    unsafe { faith_destroy(h) };
}

#[test]
fn null_and_bad_input_is_harmless() {
    unsafe {
        faith_step(std::ptr::null_mut(), 0.016, std::ptr::null(), std::ptr::null_mut());
        faith_set_world(std::ptr::null_mut(), std::ptr::null(), 3);
        let h = new();
        faith_set_world(h, std::ptr::null(), 0);
        assert_eq!(faith_bind_skeleton(h, 0, 0, std::ptr::null(), std::ptr::null(), std::ptr::null()), -1);
        let mut f = FaithFrame::default();
        faith_step(h, 0.016, std::ptr::null(), &mut f);
        faith_destroy(h);
    }
}

/// With Mirror's Edge (ME_INSTALL): the body animates and a Skyrim-named skeleton binds and
/// poses.
#[test]
fn animates_and_poses_a_skeleton() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let dir = CString::new(dir.to_string_lossy().into_owned()).unwrap();
    let h = unsafe { faith_create(dir.as_ptr(), U) };
    assert_eq!(unsafe { faith_animated(h) }, 1, "{:?}", unsafe { CStr::from_ptr(faith_last_error()) });
    world(h, 0.0);
    unsafe { faith_teleport(h, FaithVec3::default(), 0.0) };
    // A small arm: spine, clavicle, upper arm, forearm, hand (rest frames: identity, lengths
    // along the arm).
    let names = ["NPC Spine2 [Spn2]", "NPC L Clavicle [LClv]", "NPC L UpperArm [LUar]", "NPC L Forearm [LLar]", "NPC L Hand [LHnd]"];
    let cnames: Vec<CString> = names.iter().map(|n| CString::new(*n).unwrap()).collect();
    let ptrs: Vec<*const c_char> = cnames.iter().map(|c| c.as_ptr()).collect();
    let parents = [-1, 0, 1, 2, 3];
    let rest: Vec<FaithXform> = [[0.0, 0.0, 90.0], [-3.0, 0.0, 10.0], [-12.0, 0.0, 0.0], [-20.0, 0.0, 0.0], [-18.0, 0.0, 0.0]]
        .iter()
        .map(|p| FaithXform { rot: [0.0, 0.0, 0.0, 1.0], pos: *p, scale: 1.0 })
        .collect();
    let id = unsafe { faith_bind_skeleton(h, 1, 5, ptrs.as_ptr(), parents.as_ptr(), rest.as_ptr()) };
    assert!(id >= 0, "{:?}", unsafe { CStr::from_ptr(faith_last_error()) });
    assert_eq!(unsafe { faith_skeleton_mapped(h, id) }, 5);
    let (f, _) = run(h, 1.0, |_| FaithInput { move_y: 1.0, ..Default::default() });
    assert_eq!(f.animated, 1);
    let name = unsafe { CStr::from_ptr(faith_anim_name(h)) }.to_string_lossy().into_owned();
    assert!(!name.is_empty());
    let mut out = vec![FaithXform::default(); 5];
    let root = FaithXform { rot: [0.0, 0.0, 0.0, 1.0], pos: [f.feet.x, f.feet.y, f.feet.z], scale: 1.0 };
    assert_eq!(unsafe { faith_pose_skeleton(h, id, root, out.as_mut_ptr()) }, 1);
    for o in &out {
        assert!(o.rot.iter().chain(&o.pos).all(|v| v.is_finite()));
        assert!((Quat::from_array(o.rot).length() - 1.0).abs() < 1e-3);
    }

    // Her own body: arms and torso, and legs, skinned in front of the camera.
    use crate::body::*;
    assert_eq!(unsafe { faith_body_parts(h) }, 2);
    for part in 0..2 {
        let mut info = FaithPartInfo::default();
        assert_eq!(unsafe { faith_body_part(h, part, &mut info) }, 1);
        assert!(info.vertex_count > 1000 && info.index_count % 3 == 0 && info.section_count > 0, "part {part}");
        let idx = unsafe { std::slice::from_raw_parts(faith_body_indices(h, part), info.index_count as usize) };
        assert!(idx.iter().all(|&i| i < info.vertex_count));
        let secs = unsafe { std::slice::from_raw_parts(faith_body_sections(h, part), info.section_count as usize) };
        assert_eq!(secs.iter().map(|s| s.index_count).sum::<u32>(), info.index_count);
        let (mut w, mut hh) = (0, 0);
        let tex = unsafe { faith_body_texture(h, part, secs[0].material, 0, &mut w, &mut hh) };
        assert!(!tex.is_null() && w >= 256 && hh >= 256, "part {part} colour texture {w}x{hh}");
        let mut verts = vec![FaithVertex::default(); info.vertex_count as usize];
        assert_eq!(unsafe { faith_body_skin(h, part, verts.as_mut_ptr()) }, 1);
        let mut centre = Vec3::ZERO;
        for v in &verts {
            assert!(v.pos.iter().chain(&v.normal).all(|x| x.is_finite()));
            centre += Vec3::from(v.pos);
        }
        centre /= verts.len() as f32;
        // Looking ahead, both parts are below the eye; the arms ahead of it, within reach.
        assert!(centre.y < 0.0 && centre.length() < 1.5 * U, "part {part} centre {centre}");
        if part == 0 {
            assert!(centre.z < 0.0, "arms in front of the camera: {centre}");
        }
    }
    unsafe { faith_destroy(h) };
}

/// faith.h's static_asserts.
#[test]
fn layout_matches_the_header() {
    assert_eq!(std::mem::size_of::<FaithVec3>(), 12);
    assert_eq!(std::mem::size_of::<FaithInput>(), 24);
    assert_eq!(std::mem::size_of::<FaithFrame>(), 112);
    assert_eq!(std::mem::size_of::<FaithXform>(), 32);
    assert_eq!(std::mem::size_of::<FaithCourseVertex>(), 36);
    assert_eq!(std::mem::size_of::<FaithCourse>(), 28);
    assert_eq!(std::mem::size_of::<crate::body::FaithVertex>(), 48);
    assert_eq!(std::mem::size_of::<crate::body::FaithSection>(), 12);
    assert_eq!(std::mem::size_of::<crate::body::FaithPartInfo>(), 20);
    assert_eq!(std::mem::offset_of!(FaithFrame, events), 96);
    assert_eq!(std::mem::offset_of!(FaithFrame, speed_blur), 108);
}

/// The Moves map far out in the host's world: she stands on its first roof, springboards up to
/// the second (its checkpoint reached), and leaving puts the host's world back.
#[test]
fn plays_the_moves_course() {
    use faith_move::moves::{z, M2_Y};
    let h = new();
    world(h, 0.0);
    let anchor = FaithVec3 { x: 150_000.0, y: -90_000.0, z: 20_000.0 };
    assert_eq!(unsafe { faith_course_start(h, 0, anchor) }, 1);
    let (f, _) = run(h, 0.5, |_| FaithInput::default());
    assert!(f.on_ground != 0 && (f.feet.z - anchor.z).abs() < 1.0, "on M1's roof: {:?}", Vec3::from(f.feet));
    let n = unsafe { faith_course_mesh(h, std::ptr::null_mut(), 0) };
    let mut verts = vec![course::FaithCourseVertex::default(); n as usize];
    assert_eq!(unsafe { faith_course_mesh(h, verts.as_mut_ptr(), n) }, n);
    assert!(n > 300 && n % 3 == 0 && verts.iter().all(|v| Vec3::from(v.pos).is_finite()));
    assert!(verts.windows(2).all(|w| w[0].look <= w[1].look), "grouped by look");

    let local = |h: *mut Faith| unsafe { handle(h) }.unwrap().ctrl.feet;
    let mut jumped = false;
    let mut t = 0.0;
    let mut f = FaithFrame::default();
    while t < 8.0 {
        let c = local(h);
        let stop = c.z < z::M2_START - 2.0 && f.on_ground != 0;
        let jump = !jumped && c.z < z::STEP_START + 3.0;
        jumped |= jump;
        let i = FaithInput { move_y: if stop { 0.0 } else { 1.0 }, jump_pressed: jump as u8, ..Default::default() };
        unsafe { faith_step(h, 1.0 / 60.0, &i, &mut f) };
        t += 1.0 / 60.0;
    }
    let mut s = FaithCourse::default();
    assert_eq!(unsafe { faith_course_status(h, &mut s) }, 1);
    let c = local(h);
    assert!(s.checkpoint == 1 && (c.y - M2_Y).abs() < 0.1, "checkpoint {} at {c}", s.checkpoint);
    let msg = unsafe { std::ffi::CStr::from_ptr(faith_course_message(h)) }.to_str().unwrap();
    assert_eq!(msg, "Checkpoint - M2 Balance");

    unsafe { faith_teleport(h, FaithVec3 { x: 0.0, y: 0.0, z: 0.0 }, 0.0) };
    assert_eq!(unsafe { faith_course_status(h, &mut s) }, 0);
    let (f, _) = run(h, 0.5, |_| FaithInput::default());
    assert!(f.on_ground != 0 && f.feet.z.abs() < 1.0, "back on the host's ground: {:?}", Vec3::from(f.feet));
    unsafe { faith_destroy(h) };
}
