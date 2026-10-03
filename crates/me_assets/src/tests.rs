//! These read a real Mirror's Edge install, so they only run when one is
//! available: set ME_INSTALL to the install folder (containing TdGame).

use glam::{Mat4, Vec3};

use super::*;

fn arms() -> Option<FaithArms> {
    let dir = std::env::var_os("ME_INSTALL")?;
    Some(FaithArms::load(Path::new(&dir), 1024).expect("load Faith's arms"))
}

fn pose_of(a: &FaithArms, seq: &str, t: f32) -> Vec<Mat4> {
    let map = TrackMap::new(&a.mesh, &a.anims);
    let rest = Pose::rest(&a.mesh);
    let mut p = rest.clone();
    pose::sample(&a.anims.sequences[seq], &map, &rest, t, true, &mut p);
    let mut g = vec![];
    pose::globals(&a.mesh, &p, &mut g);
    g
}

fn pos(a: &FaithArms, g: &[Mat4], bone: &str) -> Vec3 {
    g[a.bone(bone).unwrap()].w_axis.truncate()
}

#[test]
fn mesh_and_textures() {
    let Some(a) = arms() else { return };
    let m = &a.mesh;
    assert_eq!(m.bones.len(), 74);
    assert_eq!(m.vertices.len(), 4037);
    assert_eq!(m.indices.len(), 20628);
    assert!(m.vertices.iter().all(|v| v.weights.iter().map(|&w| w as u32).sum::<u32>() == 255));
    assert_eq!(m.sections.len(), 2);
    let names: Vec<_> = a.materials.iter().map(|s| s.material.as_str()).collect();
    assert_eq!(names, ["MI_Female_Arm_Skinn", "MI_Female_Arm_Glove"]);
    for s in &a.materials {
        let t = s.diffuse.as_ref().expect("diffuse");
        assert!(t.width == 1024 && t.height == 1024, "{} {}x{}", s.material, t.width, t.height);
        // Not a flat/black image.
        let avg: u64 = t.pixels.chunks(4).map(|p| p[0] as u64).sum::<u64>() / (t.width * t.height) as u64;
        assert!((20..240).contains(&avg), "{} avg red {avg}", s.material);
    }
}

#[test]
fn legs_load_with_their_textures() {
    let Some(a) = arms() else { return };
    let (legs, slots) = a.legs.as_ref().expect("legs mesh should load from the 1P package");
    assert_eq!(legs.bones.len(), a.mesh.bones.len());
    assert_eq!(slots.len(), 3);
    let have = |n: &str| slots.iter().find(|s| s.material == n).is_some_and(|s| s.diffuse.is_some());
    assert!(have("MI_SHtest"), "pants texture (CH_Faith_Cinematic.Faith_Cine_Lower_C)");
    assert!(have("MI_Faith_Lowres_Upper"), "top texture (CH_TKY_Crim_Fixer)");
}

#[test]
fn animations_decode_to_valid_rotations() {
    let Some(a) = arms() else { return };
    // The unarmed set and the disarm clips it lacks from the one-handed set.
    assert_eq!(a.anims.sequences.len(), 285);
    assert_eq!(a.anims.bones, a.mesh.bones.iter().map(|b| b.name.clone()).collect::<Vec<_>>());
    for s in a.anims.sequences.values() {
        for t in &s.tracks {
            for q in &t.rotations {
                let n = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
                assert!((n - 1.0).abs() < 1e-3, "{}: |q|² = {n}", s.name);
            }
        }
    }
}

#[test]
fn animation_sound_notifies() {
    use crate::anim::Notify;
    let Some(a) = arms() else { return };
    let run = &a.anims.sequences["runfwd"];
    let feet: Vec<_> = run.notifies.iter().filter_map(|n| match n.notify { Notify::Footstep(f) => Some((n.time, f)), _ => None }).collect();
    assert_eq!(feet.len(), 2, "{:?}", run.notifies);
    assert_eq!(feet[0].1, -3); // run step, left
    assert_eq!(feet[1].1, 3);
    assert!(run.notifies.iter().any(|n| n.notify == Notify::Character("ECSClothing_Run".into())));
    let total: usize = a.anims.sequences.values().map(|s| s.notifies.len()).sum();
    assert!(total > 800, "{total}");
}

#[test]
fn poses_look_right() {
    let Some(a) = arms() else { return };
    // Forward is +Z, up is −Y, right is −X in mesh space.
    let rel = |seq: &str, t: f32, hand: &str| {
        let g = pose_of(&a, seq, t);
        pos(&a, &g, hand) - pos(&a, &g, "EyeJoint")
    };
    // Running: arms pump in opposite phase, each swinging in front.
    let len = a.anims.sequences["runfwd"].length;
    let r: Vec<f32> = (0..8).map(|k| rel("runfwd", len * k as f32 / 8.0, "RightHand").z).collect();
    let l: Vec<f32> = (0..8).map(|k| rel("runfwd", len * k as f32 / 8.0, "LeftHand").z).collect();
    let (rmax, rmin) = (r.iter().cloned().fold(f32::MIN, f32::max), r.iter().cloned().fold(f32::MAX, f32::min));
    assert!(rmax > 25.0 && rmin < 8.0, "right hand swing {r:?}");
    let ri = r.iter().position(|&x| x == rmax).unwrap();
    assert!(l[ri] < 10.0, "left hand back while right is forward: {l:?}");
    // Hanging: both hands up at the ledge and forward.
    for h in ["RightHand", "LeftHand"] {
        let d = rel("Hang", 0.5, h);
        assert!(d.y < 0.0 && d.z > 15.0, "{h} while hanging: {d}");
    }
    // Wallrun on the right: right hand out on the wall (−X).
    assert!(rel("WallrunRight", 0.25, "RightHand").x < -40.0);
    assert!(rel("WallrunLeft", 0.25, "LeftHand").x > 40.0);
}

#[test]
fn triangles_face_outward_after_conversion() {
    // Front faces (counter-clockwise in view space) must agree with the
    // mesh's own normals, or the renderer culls the outside of the arms.
    let Some(a) = arms() else { return };
    let mut meshes = vec![&a.mesh];
    if let Some((l, _)) = &a.legs {
        meshes.push(l);
    }
    for m in meshes {
        let mut g = vec![];
        pose::globals(m, &Pose::rest(m), &mut g);
        let (mut p, mut n) = (vec![], vec![]);
        Skinner::new(m).skin_into(m, &g, Mat4::IDENTITY, &mut p, &mut n);
        for s in 0..m.sections.len() {
            let idx = pose::section_indices(m, s);
            let mut agree = 0;
            for t in idx.chunks_exact(3) {
                let (pa, pb, pc) = (Vec3::from(p[t[0] as usize]), Vec3::from(p[t[1] as usize]), Vec3::from(p[t[2] as usize]));
                let vn = Vec3::from(n[t[0] as usize]) + Vec3::from(n[t[1] as usize]) + Vec3::from(n[t[2] as usize]);
                if (pb - pa).cross(pc - pa).dot(vn) > 0.0 {
                    agree += 1;
                }
            }
            assert!(agree * 100 >= idx.len() / 3 * 99, "{} section {s}: {agree}/{} face outward", m.name, idx.len() / 3);
        }
    }
}

#[test]
fn rest_pose_skins_to_itself() {
    let Some(a) = arms() else { return };
    let mut g = vec![];
    pose::globals(&a.mesh, &Pose::rest(&a.mesh), &mut g);
    let sk = Skinner::new(&a.mesh);
    let (mut p, mut n) = (vec![], vec![]);
    sk.skin_into(&a.mesh, &g, Mat4::IDENTITY, &mut p, &mut n);
    for (v, q) in a.mesh.vertices.iter().zip(&p) {
        let expect = pose::to_view(Vec3::from(v.position));
        assert!((Vec3::from(*q) - expect).length() < 1e-4);
    }
}

#[test]
fn sound_cues_decode() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let cooked = cooked_pc(Path::new(&dir)).unwrap();
    let mut bank = SoundBank::new(Packages::new(&cooked));
    let run = bank.cue("A_Material_Footstep.Concrete._03_Female_FootStepRun").expect("run footstep cue");
    assert_eq!(run.waves.len(), 14);
    assert!(run.waves.iter().all(|w| w.ogg.starts_with(b"OggS")));
    assert!(run.volume.1 > 0.0 && run.pitch.0 > 0.5 && run.pitch.1 < 1.5, "{:?} {:?}", run.volume, run.pitch);
    let wind = bank.cue("A_Ambience_Wind.Wind.Wind").expect("wind");
    assert!(wind.looping);
    assert!(bank.cue("A_Character_Female_01.Breath_Medium.Breath_Medium_Short_In").is_some());
    assert!(bank.cue("A_Nope.X.Y").is_none());
    // The slide scrape loops; the game's slide move stops it when it ends.
    assert!(bank.cue("A_Material_Footstep.Concrete._11_Female_FootStepSlide").expect("slide").looping);
}

#[test]
#[ignore]
fn list_footstep_surfaces() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let cooked = cooked_pc(Path::new(&dir)).unwrap();
    let mut pk = Packages::new(&cooked);
    for name in ["A_Material_Footstep", "A_Material_Handstep"] {
        let p = pk.get(name).unwrap();
        let mut groups: Vec<String> = (0..p.exports.len())
            .filter(|&i| p.class_name(&p.exports[i]) == "SoundCue")
            .map(|i| format!("{name}.{}", p.path(i as i32 + 1)))
            .collect();
        groups.sort();
        for g in groups { eprintln!("{g}"); }
    }
}

#[test]
#[ignore]
fn list_material_params() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let cooked = cooked_pc(Path::new(&dir)).unwrap();
    let mut pkgs = Packages::new(&cooked);
    for (pk, mesh) in [(ARMS_PACKAGE, ARMS_MESH), (ARMS_PACKAGE, LEGS_MESH)] {
        let p = std::rc::Rc::new(crate::package::Package::open(&cooked.join(pk)).unwrap());
        let Ok(m) = crate::skeletal::SkeletalMesh::read(&p, mesh) else { continue };
        for &mi in &m.materials {
            eprintln!("{pk} {mesh} material {}", p.path(mi));
            if let Some((mp, me)) = pkgs.resolve(&p, mi) {
                for (n, v) in crate::material_textures(&mp, me) {
                    let fmt = pkgs.resolve(&mp, v).map(|(tp, ti)| {
                        let props = crate::props::Props::read(&tp, &tp.exports[ti]).ok();
                        let f = props.as_ref().and_then(|pr| pr.name(&tp, "Format")).unwrap_or_default();
                        let c = props.as_ref().and_then(|pr| pr.name(&tp, "CompressionSettings")).unwrap_or_default();
                        format!("{} {f} {c}", tp.exports[ti].name)
                    });
                    eprintln!("   {n} -> {} {:?}", mp.path(v), fmt);
                }
            }
        }
    }
}

#[test]
fn materials_have_normal_and_spec_maps() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let arms = FaithArms::load(Path::new(&dir), 256).unwrap();
    for slot in &arms.materials {
        assert!(slot.diffuse.is_some() && slot.normal.is_some() && slot.specular.is_some(), "{}", slot.material);
    }
    // The packed tangent basis is (close to) orthonormal.
    let mut bad = 0;
    for v in &arms.mesh.vertices {
        let (t, n) = (glam::Vec3::from(v.tangent), glam::Vec3::from(v.normal));
        if t.dot(n).abs() > 0.2 || (t.length() - 1.0).abs() > 0.1 {
            bad += 1;
        }
    }
    assert!(bad * 100 < arms.mesh.vertices.len(), "{bad} of {}", arms.mesh.vertices.len());
}

#[test]
#[ignore]
fn list_sequences() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let arms = FaithArms::load(Path::new(&dir), 16).unwrap();
    let mut v: Vec<_> = arms.anims.sequences.iter().map(|(k, s)| format!("{k} {:.2}s", s.length)).collect();
    v.sort();
    eprintln!("{}", v.join("\n"));
}

#[test]
#[ignore]
fn list_notifier_dummies() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let arms = FaithArms::load(Path::new(&dir), 16).unwrap();
    for (k, s) in &arms.anims.sequences {
        if k.to_ascii_lowercase().starts_with("notifierdummy") || k.starts_with("Melee") || k.starts_with("swing") || k.starts_with("zip") || k.starts_with("SpringBoard") || k.starts_with("walkbalance") {
            eprintln!("{k} {:.2}s rate {:.2}: {:?}", s.length, s.rate, s.notifies.iter().map(|n| (n.time, &n.notify)).collect::<Vec<_>>());
        }
    }
}

#[test]
#[ignore]
fn list_cues_in() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let cooked = cooked_pc(Path::new(&dir)).unwrap();
    let mut pk = Packages::new(&cooked);
    let mut bank = SoundBank::new(Packages::new(&cooked));
    for name in ["A_Character_Melee", "A_Kits"] {
        let Ok(p) = pk.get(name) else { eprintln!("missing {name}"); continue };
        for i in 0..p.exports.len() {
            if p.class_name(&p.exports[i]) == "SoundCue" {
                let path = format!("{name}.{}", p.path(i as i32 + 1));
                let c = bank.cue(&path);
                eprintln!("{path} {:?}", c.map(|c| (c.waves.len(), c.looping, c.volume)));
            }
        }
    }
}

/// The arms' forearm twist morphs load and fit the arms mesh.
#[test]
fn arm_morphs_fit_the_mesh() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let arms = crate::FaithArms::load(Path::new(&dir), 64).unwrap();
    let names: Vec<_> = arms.morphs.iter().map(|m| m.name.as_str()).collect();
    for want in ["LeftForeArmRollBlend90", "LeftForeArmRollBlend90m", "RightForeArmRollBlend90", "RightForeArmRollBlend90m"] {
        assert!(names.contains(&want), "{names:?}");
    }
    for m in &arms.morphs {
        let max = m.deltas.iter().map(|(_, d)| glam::Vec3::from(*d).length()).fold(0.0f32, f32::max);
        let bones: std::collections::BTreeSet<_> = m.deltas.iter().map(|(v, _)| {
            let vx = &arms.mesh.vertices[*v as usize];
            let k = (0..4).max_by_key(|&k| vx.weights[k]).unwrap();
            arms.mesh.bones[vx.bones[k] as usize].name.clone()
        }).collect();
        eprintln!("{} {} verts, max delta {max:.2}, bones {bones:?}", m.name, m.deltas.len());
        assert!(m.deltas.iter().all(|(v, _)| (*v as usize) < arms.mesh.vertices.len()), "{}", m.name);
    }
}

/// TdMove_Disarm's clips come from the one-handed set, on Faith's bones.
#[test]
fn disarm_clips_load() {
    let Some(a) = arms() else { return };
    for n in ["SnatchFwd", "SnatchFwd2", "SnatchFwd3", "SnatchBack", "SnatchFail"] {
        let s = a.anims.sequences.get(n).unwrap_or_else(|| panic!("{n}"));
        eprintln!("{n} {:.2} s", s.length);
        assert_eq!(s.tracks.len(), a.anims.bones.len());
        let g = pose_of(&a, n, s.length * 0.5);
        assert!(g.iter().all(|m| m.is_finite()), "{n}");
        // The hands reach out and back: the tracks landed on Faith's bones.
        let moved = pos(&a, &pose_of(&a, n, 0.0), "RightHand").distance(pos(&a, &g, "RightHand"));
        assert!(moved > 5.0, "{n}: right hand moved {moved}");
    }
}

/// The disarm's victim side: the patrol cop, its four clips (as Faith's, by name), facing the
/// way Faith's own mesh does (toes ahead along -Z, the view's forward).
#[test]
fn the_cop_and_its_disarm_clips() {
    let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
    let cop = FaithArms::load_character(Path::new(&dir), VICTIM_PACKAGE, VICTIM_MESH, VICTIM_ANIMS, VICTIM_SET).unwrap();
    for n in ["SnatchFwd", "SnatchFwd2", "SnatchFwd3", "SnatchBack"] {
        let s = cop.anims.sequences.get(n).unwrap_or_else(|| panic!("{n}"));
        assert!(s.length > 1.5, "{n} {}", s.length);
        for t in &s.tracks {
            for q in &t.rotations {
                assert!(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] <= 1.001, "{n}");
            }
        }
    }
    let rest = Pose::rest(&cop.mesh);
    let mut g = vec![];
    pose::globals(&cop.mesh, &rest, &mut g);
    let at = |n: &str| pose::to_view(g[cop.bone(n).unwrap()].w_axis.truncate());
    let toes = (at("LeftToeBase") - at("LeftFoot")) + (at("RightToeBase") - at("RightFoot"));
    let head = at("Head") - at("LeftFoot");
    eprintln!("cop toes {toes} head {head}");
    assert!(toes.z < 0.0 && toes.z.abs() > toes.x.abs(), "toes {toes}");
    assert!(head.y > 1.4, "head {head}");
}
