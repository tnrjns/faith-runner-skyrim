//! Retargeting checks (the `retarget` feature) against Faith's real skeleton and animations. Need ME_INSTALL (skip
//! otherwise).

use glam::{Mat4, Quat, Vec2, Vec3};
use me_assets::FaithArms;
use me_assets::pose::{self, to_view, Pose};

use crate::retarget::{self, AnchorSpec, BoneRest, HostFrame, LinkSpec, Placement, Retarget, Xform};
use crate::Rig;

fn arms() -> Option<FaithArms> {
    let dir = std::env::var_os("ME_INSTALL")?;
    Some(FaithArms::load(std::path::Path::new(&dir), 4).expect("load"))
}

/// Run on the greybox for a while and return the rig's last frame and Faith's pose.
fn animated(arms: &FaithArms) -> (Vec<Mat4>, Placement) {
    use faith_move::{greybox, CameraFx, Controller, Input, Tuning};
    let level = greybox::greybox();
    let world = level.world();
    let cp = &level.checkpoints[0];
    let mut c = Controller::new(Tuning::default(), cp.spawn, cp.yaw);
    let mut fx = CameraFx::default();
    let mut rig = Rig::new(arms, c.yaw);
    let dt = 1.0 / 60.0;
    let mut frame = None;
    for i in 0..90 {
        let input = Input { move_axis: Vec2::new(0.3, 1.0), look: Vec2::new(0.004, if i < 40 { -0.01 } else { 0.0 }), ..Default::default() };
        c.step(dt, &input, &world);
        let shot = fx.update(dt, &c, &input);
        frame = Some(rig.update(dt, &c, &shot, arms));
    }
    let f = frame.unwrap();
    (rig.driver.globals.clone(), Placement { origin: f.origin, body_rot: f.body_rot, legs_rot: f.legs_rot })
}

fn worlds(r: &Retarget, local: &[Xform], root: Xform) -> Vec<Xform> {
    let mut w: Vec<Xform> = vec![];
    for (i, b) in r.bones().iter().enumerate() {
        let parent = b.parent.map_or(root, |p| w[p]);
        w.push(parent.then(&local[i]));
    }
    w
}

/// Faith onto her own skeleton (in Skyrim's frame and units) gives her own pose back.
#[test]
fn retargets_onto_its_own_skeleton() {
    let Some(arms) = arms() else { return };
    let f = HostFrame::skyrim();
    let rest = Pose::rest(&arms.mesh);
    let bones: Vec<BoneRest> = arms
        .mesh
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| BoneRest {
            name: b.name.clone(),
            parent: (i > 0).then_some(b.parent),
            rot: f.rot(rest.rot[i]),
            pos: f.point(to_view(rest.pos[i])),
            scale: 1.0,
        })
        .collect();
    let names: Vec<&'static str> = arms.mesh.bones.iter().map(|b| &*Box::leak(b.name.clone().into_boxed_str())).collect();
    let links: Vec<LinkSpec> = names.iter().map(|n| LinkSpec { host: n, me: n, aim: None, side: None }).collect();
    // The shoulders pinned to Faith's, as the first-person arms are (her clips move them).
    let anchors = [
        AnchorSpec { host: "LeftShoulder", me: "LeftShoulder", carry_body: false, scaled: false, optional: false },
        AnchorSpec { host: "RightShoulder", me: "RightShoulder", carry_body: false, scaled: false, optional: false },
    ];
    let r = Retarget::new(bones, f, &arms, &links, &anchors);
    assert_eq!(r.mapped(), arms.mesh.bones.len());

    let (me, place) = animated(&arms);
    let mut local = vec![];
    // The skeleton hangs off the world origin here.
    r.pose(&me, &place, Xform::IDENTITY, &mut local);
    let w = worlds(&r, &local, Xform::IDENTITY);
    let legs = ["Hips", "Spine", "Spine1"];
    for (i, b) in arms.mesh.bones.iter().enumerate() {
        let is_legs = legs.contains(&b.name.as_str()) || b.name.contains("Leg") || b.name.contains("Foot") || b.name.contains("Toe");
        let placed = if is_legs { place.legs_rot } else { place.body_rot };
        let want = f.rot(placed * Quat::from_mat4(&me[i]).normalize());
        let err = want.angle_between(w[i].rot).to_degrees();
        assert!(err < 0.1, "{}: rotation off by {err} degrees", b.name);
    }
    // Rotations with the shoulders pinned put the hands where Faith's are.
    for name in ["LeftHand", "RightHand", "LeftHandIndex3"] {
        let i = arms.bone(name).unwrap();
        let want = f.point(place.origin + place.body_rot * to_view(me[i].w_axis.truncate()));
        assert!((want - w[i].pos).length() < 0.5, "{name}: {} vs {want}", w[i].pos);
    }
}

/// A Skyrim-named skeleton built on Faith's rest pose: Skyrim's hierarchy (COM, a Spine2
/// between Spine1 and the neck) and bone frames turned every which way.
fn skyrim_skeleton(arms: &FaithArms, f: HostFrame) -> Vec<BoneRest> {
    skyrim_skeleton_on(arms, f, "SpineX")
}

/// The same, on any of the game's skeletons (`spine2`: the bone Skyrim's Spine2 rests like).
fn skyrim_skeleton_on(arms: &FaithArms, f: HostFrame, spine2: &str) -> Vec<BoneRest> {
    let mut g = vec![];
    pose::globals(&arms.mesh, &Pose::rest(&arms.mesh), &mut g);
    let me_pos = |n: &str| f.point(to_view(g[arms.bone(n).unwrap()].w_axis.truncate()));
    let me_rot = |n: &str| f.rot(Quat::from_mat4(&g[arms.bone(n).unwrap()]).normalize());

    // (name, parent, where, Faith's bone for its rest rotation)
    let mut spec: Vec<(String, Option<&str>, Vec3, &str)> = vec![
        ("NPC Root [Root]".into(), None, Vec3::ZERO, "root"),
        ("NPC COM [COM ]".into(), Some("NPC Root [Root]"), me_pos("Hips"), "Hips"),
        ("NPC Pelvis [Pelv]".into(), Some("NPC COM [COM ]"), me_pos("Hips"), "Hips"),
        ("NPC Spine [Spn0]".into(), Some("NPC Pelvis [Pelv]"), me_pos("Spine"), "Spine"),
        ("NPC Spine1 [Spn1]".into(), Some("NPC Spine [Spn0]"), me_pos("Spine1"), "Spine1"),
        ("NPC Spine2 [Spn2]".into(), Some("NPC Spine1 [Spn1]"), (me_pos("Spine1") + me_pos("Neck")) * 0.5, spine2),
        ("NPC Neck [Neck]".into(), Some("NPC Spine2 [Spn2]"), me_pos("Neck"), "Neck"),
        ("NPC Head [Head]".into(), Some("NPC Neck [Neck]"), me_pos("Head"), "Head"),
    ];
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    for (s, me) in [("L", "Left"), ("R", "Right")] {
        let chain = [
            ("Clavicle", "NPC Spine2 [Spn2]".to_string(), "Shoulder"),
            ("UpperArm", format!("NPC {s} Clavicle"), "Arm"),
            ("Forearm", format!("NPC {s} UpperArm"), "ForeArm"),
            ("Hand", format!("NPC {s} Forearm"), "Hand"),
            ("Thigh", "NPC Pelvis [Pelv]".to_string(), "UpLeg"),
            ("Calf", format!("NPC {s} Thigh"), "Leg"),
            ("Foot", format!("NPC {s} Calf"), "Foot"),
            ("Toe0", format!("NPC {s} Foot"), "ToeBase"),
        ];
        for (n, parent, m) in chain {
            let parent = spec.iter().find(|x| x.0.starts_with(&parent)).unwrap().0.clone();
            let me_name = leak(format!("{me}{m}"));
            spec.push((format!("NPC {s} {n}"), Some(leak(parent)), me_pos(me_name), me_name));
        }
        for (digit, name) in ["Thumb", "Index", "Middle", "Ring", "Pinky"].iter().enumerate() {
            for j in 0..3 {
                let parent = if j == 0 { format!("NPC {s} Hand") } else { format!("NPC {s} Finger{digit}{}", j - 1) };
                let me_name = leak(format!("{me}Hand{name}{}", j + 1));
                spec.push((format!("NPC {s} Finger{digit}{j}"), Some(leak(parent)), me_pos(me_name), me_name));
            }
        }
    }
    // Bone frames unlike Faith's: each turned by its own arbitrary rotation.
    let odd = |i: usize| Quat::from_euler(glam::EulerRot::XYZ, 0.7 * i as f32, 1.3 + 0.4 * i as f32, -0.5 * i as f32);
    let world: Vec<Xform> = spec.iter().enumerate().map(|(i, s)| Xform { rot: me_rot(s.3) * odd(i), pos: s.2, scale: 1.0 }).collect();
    let bones: Vec<BoneRest> = spec
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let parent = s.1.map(|p| spec.iter().position(|x| x.0 == p || x.0.starts_with(p)).unwrap());
            let pw = parent.map_or(Xform::IDENTITY, |p| world[p]);
            BoneRest {
                name: s.0.clone(),
                parent,
                rot: pw.rot.inverse() * world[i].rot,
                pos: pw.rot.inverse() * (world[i].pos - pw.pos),
                scale: 1.0,
            }
        })
        .collect();
    bones
}

/// A Skyrim-named skeleton with its own hierarchy (COM, a Spine2 between Spine1 and the neck)
/// and bone frames turned every which way still ends up posed like Faith: each limb segment
/// points the same way.
#[test]
fn retargets_onto_a_skyrim_skeleton() {
    let Some(arms) = arms() else { return };
    let f = HostFrame::skyrim();
    let bones = skyrim_skeleton(&arms, f);
    let r = Retarget::new(bones, f, &arms, &retarget::skyrim_links(), &retarget::skyrim_body_anchors());
    assert!(r.mapped() >= 40, "mapped {}", r.mapped());

    let (me, place) = animated(&arms);
    let mut local = vec![];
    let root = Xform { rot: Quat::from_rotation_z(0.3), pos: f.point(place.origin), scale: 1.0 };
    r.pose(&me, &place, root, &mut local);
    let w = worlds(&r, &local, root);
    let host = |n: &str| w[r.bones().iter().position(|b| b.name.starts_with(n)).unwrap()].pos;
    let placed_me = |n: &str, legs: bool| {
        let rot = if legs { place.legs_rot } else { place.body_rot };
        f.point(place.origin + rot * to_view(me[arms.bone(n).unwrap()].w_axis.truncate()))
    };
    for (s, m) in [("L", "Left"), ("R", "Right")] {
        let pairs = [
            ("UpperArm", "Forearm", "Arm", "ForeArm", false),
            ("Forearm", "Hand", "ForeArm", "Hand", false),
            ("Hand", "Finger20", "Hand", "HandMiddle1", false),
            ("Finger10", "Finger11", "HandIndex1", "HandIndex2", false),
            ("Thigh", "Calf", "UpLeg", "Leg", true),
            ("Calf", "Foot", "Leg", "Foot", true),
            ("Foot", "Toe0", "Foot", "ToeBase", true),
        ];
        for (ha, hb, ma, mb, legs) in pairs {
            let got = host(&format!("NPC {s} {hb}")) - host(&format!("NPC {s} {ha}"));
            let want = placed_me(&format!("{m}{mb}"), legs) - placed_me(&format!("{m}{ma}"), legs);
            let err = got.angle_between(want).to_degrees();
            assert!(err < 2.0, "{s} {ha}->{hb}: {err} degrees off");
        }
    }
    // The hips go where Faith's do (scaled to the host's legs: here the same size).
    let hips = host("NPC Pelvis") - placed_me("Hips", true);
    assert!(hips.length() < 0.5, "hips off by {hips}");
}

/// The anchors don't fight: the first-person shoulders land exactly on Faith's.
#[test]
fn shoulder_anchors_land_on_faiths() {
    let Some(arms) = arms() else { return };
    let f = HostFrame::skyrim();
    let mut g = vec![];
    pose::globals(&arms.mesh, &Pose::rest(&arms.mesh), &mut g);
    let p = |n: &str| f.point(to_view(g[arms.bone(n).unwrap()].w_axis.truncate()));
    let q = |n: &str| f.rot(Quat::from_mat4(&g[arms.bone(n).unwrap()]).normalize());
    // A short arms-only skeleton: spine, then each side's clavicle and arm, shifted 10 units.
    let off = Vec3::new(3.0, -4.0, 9.0);
    let mut bones = vec![BoneRest { name: "NPC Spine2 [Spn2]".into(), parent: None, rot: q("SpineX"), pos: p("Spine1") + off, scale: 1.0 }];
    for (s, m) in [("L", "Left"), ("R", "Right")] {
        let base = bones.len();
        let w0 = Xform { rot: q("SpineX"), pos: p("Spine1") + off, scale: 1.0 };
        let cl = Xform { rot: q(&format!("{m}Shoulder")), pos: p(&format!("{m}Shoulder")) + off, scale: 1.0 };
        let ua = Xform { rot: q(&format!("{m}Arm")), pos: p(&format!("{m}Arm")) + off, scale: 1.0 };
        let rel = |a: Xform, b: Xform| (a.rot.inverse() * b.rot, a.rot.inverse() * (b.pos - a.pos));
        let (r1, t1) = rel(w0, cl);
        let (r2, t2) = rel(cl, ua);
        bones.push(BoneRest { name: format!("NPC {s} Clavicle"), parent: Some(0), rot: r1, pos: t1, scale: 1.0 });
        bones.push(BoneRest { name: format!("NPC {s} UpperArm"), parent: Some(base), rot: r2, pos: t2, scale: 1.0 });
    }
    let links = retarget::skyrim_links();
    let anchors: Vec<AnchorSpec> = retarget::skyrim_arms_anchors();
    let r = Retarget::new(bones, f, &arms, &links, &anchors);
    let (me, place) = animated(&arms);
    let mut local = vec![];
    r.pose(&me, &place, Xform::IDENTITY, &mut local);
    let w = worlds(&r, &local, Xform::IDENTITY);
    for (i, m) in [(1, "LeftShoulder"), (3, "RightShoulder")] {
        let want = f.point(place.origin + place.body_rot * to_view(me[arms.bone(m).unwrap()].w_axis.truncate()));
        assert!((w[i].pos - want).length() < 0.01, "{m}: {} vs {want}", w[i].pos);
    }
}

/// With Faith's own body drawn, the first-person skeleton's hands are pinned onto hers (what
/// they hold goes in her grip); without asking, they aren't moved.
#[test]
fn hands_pin_onto_faiths_when_asked() {
    let Some(arms) = arms() else { return };
    let f = HostFrame::skyrim();
    let r = Retarget::new(skyrim_skeleton(&arms, f), f, &arms, &retarget::skyrim_links(), &retarget::skyrim_arms_anchors());
    let (me, place) = animated(&arms);
    let hand = |local: &[Xform], s: &str| {
        let w = worlds(&r, local, Xform::IDENTITY);
        w[r.bones().iter().position(|b| b.name == format!("NPC {s} Hand")).unwrap()].pos
    };
    let mut free = vec![];
    r.pose_with(&me, &place, Xform::IDENTITY, false, &mut free);
    let mut pinned = vec![];
    r.pose_with(&me, &place, Xform::IDENTITY, true, &mut pinned);
    for (s, m) in [("L", "LeftHand"), ("R", "RightHand")] {
        let want = f.point(place.origin + place.body_rot * to_view(me[arms.bone(m).unwrap()].w_axis.truncate()));
        assert!((hand(&pinned, s) - want).length() < 0.01, "{s}: {} vs {want}", hand(&pinned, s));
    }
    // The bones here are Faith's own lengths, so free hands are close anyway; the pin is exact.
    assert!((hand(&free, "L") - hand(&pinned, "L")).length() < 5.0);
}

/// from her camera its hands still land exactly on hers, the elbows bending her way.
#[test]
fn arm_ik_reaches_faiths_hands_with_other_arm_lengths() {
    let Some(arms) = arms() else { return };
    let f = HostFrame::skyrim();
    let mut bones = skyrim_skeleton(&arms, f);
    for (name, k) in [("Forearm", 1.2), ("Hand", 0.85)] {
        for s in ["L", "R"] {
            let i = bones.iter().position(|b| b.name == format!("NPC {s} {name}")).unwrap();
            bones[i].pos *= k;
        }
    }
    let plain = Retarget::new(bones.clone(), f, &arms, &retarget::skyrim_links(), &retarget::skyrim_body_anchors());
    let r = Retarget::new(bones, f, &arms, &retarget::skyrim_links(), &retarget::skyrim_body_anchors()).with_arm_ik(&arms, &retarget::skyrim_arm_ik());
    let (me, place) = animated(&arms);
    let root = Xform { rot: Quat::IDENTITY, pos: f.point(place.origin), scale: 1.0 };
    let at = |r: &Retarget, local: &[Xform], n: &str| {
        let w = worlds(r, local, root);
        w[r.bones().iter().position(|b| b.name == n).unwrap()].pos
    };
    let faith = |n: &str| f.point(place.origin + place.body_rot * to_view(me[arms.bone(n).unwrap()].w_axis.truncate()));
    let (mut a, mut b) = (vec![], vec![]);
    plain.pose_with(&me, &place, root, true, &mut a);
    r.pose_with(&me, &place, root, true, &mut b);
    for (s, m) in [("L", "Left"), ("R", "Right")] {
        let want = faith(&format!("{m}Hand"));
        let off_plain = (at(&plain, &a, &format!("NPC {s} Hand")) - want).length();
        let off_ik = (at(&r, &b, &format!("NPC {s} Hand")) - want).length();
        assert!(off_plain > 1.0, "{s}: without IK the hand should miss ({off_plain})");
        assert!(off_ik < 0.5, "{s}: hand {off_ik} units off Faith's");
        // The elbow bends to the same side of the shoulder-hand line as hers.
        let sh = at(&r, &b, &format!("NPC {s} UpperArm"));
        let line = (want - sh).normalize();
        let side = |p: Vec3| {
            let d = p - sh;
            d - line * d.dot(line)
        };
        let host_elbow = side(at(&r, &b, &format!("NPC {s} Forearm")));
        let her_elbow = side(faith(&format!("{m}ForeArm")));
        assert!(host_elbow.dot(her_elbow) > 0.0, "{s}: elbow bends the other way");
    }
}

/// appear through her arms' wider one: the same direction scaled by k, the same depth.
#[test]
fn arm_ik_matches_faiths_hands_on_screen() {
    let Some(arms) = arms() else { return };
    let f = HostFrame::skyrim();
    let r = Retarget::new(skyrim_skeleton(&arms, f), f, &arms, &retarget::skyrim_links(), &retarget::skyrim_body_anchors()).with_arm_ik(&arms, &retarget::skyrim_arm_ik());
    let (me, place) = animated(&arms);
    let root = Xform { rot: Quat::IDENTITY, pos: f.point(place.origin), scale: 1.0 };
    let cam = place.origin + place.body_rot * Vec3::new(0.0, 1.6, 0.0);
    // Looking down at her hands (they hang at her sides): in front of the view, on screen.
    let rot = place.body_rot * Quat::from_rotation_x(-1.25);
    let k = 0.7;
    let mut local = vec![];
    r.pose_seen(&me, &place, root, true, Some((cam, rot, k)), &mut local);
    let w = worlds(&r, &local, root);
    let in_view = |p: Vec3| (f.axes * rot).inverse() * (p - f.point(cam));
    for (s, m) in [("L", "LeftHand"), ("R", "RightHand")] {
        let host = in_view(w[r.bones().iter().position(|b| b.name == format!("NPC {s} Hand")).unwrap()].pos);
        let her = in_view(f.point(place.origin + place.body_rot * to_view(me[arms.bone(m).unwrap()].w_axis.truncate())));
        let ahead = -her.z / her.length();
        assert!(ahead > 0.6, "{s}: the hand should be well in view ({ahead})");
        assert!((host.z - her.z).abs() < 0.5, "{s}: depth {} vs {}", host.z, her.z);
        assert!((host.x - her.x * k).abs() < 0.5 && (host.y - her.y * k).abs() < 0.5, "{s}: {host} vs {} x {k}", her);
    }
}

/// The whole-body view as the plugin poses it, every frame through stand / look / run / jump
/// (run with --ignored --nocapture): how close the body gets to the camera (near-clip popping)
/// and how much it jumps about against the camera from frame to frame (shaking).
#[test]
#[ignore]
fn body_view_flicker_report() {
    use faith_move::{greybox, CameraFx, Controller, Input, State, Tuning};
    let Some(arms) = arms() else { return };
    let f = HostFrame::skyrim();
    let mut bones = skyrim_skeleton(&arms, f);
    for b in &mut bones {
        b.pos *= 1.08;
    }
    let r = Retarget::new(bones, f, &arms, &retarget::skyrim_links(), &retarget::skyrim_body_anchors()).with_arm_ik(&arms, &retarget::skyrim_arm_ik());
    let idx = |n: &str| r.bones().iter().position(|b| b.name == n || b.name.starts_with(&format!("{n} ["))).unwrap();
    let watch = ["NPC Neck", "NPC Spine2", "NPC L Clavicle", "NPC R Clavicle", "NPC L UpperArm", "NPC R UpperArm"];
    let level = greybox::greybox();
    let world = level.world();
    let cp = &level.checkpoints[0];
    let mut c = Controller::new(Tuning::default(), cp.spawn, cp.yaw);
    c.state = State::Ground;
    let mut fx = CameraFx::default();
    let mut rig = Rig::new(&arms, c.yaw);
    let dt = 1.0 / 60.0;
    let mut last: Option<Vec<Vec3>> = None;
    let (mut seg_min, mut seg_jump, mut seg_jump_p) = (f32::MAX, 0.0f32, "");
    for step in 0..600 {
        let (mv, look_x, look_y, jump, label) = match step {
            0..=119 => (Vec2::ZERO, 0.0, 0.0, false, "stand"),
            120..=179 => (Vec2::ZERO, 0.0, -0.02, false, "look down"),
            180..=239 => (Vec2::ZERO, 0.03, 0.02, false, "look round"),
            240..=419 => (Vec2::new(0.0, 1.0), 0.0, 0.0, false, "run/sprint"),
            420 => (Vec2::new(0.0, 1.0), 0.0, 0.0, true, "jump"),
            _ => (Vec2::new(0.0, 1.0), 0.0, 0.0, false, "jump/land"),
        };
        let input = Input { move_axis: mv, look: Vec2::new(look_x, look_y), jump_pressed: jump, jump_held: jump, ..Default::default() };
        c.step(dt, &input, &world);
        let shot = fx.update(dt, &c, &input);
        let fr = rig.update(dt, &c, &shot, &arms);
        let place = Placement { origin: fr.origin, body_rot: fr.body_rot, legs_rot: fr.legs_rot };
        let root = Xform { rot: Quat::IDENTITY, pos: f.point(place.origin), scale: 1.0 };
        let mut local = vec![];
        r.pose_seen(&rig.driver.globals, &place, root, true, Some((fr.cam_pos, fr.cam_rot, 0.7)), &mut local);
        let w = worlds(&r, &local, root);
        let cam = f.point(fr.cam_pos);
        let rot = f.axes * fr.cam_rot;
        // In the camera's own space (so the camera's own motion doesn't count as shaking).
        let now: Vec<Vec3> = watch.iter().map(|n| rot.inverse() * (w[idx(n)].pos - cam)).collect();
        for (i, p) in now.iter().enumerate() {
            if p.length() < seg_min {
                seg_min = p.length();
            }
            if let Some(l) = &last {
                let j = (*p - l[i]).length();
                if j > seg_jump {
                    seg_jump = j;
                    seg_jump_p = watch[i];
                }
            }
        }
        last = Some(now);
        if step % 60 == 59 {
            eprintln!("{label:11} closest body bone to the camera {seg_min:5.1} units | biggest frame-to-frame jump {seg_jump:5.2} units ({seg_jump_p})");
            seg_min = f32::MAX;
            seg_jump = 0.0;
        }
    }
}

/// The disarm's victim: the cop's clip on a Skyrim-named skeleton built on the cop, each limb
/// pointing the way the cop's does, standing where it's put and facing the way it's turned.
#[test]
fn the_victim_clip_onto_a_skyrim_skeleton() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    use me_assets::*;
    let cop = FaithArms::load_character(std::path::Path::new(&dir), VICTIM_PACKAGE, VICTIM_MESH, VICTIM_ANIMS, VICTIM_SET).unwrap();
    let f = HostFrame::skyrim();
    let r = Retarget::new(skyrim_skeleton_on(&cop, f, "Spine2"), f, &cop, &retarget::skyrim_npc_links(), &retarget::skyrim_npc_anchors());
    assert!(r.mapped() >= 40, "mapped {}", r.mapped());
    let mut me = vec![];
    for seq in ["SnatchFwd", "SnatchBack"] {
        let len = cop.anims.sequences[seq].length;
        for t in [0.0, len * 0.4, len * 0.8] {
            assert!(retarget::clip_globals(&cop, seq, t, &mut me));
            let place = Placement { origin: Vec3::new(2.0, 0.0, -3.0), body_rot: Quat::from_rotation_y(0.8), legs_rot: Quat::from_rotation_y(0.8) };
            let root = Xform { rot: Quat::IDENTITY, pos: Vec3::ZERO, scale: 1.0 };
            let mut local = vec![];
            r.pose(&me, &place, root, &mut local);
            let w = worlds(&r, &local, root);
            let host = |n: &str| w[r.bones().iter().position(|b| b.name.starts_with(n)).unwrap()].pos;
            let placed = |n: &str| f.point(place.origin + place.body_rot * to_view(me[cop.bone(n).unwrap()].w_axis.truncate()));
            for (s, m) in [("L", "Left"), ("R", "Right")] {
                for (ha, hb, ma, mb) in [("UpperArm", "Forearm", "Arm", "ForeArm"), ("Forearm", "Hand", "ForeArm", "Hand"), ("Thigh", "Calf", "UpLeg", "Leg")] {
                    let got = host(&format!("NPC {s} {hb}")) - host(&format!("NPC {s} {ha}"));
                    let want = placed(&format!("{m}{mb}")) - placed(&format!("{m}{ma}"));
                    let err = got.angle_between(want).to_degrees();
                    assert!(err < 2.0, "{seq} {t}: {s} {ha}->{hb} {err} degrees off");
                }
            }
            let hips = host("NPC Pelvis") - placed("Hips");
            assert!(hips.length() < 1.0, "{seq} {t}: hips off by {hips}");
        }
    }
}

/// Both takedown clips are authored from her spot: the cop's has him DisarmOffset (1.26 m)
/// ahead of its origin, facing her for the front snatches and away for the one from behind. Laid
/// out from one origin, the same way round, her hands meet his (hands, gun, head), closer than
/// with his turned round.
#[test]
fn takedown_clips_share_her_origin() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    use me_assets::*;
    let faith = FaithArms::load(std::path::Path::new(&dir), 4).unwrap();
    let cop = FaithArms::load_character(std::path::Path::new(&dir), VICTIM_PACKAGE, VICTIM_MESH, VICTIM_ANIMS, VICTIM_SET).unwrap();
    for (seq, faces_her) in [("SnatchFwd", true), ("SnatchFwd2", true), ("SnatchFwd3", true), ("SnatchBack", false)] {
        let len = faith.anims.sequences[seq].length.min(cop.anims.sequences[seq].length);
        let (mut fm, mut cm) = (vec![], vec![]);
        // Where the cop stands, and which way he faces, at the start.
        retarget::clip_globals(&cop, seq, 0.0, &mut cm);
        let at = |m: &[Mat4], n: &str| to_view(m[cop.bone(n).unwrap()].w_axis.truncate());
        let root = at(&cm, "root");
        assert!((root.z + 1.259).abs() < 0.05 && root.x.abs() < 0.05, "{seq}: he starts DisarmOffset ahead: {root}");
        let toes = (at(&cm, "LeftToeBase") - at(&cm, "LeftFoot")) + (at(&cm, "RightToeBase") - at(&cm, "RightFoot"));
        assert_eq!(toes.z > 0.0, faces_her, "{seq}: facing her {faces_her}: toes {toes}");
        let mut sums = vec![];
        for (same, rot) in [(true, Quat::IDENTITY), (false, Quat::from_rotation_y(std::f32::consts::PI))] {
            let mut sum = 0.0;
            for fh in ["LeftHand", "RightHand"] {
                let mut best = f32::MAX;
                let mut t = 0.0;
                while t < len {
                    retarget::clip_globals(&faith, seq, t, &mut fm);
                    retarget::clip_globals(&cop, seq, t, &mut cm);
                    let a = to_view(fm[faith.bone(fh).unwrap()].w_axis.truncate());
                    for ch in ["LeftHand", "RightHand", "RightWeapon", "Head"] {
                        best = best.min(a.distance(rot * to_view(cm[cop.bone(ch).unwrap()].w_axis.truncate())));
                    }
                    t += 1.0 / 30.0;
                }
                sum += best;
            }
            eprintln!("{seq}, {}: her hands' closest {sum:.2} m (both summed)", if same { "same origin" } else { "turned round" });
            sums.push(sum);
        }
        assert!(sums[0] < 0.4 && sums[0] < sums[1], "{seq}: from one origin her hands come within {:.2} m (turned round {:.2})", sums[0], sums[1]);
    }
}
