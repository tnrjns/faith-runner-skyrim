//! Ziplines, swing poles and balance beams in the host's own world: the host passes the thin
//! capsules and long thin boxes in its collision, and the ones that work as Mirror's Edge's
//! fixtures are found again each time the collision is (faith_move::fixtures).

use crate::{guard, handle, Faith, FaithVec3};

/// A thin, long piece of the host's collision (host frame and units): a capsule's centre line
/// and radius, or the middle of a box's top and half its width.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithFixtureCandidate {
    pub a: FaithVec3,
    pub b: FaithVec3,
    pub thickness: f32,
    pub capsule: u32,
}

/// A fixture found (host frame): `kind` 0 zipline (a the high end), 1 swing pole, 2 balance beam,
/// 3 drainpipe (a its foot, b the top she climbs out onto).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FaithFixture {
    pub kind: u32,
    pub a: FaithVec3,
    pub b: FaithVec3,
}

/// The candidates to go with the next faith_set_world (replaces the last ones).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_set_fixture_candidates(h: *mut Faith, c: *const FaithFixtureCandidate, count: u32) {
    let Some(f) = (unsafe { handle(h) }) else { return };
    guard((), || {
        f.host_candidates.clear();
        if !c.is_null() {
            f.host_candidates.extend_from_slice(unsafe { std::slice::from_raw_parts(c, count as usize) });
        }
    });
}

/// The fixtures found in the current world, up to `max` into `out` (NULL to count).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faith_world_fixtures(h: *mut Faith, out: *mut FaithFixture, max: u32) -> u32 {
    let Some(f) = (unsafe { handle(h) }) else { return 0 };
    guard(0, || {
        let found: Vec<FaithFixture> = f
            .world
            .fixtures
            .iter()
            .filter_map(|x| match *x {
                faith_move::Fixture::ZipLine { a, b } => Some((0, a, b)),
                faith_move::Fixture::SwingPole { a, b } => Some((1, a, b)),
                faith_move::Fixture::Beam { a, b } => Some((2, a, b)),
                faith_move::Fixture::Ladder(l) => Some((3, l.base, glam::Vec3::new(l.base.x, l.top, l.base.z))),
                _ => None,
            })
            .map(|(kind, a, b)| FaithFixture { kind, a: f.host(a).into(), b: f.host(b).into() })
            .collect();
        if out.is_null() {
            return found.len() as u32;
        }
        let out = unsafe { std::slice::from_raw_parts_mut(out, max as usize) };
        let n = found.len().min(out.len());
        out[..n].copy_from_slice(&found[..n]);
        n as u32
    })
}
