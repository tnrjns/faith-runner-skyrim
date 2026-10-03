//! Posing: sample and blend sequences, build bone matrices, CPU-skin the mesh.
//!
//! Everything here works in Mirror's Edge mesh space (Unreal units, UE3 axes).
//! [`to_view`] converts a camera-relative point into a right-handed, Y-up,
//! −Z-forward view space in metres, flipping handedness.

use glam::{Mat4, Quat, Vec3};

use crate::anim::{AnimSet, Sequence};
use crate::skeletal::SkeletalMesh;

/// Local (parent-space) transforms for every mesh bone.
#[derive(Clone, Debug)]
pub struct Pose {
    pub rot: Vec<Quat>,
    pub pos: Vec<Vec3>,
}

impl Pose {
    pub fn rest(mesh: &SkeletalMesh) -> Self {
        Pose {
            rot: mesh.bones.iter().map(|b| Quat::from_array(b.rotation).normalize()).collect(),
            pos: mesh.bones.iter().map(|b| Vec3::from(b.translation)).collect(),
        }
    }

    /// Blend `other` into self by `w` (0 = self, 1 = other).
    pub fn blend_toward(&mut self, other: &Pose, w: f32) {
        if w <= 0.0 {
            return;
        }
        let w = w.min(1.0);
        for i in 0..self.rot.len() {
            let mut b = other.rot[i];
            if self.rot[i].dot(b) < 0.0 {
                b = -b;
            }
            self.rot[i] = self.rot[i].lerp(b, w).normalize();
            self.pos[i] = self.pos[i].lerp(other.pos[i], w);
        }
    }
}

/// Maps animation tracks to mesh bones by name.
#[derive(Clone, Debug)]
pub struct TrackMap {
    /// For each mesh bone, the track index in the AnimSet (if any).
    pub track_for_bone: Vec<Option<usize>>,
}

impl TrackMap {
    pub fn new(mesh: &SkeletalMesh, set: &AnimSet) -> Self {
        let track_for_bone =
            mesh.bones.iter().map(|b| set.bones.iter().position(|n| n.eq_ignore_ascii_case(&b.name))).collect();
        TrackMap { track_for_bone }
    }
}

/// Sample `seq` at `time` seconds into `out` (bones without a track keep `rest`).
pub fn sample(seq: &Sequence, map: &TrackMap, rest: &Pose, time: f32, looping: bool, out: &mut Pose) {
    let frames = seq.frames.max(1) as f32;
    let len = seq.length.max(1e-4);
    let t = if looping {
        time.rem_euclid(len)
    } else {
        time.clamp(0.0, len * (frames - 1.0).max(0.0) / frames)
    };
    let f = t / len * frames;
    for (bone, track) in map.track_for_bone.iter().enumerate() {
        let Some(tr) = track.and_then(|i| seq.tracks.get(i)) else {
            out.rot[bone] = rest.rot[bone];
            out.pos[bone] = rest.pos[bone];
            continue;
        };
        out.rot[bone] = match tr.rotations.len() {
            0 => rest.rot[bone],
            1 => Quat::from_array(tr.rotations[0]).normalize(),
            n => {
                let (i0, i1, a) = keys(f, n, looping);
                let q0 = Quat::from_array(tr.rotations[i0]);
                let mut q1 = Quat::from_array(tr.rotations[i1]);
                if q0.dot(q1) < 0.0 {
                    q1 = -q1;
                }
                q0.lerp(q1, a).normalize()
            }
        };
        out.pos[bone] = match tr.translations.len() {
            0 => rest.pos[bone],
            1 => Vec3::from(tr.translations[0]),
            n => {
                let (i0, i1, a) = keys(f, n, looping);
                Vec3::from(tr.translations[i0]).lerp(Vec3::from(tr.translations[i1]), a)
            }
        };
    }
}

fn keys(f: f32, n: usize, looping: bool) -> (usize, usize, f32) {
    let i0 = (f.floor() as usize).min(n - 1);
    let a = f - f.floor();
    let i1 = if i0 + 1 < n { i0 + 1 } else if looping { 0 } else { n - 1 };
    (i0, i1, a)
}

/// World-from-bone matrices (mesh space).
pub fn globals(mesh: &SkeletalMesh, pose: &Pose, out: &mut Vec<Mat4>) {
    out.clear();
    for (i, b) in mesh.bones.iter().enumerate() {
        let local = Mat4::from_rotation_translation(pose.rot[i], pose.pos[i]);
        let g = if i == 0 { local } else { out[b.parent] * local };
        out.push(g);
    }
}

/// Precomputed data for skinning into a chosen bone's space.
pub struct Skinner {
    inv_rest: Vec<Mat4>,
}

impl Skinner {
    pub fn new(mesh: &SkeletalMesh) -> Self {
        let mut g = vec![];
        globals(mesh, &Pose::rest(mesh), &mut g);
        Skinner { inv_rest: g.iter().map(|m| m.inverse()).collect() }
    }

    /// Skin every vertex and express it in `frame`'s space (e.g. the camera
    /// bone), converted to view space. Normals are written alongside.
    pub fn skin_into(
        &self,
        mesh: &SkeletalMesh,
        bone_globals: &[Mat4],
        frame: Mat4,
        positions: &mut Vec<[f32; 3]>,
        normals: &mut Vec<[f32; 3]>,
    ) {
        self.skin_full(mesh, bone_globals, frame, positions, normals, None)
    }

    /// Like [`Skinner::skin_into`], also producing tangents for normal
    /// mapping (xyz, w = bitangent sign, as Bevy/glTF expect).
    pub fn skin_full(
        &self,
        mesh: &SkeletalMesh,
        bone_globals: &[Mat4],
        frame: Mat4,
        positions: &mut Vec<[f32; 3]>,
        normals: &mut Vec<[f32; 3]>,
        tangents: Option<&mut Vec<[f32; 4]>>,
    ) {
        self.skin_morphed(mesh, bone_globals, frame, &[], positions, normals, tangents)
    }

    /// [`Self::skin_full`] with morph target deltas (mesh space, per vertex; empty = none)
    /// added to the bind pose first.
    #[allow(clippy::too_many_arguments)]
    pub fn skin_morphed(
        &self,
        mesh: &SkeletalMesh,
        bone_globals: &[Mat4],
        frame: Mat4,
        morph: &[Vec3],
        positions: &mut Vec<[f32; 3]>,
        normals: &mut Vec<[f32; 3]>,
        mut tangents: Option<&mut Vec<[f32; 4]>>,
    ) {
        let to_frame = frame.inverse();
        let skin: Vec<Mat4> =
            bone_globals.iter().zip(&self.inv_rest).map(|(g, inv)| to_frame * *g * *inv).collect();
        positions.clear();
        normals.clear();
        if let Some(t) = tangents.as_deref_mut() {
            t.clear();
        }
        for (vi, v) in mesh.vertices.iter().enumerate() {
            let p = Vec3::from(v.position) + morph.get(vi).copied().unwrap_or(Vec3::ZERO);
            let n = Vec3::from(v.normal);
            let tx = Vec3::from(v.tangent);
            let ty = Vec3::from(v.bitangent);
            let mut acc_p = Vec3::ZERO;
            let mut acc_n = Vec3::ZERO;
            let mut acc_t = Vec3::ZERO;
            let mut acc_b = Vec3::ZERO;
            for k in 0..4 {
                let w = v.weights[k];
                if w == 0 {
                    continue;
                }
                let w = w as f32 / 255.0;
                let m = &skin[v.bones[k] as usize];
                acc_p += w * m.transform_point3(p);
                acc_n += w * m.transform_vector3(n);
                if tangents.is_some() {
                    acc_t += w * m.transform_vector3(tx);
                    acc_b += w * m.transform_vector3(ty);
                }
            }
            positions.push(to_view(acc_p).to_array());
            let n = (-acc_n).normalize_or_zero();
            normals.push(n.to_array());
            if let Some(out) = tangents.as_deref_mut() {
                // Same basis the game's shader uses (x along TangentX, y along
                // TangentY), carried through the view flip; w picks the
                // bitangent's side so Bevy rebuilds exactly TangentY.
                let t = (-acc_t).normalize_or_zero();
                let b = -acc_b;
                let w = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
                out.push([t.x, t.y, t.z, w]);
            }
        }
    }
}

/// Triangle indices for one section, ready for a right-handed renderer with
/// counter-clockwise front faces. Mirror's Edge already winds its triangles
/// the other way, so after [`to_view`]'s handedness flip they need no change.
pub fn section_indices(mesh: &SkeletalMesh, section: usize) -> Vec<u32> {
    let s = &mesh.sections[section];
    let first = s.first_index as usize;
    mesh.indices[first..first + 3 * s.triangles as usize].to_vec()
}

/// Mirror's Edge camera-bone space (forward +Z, up −Y, right −X, cm) to a
/// right-handed view space (right +X, up +Y, forward −Z, metres).
pub fn to_view(p: Vec3) -> Vec3 {
    -p * 0.01
}
