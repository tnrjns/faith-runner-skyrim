//! Puts Faith's animated pose onto another game's skeleton (Skyrim's), bone for bone.
//!
//! The two skeletons have different bone frames, lengths and rest poses. So:
//! - **Rotations** carry over through a fixed offset per bone. At bind time Faith's rest pose
//!   is turned, chain by chain, until each mapped bone points the way the host's does (along
//!   a matching pair of joints, plus a second pair to fix the roll where there is one). From
//!   then on, host bone = Faith's bone × that offset, in the world.
//! - **Lengths** stay the host's: only rotations are set, except for a few **anchors** whose
//!   position follows Faith's: the hips (crouching, landing dips, rolls), scaled to the host's
//!   leg length, and in first person the shoulders, so the arms sit where they do in Mirror's
//!   Edge relative to the camera.
//! - Every host bone gets a full local transform each frame (unmapped ones their rest one),
//!   so none of the host's own animation leaks through.

use glam::{Mat3, Mat4, Quat, Vec3};
use me_assets::FaithArms;
use me_assets::pose::{self, Pose};

/// A host skeleton bone at rest, in parent order (a bone's parent comes before it).
#[derive(Clone, Debug)]
pub struct BoneRest {
    pub name: String,
    pub parent: Option<usize>,
    pub rot: Quat,
    pub pos: Vec3,
    pub scale: f32,
}

/// A transform: rotation, translation and uniform scale (the way Gamebryo / NetImmerse
/// stores them: world = parent.rot * local.pos * parent.scale + parent.pos).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub rot: Quat,
    pub pos: Vec3,
    pub scale: f32,
}

impl Xform {
    pub const IDENTITY: Xform = Xform { rot: Quat::IDENTITY, pos: Vec3::ZERO, scale: 1.0 };

    pub fn then(&self, local: &Xform) -> Xform {
        Xform { rot: (self.rot * local.rot).normalize(), pos: self.pos + self.rot * local.pos * self.scale, scale: self.scale * local.scale }
    }
}

/// The host's world against faith_move's: how to turn faith's axes into the host's, and its
/// units per metre. Skyrim: Z up, north +Y, 70 units a metre.
#[derive(Clone, Copy, Debug)]
pub struct HostFrame {
    pub axes: Quat,
    pub units_per_meter: f32,
}

impl HostFrame {
    /// Skyrim (Gamebryo): x east, y north, z up. faith_move's -Z (yaw 0) is north.
    pub fn skyrim() -> Self {
        HostFrame { axes: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2), units_per_meter: 70.0 }
    }

    pub fn point(&self, p: Vec3) -> Vec3 {
        self.axes * p * self.units_per_meter
    }

    pub fn point_back(&self, p: Vec3) -> Vec3 {
        self.axes.inverse() * p / self.units_per_meter
    }

    pub fn rot(&self, q: Quat) -> Quat {
        (self.axes * q * self.axes.inverse()).normalize()
    }

    pub fn rot_back(&self, q: Quat) -> Quat {
        (self.axes.inverse() * q * self.axes).normalize()
    }
}

/// One host bone driven by one of Faith's.
#[derive(Clone, Copy, Debug)]
pub struct LinkSpec {
    pub host: &'static str,
    pub me: &'static str,
    /// The bone points from `aim.0` to `aim.1` in Faith's skeleton, and from the host bone to
    /// `aim.2` in the host's.
    pub aim: Option<(&'static str, &'static str, &'static str)>,
    /// A second direction that fixes the roll about the first: Faith's `.0` to `.1`, the
    /// host's `.2` to `.3`.
    pub side: Option<(&'static str, &'static str, &'static str, &'static str)>,
}

/// A host bone whose position follows Faith's bone `me`.
#[derive(Clone, Copy, Debug)]
pub struct AnchorSpec {
    pub host: &'static str,
    pub me: &'static str,
    /// Moved by shifting its topmost ancestor under the skeleton's root (the hips carry the
    /// whole body) rather than the bone itself.
    pub carry_body: bool,
    /// Scaled to the host's size (by the hips' rest height), or exactly Faith's.
    pub scaled: bool,
    /// Only applied when `pose` is asked for the optional anchors.
    pub optional: bool,
}

/// An arm the host's hand is reached along exactly onto Faith's (two-bone IK): the host's upper
/// arm and forearm bones (the first of each the main bone, the rest the twist bones that turn
/// with it), its hand, and Faith's hand and elbow.
pub struct IkSpec {
    pub upper: &'static [&'static str],
    pub lower: &'static [&'static str],
    pub hand: &'static str,
    pub me_hand: &'static str,
    pub me_elbow: &'static str,
}

struct Ik {
    upper: Vec<usize>,
    lower: Vec<usize>,
    hand: usize,
    me_hand: usize,
    me_elbow: usize,
}

/// Where a two-bone chain's middle joint goes: from `s`, bones `a` then `b` long, reaching for
/// `t`, bending towards `pole`. The reach is held just inside what the bones span.
pub fn two_bone_elbow(s: Vec3, t: Vec3, pole: Vec3, a: f32, b: f32) -> Vec3 {
    let to = t - s;
    let d = to.length().clamp((a - b).abs() + 1e-3, a + b - 1e-3);
    let u = to.normalize_or(Vec3::X);
    let mut v = (pole - s) - u * (pole - s).dot(u);
    if v.length_squared() < 1e-8 {
        v = u.any_orthonormal_vector();
    }
    let v = v.normalize();
    let cos = ((a * a + d * d - b * b) / (2.0 * a * d)).clamp(-1.0, 1.0);
    s + u * (a * cos) + v * (a * (1.0 - cos * cos).max(0.0).sqrt())
}

/// Faith's lower body: placed with the legs' facing, not the upper body's (they're separate
/// meshes on one skeleton in Mirror's Edge).
const LEGS: [&str; 13] = [
    "Hips", "Spine", "Spine1", "LeftUpLeg", "LeftLeg", "LeftFoot", "LeftToeBase", "LeftUpLegRoll", "RightUpLeg", "RightLeg", "RightFoot",
    "RightToeBase", "RightUpLegRoll",
];

/// Where one of Faith's bones is in faith_move's world this frame (her attacks sweep a limb).
pub fn bone_position(arms: &FaithArms, globals: &[Mat4], place: &Placement, name: &str) -> Option<Vec3> {
    let b = arms.bone(name)?;
    let g = globals.get(b)?;
    let rot = if LEGS.iter().any(|l| l.eq_ignore_ascii_case(name)) { place.legs_rot } else { place.body_rot };
    Some(place.origin + rot * pose::to_view(g.w_axis.truncate()))
}

/// Where the body is this frame (faith_move's world): see `RigFrame`.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub origin: Vec3,
    pub body_rot: Quat,
    pub legs_rot: Quat,
}

struct Link {
    me: usize,
    legs: bool,
    /// Host bone world = Faith's bone world (host frame) * offset.
    offset: Quat,
}

struct Anchor {
    /// Only when asked (`pose`'s `optional`): the hands, pinned to Faith's when her own body is
    /// drawn and the host's hands only carry what they hold.
    optional: bool,
    bone: usize,
    me: usize,
    legs: bool,
    carrier: usize,
    scale: f32,
}

pub struct Retarget {
    bones: Vec<BoneRest>,
    links: Vec<Option<Link>>,
    anchors: Vec<Anchor>,
    frame: HostFrame,
    ik: Vec<Ik>,
}

/// Name compare ignoring case and Skyrim's bracketed short codes ("NPC L Hand [LHnd]").
fn stem(name: &str) -> String {
    name.split(" [").next().unwrap_or(name).trim().to_ascii_lowercase()
}

/// The rotation taking direction `a` (and, if given, `a2` about it) onto `b` (`b2`).
fn align(a: Vec3, b: Vec3, side: Option<(Vec3, Vec3)>) -> Quat {
    let (a, b) = (a.normalize_or_zero(), b.normalize_or_zero());
    if a == Vec3::ZERO || b == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    let basis = |x: Vec3, s: Vec3| -> Option<Mat3> {
        let y = (s - x * s.dot(x)).normalize_or_zero();
        (y != Vec3::ZERO).then(|| Mat3::from_cols(x, y, x.cross(y)))
    };
    if let Some((a2, b2)) = side {
        if let (Some(ma), Some(mb)) = (basis(a, a2), basis(b, b2)) {
            return Quat::from_mat3(&(mb * ma.transpose())).normalize();
        }
    }
    Quat::from_rotation_arc(a, b)
}

impl Retarget {
    /// Bind `bones` (the host skeleton at rest, local transforms in host units) to Faith's.
    pub fn new(bones: Vec<BoneRest>, frame: HostFrame, arms: &FaithArms, links: &[LinkSpec], anchors: &[AnchorSpec]) -> Self {
        for (i, b) in bones.iter().enumerate() {
            assert!(b.parent.is_none_or(|p| p < i), "bone {} comes before its parent", b.name);
        }
        let host_idx = |n: &str| {
            let s = stem(n);
            bones.iter().position(|b| stem(&b.name) == s)
        };
        // The host at rest, in its model space.
        let mut rest_w: Vec<Xform> = Vec::with_capacity(bones.len());
        for b in &bones {
            let parent = b.parent.map_or(Xform::IDENTITY, |p| rest_w[p]);
            rest_w.push(parent.then(&Xform { rot: b.rot, pos: b.pos, scale: b.scale }));
        }
        // Faith at rest, facing the host's forward, in host units.
        let mut g = vec![];
        pose::globals(&arms.mesh, &Pose::rest(&arms.mesh), &mut g);
        let me_rot = |m: usize| frame.rot(Quat::from_mat4(&g[m]).normalize());
        let me_pos = |m: usize| frame.point(pose::to_view(g[m].w_axis.truncate()));
        let me_idx = |n: &str| arms.bone(n);

        let mut out: Vec<Option<Link>> = (0..bones.len()).map(|_| None).collect();
        // The turn applied to Faith's rest pose so far, per host bone (to its mapped ancestors).
        let mut swing = vec![Quat::IDENTITY; bones.len()];
        for (b, bone) in bones.iter().enumerate() {
            let inherited = bone.parent.map_or(Quat::IDENTITY, |p| swing[p]);
            swing[b] = inherited;
            let Some(spec) = links.iter().find(|l| stem(l.host) == stem(&bone.name)) else { continue };
            let Some(m) = me_idx(spec.me) else { continue };
            let dir = |a: &str, z: &str, h: &str| -> Option<(Vec3, Vec3)> {
                let (ma, mz, hz) = (me_idx(a)?, me_idx(z)?, host_idx(h)?);
                Some((inherited * (me_pos(mz) - me_pos(ma)), rest_w[hz].pos - rest_w[b].pos))
            };
            let mut s = inherited;
            if let Some((da, db)) = spec.aim.and_then(|(a, z, h)| dir(a, z, h)) {
                let side = spec.side.and_then(|(a, z, h0, h1)| {
                    let (ma, mz, h0, h1) = (me_idx(a)?, me_idx(z)?, host_idx(h0)?, host_idx(h1)?);
                    Some((inherited * (me_pos(mz) - me_pos(ma)), rest_w[h1].pos - rest_w[h0].pos))
                });
                s = align(da, db, side) * inherited;
            }
            swing[b] = s;
            let matched = s * me_rot(m);
            out[b] = Some(Link { me: m, legs: LEGS.contains(&spec.me), offset: (matched.inverse() * rest_w[b].rot).normalize() });
        }

        // Hips height, for scaling Faith's hip motion to the host's legs.
        let size = || -> Option<f32> {
            let hips = me_idx("Hips")?;
            let host = links.iter().find(|l| l.me == "Hips").and_then(|l| host_idx(l.host))?;
            let up = frame.axes * Vec3::Y;
            let h = rest_w[host].pos.dot(up);
            let m = me_pos(hips).dot(up);
            (h > 0.0 && m > 0.0).then_some(h / m)
        };
        let ratio = size().unwrap_or(1.0);
        let anchors = anchors
            .iter()
            .filter_map(|a| {
                let bone = host_idx(a.host)?;
                let me = me_idx(a.me)?;
                let mut carrier = bone;
                if a.carry_body {
                    while let Some(p) = bones[carrier].parent {
                        if bones[p].parent.is_none() {
                            break;
                        }
                        carrier = p;
                    }
                }
                Some(Anchor { optional: a.optional, bone, me, legs: LEGS.contains(&a.me), carrier, scale: if a.scaled { ratio } else { 1.0 } })
            })
            .collect();
        Retarget { bones, links: out, anchors, frame, ik: vec![] }
    }

    /// Reach these arms' hands exactly onto Faith's whenever the optional anchors are asked for
    /// (the body seen from her camera, the hidden first-person arms under hers): the host's arm
    /// keeps its own lengths and bends at the elbow, in the plane Faith's elbow makes, instead
    /// of its hand landing short of or past hers.
    pub fn with_arm_ik(mut self, arms: &FaithArms, specs: &[IkSpec]) -> Self {
        let idx = |n: &str| {
            let s = stem(n);
            self.bones.iter().position(|b| stem(&b.name) == s)
        };
        let mut ik = Vec::new();
        for s in specs {
            let upper: Vec<usize> = s.upper.iter().filter_map(|n| idx(n)).collect();
            let lower: Vec<usize> = s.lower.iter().filter_map(|n| idx(n)).collect();
            let (Some(hand), Some(me_hand), Some(me_elbow)) = (idx(s.hand), arms.bone(s.me_hand), arms.bone(s.me_elbow)) else { continue };
            if upper.is_empty() || lower.is_empty() {
                continue;
            }
            ik.push(Ik { upper, lower, hand, me_hand, me_elbow });
        }
        self.ik = ik;
        self
    }

    pub fn bones(&self) -> &[BoneRest] {
        &self.bones
    }

    /// How many host bones are driven.
    pub fn mapped(&self) -> usize {
        self.links.iter().filter(|l| l.is_some()).count()
    }

    pub fn is_mapped(&self, bone: usize) -> bool {
        self.links[bone].is_some()
    }

    /// This frame's local transforms for every host bone. `me` is Faith's pose (model space,
    /// `Driver::globals`), `place` where the body is, `root_parent` the host world transform
    /// of the node the skeleton's root bones hang from.
    pub fn pose(&self, me: &[Mat4], place: &Placement, root_parent: Xform, out: &mut Vec<Xform>) {
        self.pose_with(me, place, root_parent, false, out)
    }

    /// [`Self::pose`], with the optional anchors too (`optional`).
    pub fn pose_with(&self, me: &[Mat4], place: &Placement, root_parent: Xform, optional: bool, out: &mut Vec<Xform>) {
        self.pose_seen(me, place, root_parent, optional, None, out)
    }

    /// [`Self::pose_with`], the arms' targets seen through another field of view: `seen` is
    /// Faith's camera (faith_move's frame) and how much narrower the view the body is drawn with
    /// is than her arms' (tan(its half angle) / tan(theirs)). Her hands are placed as far across
    /// the screen as hers appear (Mirror's Edge draws her arms at its own 100 degrees), at
    /// the same depth.
    pub fn pose_seen(&self, me: &[Mat4], place: &Placement, root_parent: Xform, optional: bool, seen: Option<(Vec3, Quat, f32)>, out: &mut Vec<Xform>) {
        let f = &self.frame;
        let placed = |legs: bool| if legs { place.legs_rot } else { place.body_rot };
        let world_rot = |l: &Link| f.rot(placed(l.legs) * Quat::from_mat4(&me[l.me]).normalize()) * l.offset;
        let origin = f.point(place.origin);
        out.clear();
        out.extend(self.bones.iter().map(|b| Xform { rot: b.rot, pos: b.pos, scale: b.scale }));
        let mut world = vec![Xform::IDENTITY; self.bones.len()];
        // The arm IK's turns, on top of Faith's rotations (identity for every other bone).
        let mut corr = vec![Quat::IDENTITY; self.bones.len()];
        let solve = |out: &mut Vec<Xform>, world: &mut Vec<Xform>, corr: &[Quat]| {
            for (i, b) in self.bones.iter().enumerate() {
                let parent = b.parent.map_or(root_parent, |p| world[p]);
                if let Some(l) = &self.links[i] {
                    out[i].rot = (parent.rot.inverse() * (corr[i] * world_rot(l))).normalize();
                }
                world[i] = parent.then(&out[i]);
            }
        };
        solve(out, &mut world, &corr);
        // A hand the arm IK reaches is pinned after it (only what the arm can't reach is left).
        let ik_hand = |a: &&Anchor| self.ik.iter().any(|k| k.hand == a.bone);
        let pin = |a: &Anchor, out: &mut Vec<Xform>, world: &mut Vec<Xform>, corr: &[Quat]| {
            let me_at = f.point(place.origin + placed(a.legs) * pose::to_view(me[a.me].w_axis.truncate()));
            let target = origin + (me_at - origin) * a.scale;
            let delta = target - world[a.bone].pos;
            let c = a.carrier;
            let parent = self.bones[c].parent.map_or(root_parent, |p| world[p]);
            out[c].pos += parent.rot.inverse() * delta / parent.scale;
            solve(out, world, corr);
        };
        for a in self.anchors.iter().filter(|a| (optional || !a.optional) && !(optional && ik_hand(a))) {
            pin(a, out, &mut world, &corr);
        }
        if !optional {
            return;
        }
        let me_at = |m: usize| {
            let p = f.point(place.origin + place.body_rot * pose::to_view(me[m].w_axis.truncate()));
            match seen {
                Some((cam, rot, k)) => {
                    // The camera's turn takes its own axes (right x, up y, back z) to faith_move's;
                    // to the host's, add the frame's axes after it.
                    let (c, r) = (f.point(cam), f.axes * rot);
                    let v = r.inverse() * (p - c);
                    // Only what's in front of the view is on screen to match: full within ~53
                    // degrees of where she looks, none past ~72 (hands hanging at her sides stay
                    // where they are).
                    let ahead = (-v.z / v.length().max(1e-4)).clamp(-1.0, 1.0);
                    let w = ((ahead - 0.3) / 0.3).clamp(0.0, 1.0);
                    let s = 1.0 - w * (1.0 - k);
                    c + r * Vec3::new(v.x * s, v.y * s, v.z)
                }
                None => p,
            }
        };
        for arm in &self.ik {
            let (u, l) = (arm.upper[0], arm.lower[0]);
            let s = world[u].pos;
            let a = (world[l].pos - s).length();
            let b = (world[arm.hand].pos - world[l].pos).length();
            if a < 1e-4 || b < 1e-4 {
                continue;
            }
            let target = me_at(arm.me_hand);
            let elbow = two_bone_elbow(s, target, me_at(arm.me_elbow), a, b);
            // Turn the upper arm so the elbow lands there, then the forearm so the hand does.
            let turn = Quat::from_rotation_arc((world[l].pos - s).normalize(), (elbow - s).normalize());
            for &i in &arm.upper {
                corr[i] = turn * corr[i];
            }
            solve(out, &mut world, &corr);
            let e = world[l].pos;
            let turn = Quat::from_rotation_arc((world[arm.hand].pos - e).normalize(), (target - e).normalize_or(world[arm.hand].pos - e));
            for &i in &arm.lower {
                corr[i] = turn * corr[i];
            }
            solve(out, &mut world, &corr);
        }
        for a in self.anchors.iter().filter(|a| a.optional && ik_hand(a)) {
            pin(a, out, &mut world, &corr);
        }
    }
}

/// Skyrim's skeletons (`skeleton.nif`, `skeleton_female.nif`, `_1stperson\skeleton.nif`) from
/// Faith's. Names match without the bracketed codes.
pub fn skyrim_links() -> Vec<LinkSpec> {
    let mut v = vec![
        LinkSpec { host: "NPC Pelvis", me: "Hips", aim: Some(("Hips", "Spine1", "NPC Spine1")), side: Some(("RightUpLeg", "LeftUpLeg", "NPC R Thigh", "NPC L Thigh")) },
        LinkSpec { host: "NPC Spine", me: "Spine", aim: Some(("Spine", "Spine1", "NPC Spine1")), side: Some(("RightUpLeg", "LeftUpLeg", "NPC R Thigh", "NPC L Thigh")) },
        LinkSpec { host: "NPC Spine1", me: "Spine1", aim: Some(("Spine1", "Neck", "NPC Neck")), side: Some(("RightShoulder", "LeftShoulder", "NPC R Clavicle", "NPC L Clavicle")) },
        // SpineX is the upper body's own root in Mirror's Edge (it sits at the eye); it turns the
        // chest, so it drives Spine2, pointing the way the chest does.
        LinkSpec { host: "NPC Spine2", me: "SpineX", aim: Some(("Spine1", "Neck", "NPC Neck")), side: Some(("RightShoulder", "LeftShoulder", "NPC R Clavicle", "NPC L Clavicle")) },
        LinkSpec { host: "NPC Neck", me: "Neck", aim: Some(("Neck", "Head", "NPC Head")), side: Some(("RightShoulder", "LeftShoulder", "NPC R Clavicle", "NPC L Clavicle")) },
        LinkSpec { host: "NPC Head", me: "Head", aim: None, side: None },
    ];
    for (s, me) in [("L", "Left"), ("R", "Right")] {
        let n = |x: &str| -> &'static str { Box::leak(format!("NPC {s} {x}").into_boxed_str()) };
        let m = |x: &str| -> &'static str { Box::leak(format!("{me}{x}").into_boxed_str()) };
        v.extend([
            LinkSpec { host: n("Clavicle"), me: m("Shoulder"), aim: Some((m("Shoulder"), m("Arm"), n("UpperArm"))), side: None },
            LinkSpec { host: n("UpperArm"), me: m("Arm"), aim: Some((m("Arm"), m("ForeArm"), n("Forearm"))), side: None },
            LinkSpec { host: n("UpperarmTwist1"), me: m("Arm"), aim: Some((m("Arm"), m("ForeArm"), n("Forearm"))), side: None },
            LinkSpec { host: n("UpperarmTwist2"), me: m("Arm"), aim: Some((m("Arm"), m("ForeArm"), n("Forearm"))), side: None },
            LinkSpec { host: n("Forearm"), me: m("ForeArm"), aim: Some((m("ForeArm"), m("Hand"), n("Hand"))), side: None },
            // The forearm's twist bones roll with the hand, like Faith's ForeArmRoll.
            LinkSpec { host: n("ForearmTwist1"), me: m("ForeArmRoll"), aim: Some((m("ForeArm"), m("Hand"), n("Hand"))), side: None },
            LinkSpec { host: n("ForearmTwist2"), me: m("ForeArmRoll"), aim: Some((m("ForeArm"), m("Hand"), n("Hand"))), side: None },
            LinkSpec {
                host: n("Hand"),
                me: m("Hand"),
                aim: Some((m("Hand"), m("HandMiddle1"), n("Finger20"))),
                side: Some((m("HandPinky1"), m("HandIndex1"), n("Finger40"), n("Finger10"))),
            },
            LinkSpec { host: n("Thigh"), me: m("UpLeg"), aim: Some((m("UpLeg"), m("Leg"), n("Calf"))), side: None },
            LinkSpec { host: n("Calf"), me: m("Leg"), aim: Some((m("Leg"), m("Foot"), n("Foot"))), side: None },
            LinkSpec { host: n("Foot"), me: m("Foot"), aim: Some((m("Foot"), m("ToeBase"), n("Toe0"))), side: None },
            LinkSpec { host: n("Toe0"), me: m("ToeBase"), aim: None, side: None },
        ]);
        // Thumb Finger0x, index 1x, middle 2x, ring 3x, little 4x; three joints each.
        for (digit, name) in ["Thumb", "Index", "Middle", "Ring", "Pinky"].iter().enumerate() {
            // Faith's joints are numbered 1-3 (Index0 etc. are the palm's).
            for j in 0..3 {
                let me_j = m(&format!("Hand{name}{}", j + 1));
                let next = (j < 2).then(|| (me_j, m(&format!("Hand{name}{}", j + 2)), n(&format!("Finger{digit}{}", j + 1))));
                v.push(LinkSpec { host: n(&format!("Finger{digit}{j}")), me: me_j, aim: next, side: None });
            }
        }
    }
    v
}

/// Skyrim's whole body from another of the game's characters (the disarm's victim, a cop): as
/// [`skyrim_links`], but a third-person skeleton has a real Spine2 where Faith's first-person
/// one has SpineX.
pub fn skyrim_npc_links() -> Vec<LinkSpec> {
    let mut v = skyrim_links();
    for l in &mut v {
        if stem(l.host) == "npc spine2" {
            l.me = "Spine2";
            l.aim = Some(("Spine2", "Neck", "NPC Neck"));
        }
    }
    v
}

/// The victim's body: only the hips carry it, scaled to its legs.
pub fn skyrim_npc_anchors() -> Vec<AnchorSpec> {
    vec![AnchorSpec { host: "NPC Pelvis", me: "Hips", carry_body: true, scaled: true, optional: false }]
}

/// Another character's pose from one of its clips at `time` (model space, as
/// `Driver::globals`): the disarm's victim side.
pub fn clip_globals(who: &FaithArms, seq: &str, time: f32, out: &mut Vec<Mat4>) -> bool {
    let Some(s) = who.anims.sequences.get(seq) else { return false };
    let map = pose::TrackMap::new(&who.mesh, &who.anims);
    let rest = Pose::rest(&who.mesh);
    let mut p = rest.clone();
    pose::sample(s, &map, &rest, time, false, &mut p);
    pose::globals(&who.mesh, &p, out);
    true
}

/// The third-person body: the hips carry it, scaled to its legs.
pub fn skyrim_body_anchors() -> Vec<AnchorSpec> {
    vec![
        AnchorSpec { host: "NPC Pelvis", me: "Hips", carry_body: true, scaled: true, optional: false },
        // Seen from Faith's camera (the body in first person), the shoulders sit exactly where
        // hers do against it, so the arms come into view where hers would.
        AnchorSpec { host: "NPC L Clavicle", me: "LeftShoulder", carry_body: false, scaled: false, optional: true },
        AnchorSpec { host: "NPC R Clavicle", me: "RightShoulder", carry_body: false, scaled: false, optional: true },
    ]
}

/// The first-person arms: the shoulders sit exactly where Faith's do against the camera.
pub fn skyrim_arms_anchors() -> Vec<AnchorSpec> {
    vec![
        AnchorSpec { host: "NPC L Clavicle", me: "LeftShoulder", carry_body: false, scaled: false, optional: false },
        AnchorSpec { host: "NPC R Clavicle", me: "RightShoulder", carry_body: false, scaled: false, optional: false },
        // With Faith's own body drawn, the host's (hidden) hands go exactly where hers are, so
        // what they hold (Skyrim's weapons) is in her grip.
        AnchorSpec { host: "NPC L Hand", me: "LeftHand", carry_body: false, scaled: false, optional: true },
        AnchorSpec { host: "NPC R Hand", me: "RightHand", carry_body: false, scaled: false, optional: true },
    ]
}

/// Skyrim's arms reached onto Faith's hands.
pub fn skyrim_arm_ik() -> Vec<IkSpec> {
    vec![
        IkSpec {
            upper: &["NPC L UpperArm", "NPC L UpperarmTwist1", "NPC L UpperarmTwist2"],
            lower: &["NPC L Forearm", "NPC L ForearmTwist1", "NPC L ForearmTwist2"],
            hand: "NPC L Hand",
            me_hand: "LeftHand",
            me_elbow: "LeftForeArm",
        },
        IkSpec {
            upper: &["NPC R UpperArm", "NPC R UpperarmTwist1", "NPC R UpperarmTwist2"],
            lower: &["NPC R Forearm", "NPC R ForearmTwist1", "NPC R ForearmTwist2"],
            hand: "NPC R Hand",
            me_hand: "RightHand",
            me_elbow: "RightForeArm",
        },
    ]
}
