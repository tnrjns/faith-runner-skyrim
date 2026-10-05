//! The C interface in the host's frame (Skyrim's: Z up, 70 units a metre).

use super::*;
use super::course::*;
use super::moving::*;

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
    assert_eq!(std::mem::size_of::<FaithFrame>(), 120);
    assert_eq!(std::mem::size_of::<FaithXform>(), 32);
    assert_eq!(std::mem::size_of::<crate::host_fixtures::FaithHostFixture>(), 48);
    assert_eq!(std::mem::size_of::<FaithCourseVertex>(), 36);
    assert_eq!(std::mem::size_of::<FaithCourse>(), 28);
    assert_eq!(std::mem::size_of::<crate::body::FaithVertex>(), 48);
    assert_eq!(std::mem::size_of::<crate::body::FaithSection>(), 12);
    assert_eq!(std::mem::size_of::<crate::body::FaithPartInfo>(), 20);
    assert_eq!(std::mem::offset_of!(FaithFrame, events), 96);
    assert_eq!(std::mem::offset_of!(FaithFrame, speed_blur), 108);
    assert_eq!(std::mem::offset_of!(FaithFrame, game_speed), 116);
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

/// Someone 1.2 m north: a punch lands on them (Mirror's Edge's 33.5), knocking them north; and
/// someone 3 m off is missed.
#[test]
fn punches_land_on_the_hosts_actors() {
    use super::melee::*;
    assert_eq!(std::mem::size_of::<FaithTarget>(), 40);
    assert_eq!(std::mem::size_of::<FaithHit>(), 24);
    for (dist, lands) in [(1.2, true), (3.0, false)] {
        let h = new();
        world(h, 0.0);
        unsafe { faith_teleport(h, FaithVec3::default(), 0.0) };
        run(h, 0.3, |_| FaithInput::default());
        let t = FaithTarget { id: 42, centre: FaithVec3 { x: 0.0, y: dist * U, z: 0.9 * U }, radius: 0.3 * U, half_height: 0.9 * U, eye: 0.6 * U, ..Default::default() };
        unsafe { faith_set_targets(h, &t, 1) };
        let mut got = vec![];
        let mut pressed = false;
        run(h, 1.2, |_| {
            let p = !pressed;
            pressed = true;
            let mut out = [FaithHit::default(); 4];
            let n = unsafe { faith_melee_hits(h, out.as_mut_ptr(), 4) } as usize;
            got.extend_from_slice(&out[..n]);
            FaithInput { melee_pressed: p as u8, ..Default::default() }
        });
        let mut out = [FaithHit::default(); 4];
        let n = unsafe { faith_melee_hits(h, out.as_mut_ptr(), 4) } as usize;
        got.extend_from_slice(&out[..n]);
        if lands {
            assert_eq!(got.len(), 1, "{dist} m");
            assert_eq!((got[0].target, got[0].damage, got[0].kind), (42, 33.5, 0));
            assert!(got[0].momentum.y > 0.0 && got[0].momentum.x.abs() < 1.0, "knocked north");
        } else {
            assert!(got.is_empty(), "{dist} m: missed");
        }
        unsafe { faith_destroy(h) };
    }
}

/// A cable in the host's collision becomes a zipline (host frame back out).
#[test]
fn finds_a_zipline_in_the_hosts_collision() {
    use super::world_fixtures::*;
    assert_eq!(std::mem::size_of::<FaithFixtureCandidate>(), 32);
    assert_eq!(std::mem::size_of::<FaithFixture>(), 28);
    let h = new();
    let cable = FaithFixtureCandidate {
        a: FaithVec3 { x: 0.0, y: 0.0, z: 2.8 * U },
        b: FaithVec3 { x: 0.0, y: 15.0 * U, z: 0.8 * U },
        thickness: 0.02 * U,
        capsule: 1,
    };
    unsafe { faith_set_fixture_candidates(h, &cable, 1) };
    world(h, 0.0);
    let mut out = [FaithFixture::default(); 4];
    let n = unsafe { faith_world_fixtures(h, out.as_mut_ptr(), 4) } as usize;
    assert_eq!(n, 1);
    assert_eq!(out[0].kind, 0);
    assert!((out[0].a.z - 2.8 * U).abs() < 0.5 && (out[0].b.y - 15.0 * U).abs() < 0.5);
    unsafe { faith_destroy(h) };
}

/// A gate closing in front of her stops her the frame it's given.
#[test]
fn moving_collision_counts_the_frame_its_given() {
    let h = new();
    world(h, 0.0);
    unsafe { faith_teleport(h, FaithVec3::default(), 0.0) };
    run(h, 0.3, |_| FaithInput::default());
    // A gate 2 m north, 3 m wide and high.
    let gate = cube([-1.5 * U, 2.0 * U, 0.0], [1.5 * U, 2.2 * U, 3.0 * U]);
    unsafe { faith_set_moving(h, gate.as_ptr(), (gate.len() / 9) as u32) };
    let (f, _) = run(h, 2.0, |_| FaithInput { move_y: 1.0, ..Default::default() });
    assert!(f.feet.y < 2.0 * U, "stopped by the gate: {}", f.feet.y);
    // Opened (gone): she goes through.
    unsafe { faith_set_moving(h, std::ptr::null(), 0) };
    let (f, _) = run(h, 2.0, |_| FaithInput { move_y: 1.0, ..Default::default() });
    assert!(f.feet.y > 3.0 * U, "through the open gate: {}", f.feet.y);
    unsafe { faith_destroy(h) };
}

/// How long taking a host's collision takes (run with --ignored --nocapture).
#[test]
#[ignore]
fn set_world_timing() {
    let h = new();
    let mut seed = 7u32;
    let mut r = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut tris = vec![];
    for _ in 0..2400 {
        let c = [r() * 4800.0 - 2400.0, r() * 4800.0 - 2400.0, r() * 1400.0];
        tris.extend(cube([c[0], c[1], c[2]], [c[0] + r() * 300.0, c[1] + r() * 300.0, c[2] + r() * 300.0]));
    }
    let n = (tris.len() / 9) as u32;
    let t = std::time::Instant::now();
    unsafe { faith_set_world(h, tris.as_ptr(), n) };
    eprintln!("{n} triangles: faith_set_world {:?}", t.elapsed());
    unsafe { faith_destroy(h) };
}

/// A takedown on someone 1.2 m north: facing her, a front snatch; facing away, the one from
/// behind. Either way their clip is placed facing her. Each ends when her clip does.
#[test]
fn takedowns_from_the_front_and_behind() {
    use super::melee::*;
    assert_eq!(std::mem::size_of::<FaithTakedown>(), 60);
    for (facing_y, back) in [(-1.0, false), (1.0, true)] {
        let h = new();
        world(h, 0.0);
        unsafe { faith_teleport(h, FaithVec3::default(), 0.0) };
        run(h, 0.3, |_| FaithInput::default());
        let t = FaithTarget {
            id: 7,
            centre: FaithVec3 { x: 0.0, y: 1.2 * U, z: 0.9 * U },
            radius: 0.3 * U,
            half_height: 0.9 * U,
            eye: 0.6 * U,
            facing: FaithVec3 { x: 0.0, y: facing_y, z: 0.0 },
        };
        unsafe { faith_set_targets(h, &t, 1) };
        let mut got = vec![];
        let mut pressed = false;
        run(h, 3.0, |_| {
            let p = !pressed;
            pressed = true;
            let mut out = [FaithTakedown::default(); 4];
            let n = unsafe { faith_takedowns(h, out.as_mut_ptr(), 4) } as usize;
            got.extend_from_slice(&out[..n]);
            FaithInput { takedown_pressed: p as u8, ..Default::default() }
        });
        assert_eq!(got.len(), 2, "started and ended");
        let (s, e) = (got[0], got[1]);
        assert!(s.target == 7 && s.done == 0 && e.target == 7 && e.done == 1);
        assert_eq!(s.anim == 3, back, "anim {}", s.anim);
        // They keep facing as they stood; their side of it is laid out from her spot, at them
        // (north), and her spot is DisarmOffset in front of them (or behind: the same place
        // here, as they stand facing her or away from her).
        assert!((s.enemy_dir.y - facing_y).abs() < 0.1, "they face as they stood: {:?}", Vec3::from(s.enemy_dir));
        assert!((s.clip_dir.y - 1.0).abs() < 0.1, "the clip points at them: {:?}", Vec3::from(s.clip_dir));
        assert!(s.clip_at.y.abs() < 0.1 * U, "her spot 1.26 m short of them: {:?}", Vec3::from(s.clip_at));
        unsafe { faith_destroy(h) };
    }
}

/// The ground under a point, in Faith's world: the floor's top (z 0), and nothing past the drop.
#[test]
fn ground_below_finds_the_floor() {
    use super::course::faith_ground_below;
    let h = new();
    world(h, 0.0);
    let mut out = FaithVec3::default();
    let at = FaithVec3 { x: 3.0 * U, y: -2.0 * U, z: 2.0 * U };
    assert_eq!(unsafe { faith_ground_below(h, at, 5.0 * U, &mut out) }, 1);
    assert!(out.z.abs() < 0.05 * U && (out.x - at.x).abs() < 1.0 && (out.y - at.y).abs() < 1.0, "{:?}", Vec3::from(out));
    assert_eq!(unsafe { faith_ground_below(h, at, 1.0 * U, &mut out) }, 0, "the floor is 2 m down");
    unsafe { faith_destroy(h) };
}

/// With Mirror's Edge: a Skyrim-named arm binds to the disarm's victim (the cop) and poses
/// through its clips; the bound arm's hand moves over the takedown.
#[test]
fn the_victim_binds_and_poses() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    use super::victim::*;
    let dir = CString::new(dir.to_string_lossy().into_owned()).unwrap();
    let h = unsafe { faith_create(dir.as_ptr(), U) };
    world(h, 0.0);
    let names = ["NPC Spine2 [Spn2]", "NPC L Clavicle [LClv]", "NPC L UpperArm [LUar]", "NPC L Forearm [LLar]", "NPC L Hand [LHnd]"];
    let cnames: Vec<CString> = names.iter().map(|n| CString::new(*n).unwrap()).collect();
    let ptrs: Vec<*const c_char> = cnames.iter().map(|c| c.as_ptr()).collect();
    let parents = [-1, 0, 1, 2, 3];
    let rest: Vec<FaithXform> = [[0.0, 0.0, 90.0], [-3.0, 0.0, 10.0], [-12.0, 0.0, 0.0], [-20.0, 0.0, 0.0], [-18.0, 0.0, 0.0]]
        .iter()
        .map(|p| FaithXform { rot: [0.0, 0.0, 0.0, 1.0], pos: *p, scale: 1.0 })
        .collect();
    let id = unsafe { faith_bind_victim(h, 5, ptrs.as_ptr(), parents.as_ptr(), rest.as_ptr()) };
    assert!(id >= 0, "{:?}", unsafe { CStr::from_ptr(faith_last_error()) });
    let root = FaithXform { rot: [0.0, 0.0, 0.0, 1.0], pos: [0.0, 0.0, 0.0], scale: 1.0 };
    let mut rots = vec![];
    for anim in 0..4 {
        for t in [0.2, 1.0] {
            let mut out = vec![FaithXform::default(); 5];
            assert_eq!(unsafe { faith_pose_victim(h, id, anim, t, FaithVec3::default(), 1.0, root, out.as_mut_ptr()) }, 1);
            assert!(out.iter().all(|o| o.rot.iter().chain(&o.pos).all(|v| v.is_finite())));
            rots.push(Quat::from_array(out[3].rot));
        }
    }
    assert!(rots.windows(2).any(|w| w[0].angle_between(w[1]) > 0.1), "the forearm moves");
    unsafe { faith_destroy(h) };
}

/// How long a frame of Faith's own work takes (movement, animation, sound) running a training
/// course with Mirror's Edge's animations: run with --ignored --nocapture.
#[test]
#[ignore]
fn frame_time() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let dir = CString::new(dir.to_string_lossy().into_owned()).unwrap();
    let h = unsafe { faith_create(dir.as_ptr(), U) };
    assert_eq!(unsafe { faith_animated(h) }, 1);
    let anchor = FaithVec3 { x: 0.0, y: 0.0, z: 20000.0 };
    assert_eq!(unsafe { faith_course_start(h, 3, anchor) }, 1);
    let mut times = vec![];
    let mut f = FaithFrame::default();
    for i in 0..1800 {
        let input = FaithInput { move_y: 1.0, jump_pressed: (i % 90 == 0) as u8, melee_pressed: (i % 200 == 100) as u8, ..Default::default() };
        let t = std::time::Instant::now();
        unsafe { faith_step(h, 1.0 / 60.0, &input, &mut f) };
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        if ms > 1.0 {
            eprintln!("frame {i}: {ms:.2} ms, events {:#x}, ground {}", f.events, f.on_ground);
        }
        times.push(ms);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    eprintln!("faith_step: mean {mean:.3} ms, median {:.3}, 99% {:.3}, max {:.3}", times[900], times[1782], times[1799]);
    unsafe { faith_destroy(h) };
}
