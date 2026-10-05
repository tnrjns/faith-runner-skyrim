//! Faith's own first-person body (the app's: SK_UpperBody arms and torso, SK_LowerBody legs),
//! skinned each frame for the host to draw, with its textures. Vertices come in camera space,
//! so the host only needs a projection.

use std::ffi::c_char;

use glam::{Mat4, Quat, Vec3};
use me_assets::pose::section_indices;
use me_assets::{MaterialSlot, SkeletalMesh, Skinner};

use crate::{guard, handle, Faith};

/// One skinned vertex: camera space (host units; x right, y up, z back towards the viewer),
/// its normal and tangent there (w: which side the bitangent is, bitangent = w * n x t), uv.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],
    pub uv: [f32; 2],
}

/// A run of triangles with one material.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithSection {
    pub first_index: u32,
    pub index_count: u32,
    pub material: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithPartInfo {
    pub vertex_count: u32,
    pub index_count: u32,
    pub section_count: u32,
    pub material_count: u32,
    /// The legs (placed with the legs' facing; drawn in the world in Mirror's Edge).
    pub legs: u8,
    pub _pad: [u8; 3],
}

pub(crate) struct Part {
    legs: bool,
    skinner: Skinner,
    indices: Vec<u32>,
    sections: Vec<FaithSection>,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<[f32; 4]>,
    morph: Vec<Vec3>,
}

impl Part {
    fn new(mesh: &SkeletalMesh, legs: bool) -> Part {
        let mut indices = vec![];
        let mut sections = vec![];
        // Every section, the lower body's top included: it's what you see when you look down.
        for (k, s) in mesh.sections.iter().enumerate() {
            let idx = section_indices(mesh, k);
            sections.push(FaithSection { first_index: indices.len() as u32, index_count: idx.len() as u32, material: s.material as u32 });
            indices.extend(idx);
        }
        Part { legs, skinner: Skinner::new(mesh), indices, sections, positions: vec![], normals: vec![], tangents: vec![], morph: vec![] }
    }
}

pub(crate) fn parts(arms: &me_assets::FaithArms) -> Vec<Part> {
    let mut v = vec![Part::new(&arms.mesh, false)];
    if let Some((legs, _)) = &arms.legs {
        v.push(Part::new(legs, true));
    }
    v
}

fn mesh_of(arms: &me_assets::FaithArms, legs: bool) -> (&SkeletalMesh, &[MaterialSlot]) {
    match (&arms.legs, legs) {
        (Some((m, s)), true) => (m, s),
        _ => (&arms.mesh, &arms.materials),
    }
}

/// How many parts Faith's body has (arms and torso, legs): 0 without Mirror's Edge.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_body_parts(h: *mut Faith) -> u32 {
    unsafe { handle(h) }.map_or(0, |f| f.parts.len() as u32)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_body_part(h: *mut Faith, part: u32, out: *mut FaithPartInfo) -> u8 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    let (Some(p), Some((arms, _)), Some(o)) = (f.parts.get(part as usize), &f.anim, unsafe { out.as_mut() }) else { return 0 };
    let (mesh, slots) = mesh_of(arms, p.legs);
    *o = FaithPartInfo {
        vertex_count: mesh.vertices.len() as u32,
        index_count: p.indices.len() as u32,
        section_count: p.sections.len() as u32,
        material_count: slots.len() as u32,
        legs: p.legs as u8,
        _pad: [0; 3],
    };
    1
}

/// The part's triangles (`index_count`, counter-clockwise front faces in camera space).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_body_indices(h: *mut Faith, part: u32) -> *const u32 {
    unsafe { handle(h) }.and_then(|f| f.parts.get(part as usize)).map_or(std::ptr::null(), |p| p.indices.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_body_sections(h: *mut Faith, part: u32) -> *const FaithSection {
    unsafe { handle(h) }.and_then(|f| f.parts.get(part as usize)).map_or(std::ptr::null(), |p| p.sections.as_ptr())
}

/// A material's texture, RGBA8 (`width` x `height`): kind 0 colour (sRGB), 1 normal map,
/// 2 specular. Null if it has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_body_texture(h: *mut Faith, part: u32, material: u32, kind: u32, width: *mut u32, height: *mut u32) -> *const u8 {
    let Some(f) = (unsafe { handle(h) }) else { return std::ptr::null() };
    let (Some(p), Some((arms, _))) = (f.parts.get(part as usize), &f.anim) else { return std::ptr::null() };
    let (_, slots) = mesh_of(arms, p.legs);
    let Some(slot) = slots.get(material as usize) else { return std::ptr::null() };
    let tex = match kind {
        0 => slot.diffuse.as_ref(),
        1 => slot.normal.as_ref(),
        _ => slot.specular.as_ref(),
    };
    let Some(t) = tex else { return std::ptr::null() };
    unsafe {
        if let Some(w) = width.as_mut() {
            *w = t.width;
        }
        if let Some(hh) = height.as_mut() {
            *hh = t.height;
        }
    }
    t.pixels.as_ptr()
}

/// The material's name, for the log.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_body_material_name(h: *mut Faith, part: u32, material: u32) -> *const c_char {
    let Some(f) = (unsafe { handle(h) }) else { return c"".as_ptr() };
    let (Some(p), Some((arms, _))) = (f.parts.get(part as usize), &f.anim) else { return c"".as_ptr() };
    let (_, slots) = mesh_of(arms, p.legs);
    f.names.push(std::ffi::CString::new(slots.get(material as usize).map_or("", |s| s.material.as_str())).unwrap_or_default());
    if f.names.len() > 64 {
        f.names.remove(0);
    }
    f.names.last().unwrap().as_ptr()
}

/// Where one of her bones is this frame (after `faith_step`), in camera space as
/// faith_body_skin's vertices: for what the host puts in her hands. 0: no such bone or no pose.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_body_bone(h: *mut Faith, name: *const c_char, out: *mut crate::FaithXform) -> u8 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        let (Some(frame), Some((arms, rig)), Some(o)) = (f.last, &f.anim, unsafe { out.as_mut() }) else { return 0 };
        if name.is_null() {
            return 0;
        }
        let name = unsafe { std::ffi::CStr::from_ptr(name) }.to_string_lossy();
        let Some(b) = arms.bone(&name) else { return 0 };
        let Some(g) = rig.driver.globals.get(b) else { return 0 };
        // Mesh space is the view's flipped and in centimetres: a flip leaves rotations as they are.
        let to_cam = frame.cam_rot.inverse();
        let rot = (to_cam * frame.body_rot * Quat::from_mat4(g)).normalize();
        let pos = to_cam * (frame.body_rot * me_assets::pose::to_view(g.w_axis.truncate()) + frame.origin - frame.cam_pos) * f.frame.units_per_meter;
        *o = crate::FaithXform { rot: rot.to_array(), pos: pos.to_array(), scale: 1.0 };
        1
    })
}

/// This frame's pose of a part (after `faith_step`), `vertex_count` vertices into `out`, in
/// the camera's space. Returns 1 if skinned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_body_skin(h: *mut Faith, part: u32, out: *mut FaithVertex) -> u8 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        let (Some(frame), Some((arms, rig))) = (f.last, &f.anim) else { return 0 };
        let Some(p) = f.parts.get_mut(part as usize) else { return 0 };
        if out.is_null() {
            return 0;
        }
        let (mesh, _) = mesh_of(arms, p.legs);
        let globals = &rig.driver.globals;
        if p.legs || arms.morphs.is_empty() {
            p.skinner.skin_full(mesh, globals, Mat4::IDENTITY, &mut p.positions, &mut p.normals, Some(&mut p.tangents));
        } else {
            rig.driver.forearm_morphs(arms, &mut p.morph);
            p.skinner.skin_morphed(mesh, globals, Mat4::IDENTITY, &p.morph, &mut p.positions, &mut p.normals, Some(&mut p.tangents));
        }
        // Body space -> world -> camera, in the host's units.
        let place: Quat = if p.legs { frame.legs_rot } else { frame.body_rot };
        let to_cam = frame.cam_rot.inverse();
        let rot = to_cam * place;
        let shift = to_cam * (frame.origin - frame.cam_pos);
        let k = f.frame.units_per_meter;
        let out = unsafe { std::slice::from_raw_parts_mut(out, mesh.vertices.len()) };
        for (i, o) in out.iter_mut().enumerate() {
            let pos = (rot * Vec3::from(p.positions[i]) + shift) * k;
            let n = rot * Vec3::from(p.normals[i]);
            let t = p.tangents[i];
            let tt = rot * Vec3::new(t[0], t[1], t[2]);
            *o = FaithVertex { pos: pos.to_array(), normal: n.to_array(), tangent: [tt.x, tt.y, tt.z, t[3]], uv: mesh.vertices[i].uv };
        }
        1
    })
}
