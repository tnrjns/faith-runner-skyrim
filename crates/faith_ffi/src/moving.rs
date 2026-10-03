//! What moves in the host's world: doors, gates, drawbridges, lifts. Read again every frame
//! (the rest of the collision only every so often), and collided with together with it.

use glam::Vec3;

use crate::{guard, handle, Faith};

/// The moving collision now: `count` triangles, nine floats each (host frame and units), like
/// faith_set_world. Replaces the last; an empty list clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_set_moving(h: *mut Faith, tris: *const f32, count: u32) {
    let Some(f) = (unsafe { handle(h) }) else { return };
    guard((), || {
        let data: &[f32] = if tris.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(tris, count as usize * 9) } };
        let local: Vec<[Vec3; 3]> = data
            .chunks_exact(9)
            .map(|t| [0, 3, 6].map(|i| f.local_point(Vec3::new(t[i], t[i + 1], t[i + 2]))))
            .filter(|t| t.iter().all(|p| p.is_finite()) && (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-12)
            .collect();
        f.moving = faith_move::MeshWorld::new(local, vec![]);
    });
}
