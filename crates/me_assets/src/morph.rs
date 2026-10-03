//! MorphTarget: per-vertex position deltas on the mesh's LOD0 vertices, added to the bind
//! pose before skinning (UE3 morph nodes). Faith's first-person arms use them to keep the
//! forearm's shape when it twists (`LeftForeArmRollBlend90` and friends).
//!
//! After the properties: LOD count, then per LOD a vertex array (position delta 3 x f32,
//! packed normal delta 4 bytes, source vertex index u16 = 18 bytes) and the base mesh's
//! vertex count.

use crate::package::{Cursor, Package};
use crate::props::Props;
use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct MorphTarget {
    pub name: String,
    /// (mesh vertex, position delta in mesh space).
    pub deltas: Vec<(u32, [f32; 3])>,
    /// How many vertices the mesh it was made for has.
    pub base_vertices: u32,
}

/// Every target of the MorphTargetSet export `set` in `pkg`.
pub fn read_morph_set(pkg: &Package, set: &str) -> Result<Vec<MorphTarget>> {
    let e = pkg.find("MorphTargetSet", set).ok_or_else(|| Error::Missing(format!("MorphTargetSet {set}")))?;
    let props = Props::read(pkg, e)?;
    let mut out = vec![];
    for t in props.object_array(pkg, "Targets") {
        if t <= 0 {
            continue;
        }
        out.push(read_morph_target(pkg, t as usize - 1)?);
    }
    Ok(out)
}

pub fn read_morph_target(pkg: &Package, export: usize) -> Result<MorphTarget> {
    let e = pkg.exports.get(export).ok_or_else(|| Error::Missing(format!("export {export}")))?;
    let props = Props::read(pkg, e)?;
    let mut c = Cursor::new(&pkg.data, props.end);
    if c.i32()? < 1 {
        return Err(Error::Format(format!("{}: no LODs", e.name)));
    }
    let n = c.count(18)?;
    let mut deltas = Vec::with_capacity(n);
    for _ in 0..n {
        let d = c.vec3()?;
        c.skip(4)?; // normal delta
        let src = c.u16()? as u32;
        deltas.push((src, d));
    }
    let base_vertices = c.i32()?.max(0) as u32;
    Ok(MorphTarget { name: e.name.clone(), deltas, base_vertices })
}
