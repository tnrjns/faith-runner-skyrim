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
        ("NPC Spine2 [Spn2]".into(), Some("NPC Spine1 [Spn1]"), (me_pos("Spine1") + me_pos("Neck")) * 0.5, "SpineX"),
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
