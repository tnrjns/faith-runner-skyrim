//! SkeletalMesh (skeleton + LOD0 geometry) as stored by Mirror's Edge.
//!
//! Layout after the property block (all verified against SK_UpperBody):
//! i32 (ME-specific, 1) · bounds (7 f32) · materials · origin · rot-origin ·
//! bones[52 bytes] · skeletal depth · LOD models. In LOD0: sections are 10 bytes
//! (u16 material, u16 chunk, u32 first index, u16 triangles); two index lists;
//! active bones; per-triangle shadow flags; chunks of rigid (49-byte) and soft
//! (56-byte) vertices with a per-chunk bone map.

use crate::package::{Cursor, Package};
use crate::props::Props;
use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub parent: usize,
    /// Rest rotation (x, y, z, w) in parent space, UE3 mesh convention.
    pub rotation: [f32; 4],
    /// Rest translation in parent space, Unreal units (cm).
    pub translation: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Tangent (TangentX) and bitangent (TangentY): the normal map's basis.
    pub tangent: [f32; 3],
    pub bitangent: [f32; 3],
    pub uv: [f32; 2],
    /// Mesh bone indices and weights (0..=255, summing to 255).
    pub bones: [u16; 4],
    pub weights: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct Section {
    pub material: u16,
    pub first_index: u32,
    pub triangles: u32,
}

#[derive(Clone, Debug)]
pub struct SkeletalMesh {
    pub name: String,
    /// Package object indices of the material slots.
    pub materials: Vec<i32>,
    pub bones: Vec<Bone>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub sections: Vec<Section>,
}

impl SkeletalMesh {
    pub fn read(pkg: &Package, name: &str) -> Result<Self> {
        let e = pkg
            .find("SkeletalMesh", name)
            .ok_or_else(|| Error::Missing(format!("SkeletalMesh {name}")))?;
        let props = Props::read(pkg, e)?;
        let mut c = Cursor::new(&pkg.data, props.end);
        c.i32()?; // ME-specific leading int
        c.skip(28)?; // bounds
        let nmat = c.count(4)?;
        let materials = (0..nmat).map(|_| c.i32()).collect::<Result<Vec<_>>>()?;
        c.skip(12 + 12)?; // origin, rotation origin

        let nb = c.count(52)?;
        let mut bones = Vec::with_capacity(nb);
        for i in 0..nb {
            let name = crate::package::read_name(&pkg.names, &mut c)?;
            let _flags = c.i32()?;
            let rotation = [c.f32()?, c.f32()?, c.f32()?, c.f32()?];
            let translation = c.vec3()?;
            let _children = c.i32()?;
            let parent = c.i32()?;
            let _color = c.i32()?;
            if parent < 0 || (parent as usize >= nb) || (i > 0 && parent as usize >= i) {
                return Err(Error::Format(format!("bone {name} has bad parent {parent}")));
            }
            bones.push(Bone { name, parent: parent as usize, rotation, translation });
        }
        let _depth = c.i32()?;
        let nlod = c.count(1)?;
        if nlod == 0 {
            return Err(Error::Format("mesh has no LODs".into()));
        }

        // ---- LOD 0
        let nsec = c.count(10)?;
        let mut sections = Vec::with_capacity(nsec);
        for _ in 0..nsec {
            let material = c.u16()?;
            let _chunk = c.u16()?;
            let first_index = c.u32()?;
            let triangles = c.u16()? as u32;
            sections.push(Section { material, first_index, triangles });
        }
        let elem = c.i32()? as usize;
        let nidx = c.count(elem.max(1))?;
        let mut indices = Vec::with_capacity(nidx);
        for _ in 0..nidx {
            indices.push(match elem {
                2 => c.u16()? as u32,
                4 => c.u32()?,
                _ => return Err(Error::Format(format!("index size {elem}"))),
            });
        }
        let n = c.count(2)?;
        c.skip(n * 2)?; // shadow indices
        let n = c.count(2)?;
        c.skip(n * 2)?; // active bone indices
        let n = c.count(1)?;
        c.skip(n)?; // shadow triangle double-sided flags

        let nchunks = c.count(4)?;
        let mut vertices = Vec::new();
        for _ in 0..nchunks {
            let _base = c.i32()?;
            let nr = c.count(49)?;
            let rigid_at = c.pos;
            c.skip(nr * 49)?;
            let ns = c.count(56)?;
            let soft_at = c.pos;
            c.skip(ns * 56)?;
            let nmap = c.count(2)?;
            let bonemap = (0..nmap).map(|_| c.u16()).collect::<Result<Vec<_>>>()?;
            c.skip(12)?; // num rigid, num soft, max influences
            let map = |b: u8| -> Result<u16> {
                bonemap.get(b as usize).copied().ok_or_else(|| Error::Format(format!("bone map index {b}")))
            };
            for k in 0..nr {
                let mut v = Cursor::new(&pkg.data, rigid_at + 49 * k);
                let (position, [tangent, bitangent, normal], uv) = read_vertex_common(&mut v)?;
                let bone = pkg.data[rigid_at + 49 * k + 48];
                vertices.push(Vertex { position, normal, tangent, bitangent, uv, bones: [map(bone)?, 0, 0, 0], weights: [255, 0, 0, 0] });
            }
            for k in 0..ns {
                let at = soft_at + 56 * k;
                let mut v = Cursor::new(&pkg.data, at);
                let (position, [tangent, bitangent, normal], uv) = read_vertex_common(&mut v)?;
                let b = &pkg.data[at + 48..at + 52];
                let w = &pkg.data[at + 52..at + 56];
                vertices.push(Vertex {
                    position,
                    normal,
                    tangent,
                    bitangent,
                    uv,
                    bones: [map(b[0])?, map(b[1])?, map(b[2])?, map(b[3])?],
                    weights: [w[0], w[1], w[2], w[3]],
                });
            }
        }
        if let Some(&max) = indices.iter().max() {
            if max as usize >= vertices.len() {
                return Err(Error::Format(format!("index {max} beyond {} vertices", vertices.len())));
            }
        }
        Ok(SkeletalMesh { name: name.to_string(), materials, bones, vertices, indices, sections })
    }
}

/// Position, packed tangent basis (TangentX, TangentY, TangentZ = normal), UV.
fn read_vertex_common(c: &mut Cursor) -> Result<([f32; 3], [[f32; 3]; 3], [f32; 2])> {
    let position = c.vec3()?;
    let mut basis = [[0.0; 3]; 3];
    for b in &mut basis {
        let z = c.bytes(4)?;
        *b = [z[0] as f32 / 127.5 - 1.0, z[1] as f32 / 127.5 - 1.0, z[2] as f32 / 127.5 - 1.0];
    }
    let uv = [c.f32()?, c.f32()?];
    Ok((position, basis, uv))
}
