//! Fixtures the host marks itself (its own ladders, doors, soft landings), in the host's frame,
//! applied with the next faith_set_world alongside the ones found in its collision.

use faith_move::{Aabb, Fixture};
use glam::Vec3;

use crate::{guard, handle, Faith, FaithVec3};

/// `kind` 0 ladder: `a` its foot on the wall's face, `n` out from the wall, `top` the height she
/// climbs out onto; `flags` 1 a drainpipe, 2 she can climb out over the top. 1 door: the closed
/// door's box `a`..`b`, `n` the side she comes from. 2 soft landing: the pad's box `a`..`b`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithHostFixture {
    pub kind: u32,
    pub flags: u32,
    pub a: FaithVec3,
    pub b: FaithVec3,
    pub n: FaithVec3,
    pub top: f32,
}

/// The host's own fixtures, to go with the next faith_set_world (replaces the last ones).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_set_host_fixtures(h: *mut Faith, f: *const FaithHostFixture, count: u32) {
    let Some(faith) = (unsafe { handle(h) }) else { return };
    guard((), || {
        faith.host_fixtures.clear();
        if !f.is_null() {
            faith.host_fixtures.extend_from_slice(unsafe { std::slice::from_raw_parts(f, count as usize) });
        }
    });
}

/// The doors she burst open since the last step (indices into faith_set_host_fixtures' list), up
/// to `max` into `out`; returns how many.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_doors_opened(h: *mut Faith, out: *mut u32, max: u32) -> u32 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        if out.is_null() {
            return f.doors_opened.len() as u32;
        }
        let out = unsafe { std::slice::from_raw_parts_mut(out, max as usize) };
        let n = f.doors_opened.len().min(out.len());
        out[..n].copy_from_slice(&f.doors_opened[..n]);
        n as u32
    })
}

impl Faith {
    /// The host's fixtures in faith_move's frame, each with its index in the host's list (those
    /// that make no fixture are left out).
    pub(crate) fn host_fixtures_local(&self) -> Vec<(u32, Fixture)> {
        let dir = |v: FaithVec3| self.frame.axes.inverse() * Vec3::from(v);
        let local_box = |a: FaithVec3, b: FaithVec3| {
            let (p, q) = (self.local_point(a.into()), self.local_point(b.into()));
            Aabb::new(p.min(q), p.max(q))
        };
        self.host_fixtures
            .iter()
            .enumerate()
            .filter_map(|(i, x)| Some((i as u32, match x.kind {
                0 => {
                    let base = self.local_point(x.a.into());
                    let top = self.local_point(Vec3::new(x.a.x, x.a.y, x.top)).y;
                    let mut normal = dir(x.n);
                    normal.y = 0.0;
                    let normal = normal.normalize_or_zero();
                    if normal == Vec3::ZERO {
                        return None;
                    }
                    Fixture::Ladder(faith_move::Ladder {
                        base,
                        top,
                        normal,
                        pipe: x.flags & 1 != 0,
                        exit: x.flags & 2 != 0,
                    })
                }
                1 => Fixture::Door { b: local_box(x.a, x.b), n: dir(x.n).normalize_or_zero() },
                2 => Fixture::SoftPad { b: local_box(x.a, x.b) },
                _ => return None,
            })))
            .collect()
    }
}
