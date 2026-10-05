//! Ziplines, swing poles and balance beams found in a host game's own collision, where nothing
//! marks them: thin capsules and long thin boxes (`Candidate`) that work as one.
//!
//! - **Zipline:** a cable (2.5 cm across, Mirror's Edge's are thinner still) at least 6 m long,
//!   sloping 4-30 degrees, with room to hang under it and somewhere to jump to it from.
//! - **Swing pole:** a bar at most 10 cm thick and 1-8 m long, level, with room to swing under
//!   and both sides of it, and nothing within 2.2 m below (it's not a handrail).
//! - **Balance beam:** level, at least 2.5 m long, 10-50 cm wide, with a drop of 1.5 m or more on
//!   both sides and headroom above (a plank across a gap, not a walkway).
//! - **Drainpipe** (TdLadderVolume, LT_Pipe): upright, at most 10 cm thick and 2.5 m or more
//!   long, against a wall (within 30 cm of it) with room to climb in front, up to a top she can
//!   climb out onto (a floor behind it, 1.5 m or more up and no higher than the pipe).

use glam::Vec3;

use crate::world::{tops_below, Aabb, Fixture, MeshWorld, World};

/// A thin, long piece of the host's collision. `a` to `b` is its centre line for a capsule,
/// the middle of its top for a box; `thickness` is a capsule's radius or half a box's width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub a: Vec3,
    pub b: Vec3,
    pub thickness: f32,
    pub capsule: bool,
}

fn horiz(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// The highest floor under `p` within `depth` (None: nothing).
fn floor_below(world: &MeshWorld, p: Vec3, depth: f32) -> Option<f32> {
    tops_below(world, p, 0.15, p.y - 0.05, p.y - depth).first().copied()
}

/// A box round `centre` with half sizes `half` (axis-aligned).
fn free(world: &MeshWorld, centre: Vec3, half: Vec3) -> bool {
    world.is_free(&Aabb::new(centre - half, centre + half))
}

fn zipline(world: &MeshWorld, c: &Candidate) -> Option<Fixture> {
    let (hi, lo) = if c.a.y >= c.b.y { (c.a, c.b) } else { (c.b, c.a) };
    let d = lo - hi;
    let run = horiz(d).length();
    let slope = (-d.y).atan2(run).to_degrees();
    if c.thickness > 0.06 || d.length() < 6.0 || !(4.0..=30.0).contains(&slope) {
        return None;
    }
    // Room to hang under it along the first half (the low end comes down near the ground,
    // where you let go).
    for k in [0.15, 0.3, 0.45] {
        let p = hi + d * k;
        if !free(world, p - Vec3::Y * 1.0, Vec3::new(0.25, 0.75, 0.25)) {
            return None;
        }
    }
    // Something to jump from near the top: a floor 1-3.5 m under the line, a little way on.
    let start = hi + d * (1.5 / d.length());
    let floor = floor_below(world, start, 3.5)?;
    if start.y - floor < 1.0 {
        return None;
    }
    Some(Fixture::ZipLine { a: hi, b: lo })
}

fn level(c: &Candidate) -> Option<(Vec3, f32)> {
    let d = c.b - c.a;
    let run = horiz(d).length();
    if run < 1e-3 || (d.y.abs() / run) > 4f32.to_radians().tan() {
        return None;
    }
    Some((d / d.length(), d.length()))
}

fn swing_pole(world: &MeshWorld, c: &Candidate) -> Option<Fixture> {
    let (u, len) = level(c)?;
    if c.thickness > 0.1 || !(1.0..=8.0).contains(&len) {
        return None;
    }
    let mid = (c.a + c.b) * 0.5;
    let side = Vec3::new(-u.z, 0.0, u.x);
    // Nothing within 2.2 m below (a handrail has its walkway right under it).
    if !free(world, mid - Vec3::Y * 1.175, Vec3::new(0.15, 1.025, 0.15)) {
        return None;
    }
    // Room to hang and swing: under the bar, and a body's length either side of it.
    if !free(world, mid - Vec3::Y * 1.2, Vec3::new(0.3, 0.95, 0.3)) {
        return None;
    }
    for s in [-1.2, 1.2] {
        if !free(world, mid + side * s - Vec3::Y * 1.0, Vec3::new(0.3, 0.8, 0.3)) {
            return None;
        }
    }
    Some(Fixture::SwingPole { a: c.a, b: c.b })
}

fn beam(world: &MeshWorld, c: &Candidate) -> Option<Fixture> {
    let (u, len) = level(c)?;
    let width = c.thickness * 2.0;
    if len < 2.5 || !(0.1..=0.5).contains(&width) {
        return None;
    }
    // Its top: a capsule's is its radius above the centre line.
    let lift = if c.capsule { Vec3::Y * c.thickness } else { Vec3::ZERO };
    let (a, b) = (c.a + lift, c.b + lift);
    let mid = (a + b) * 0.5;
    let side = Vec3::new(-u.z, 0.0, u.x);
    // A drop both sides: nothing within 1.5 m under either side of it.
    for s in [-1.0, 1.0] {
        let p = mid + side * s * (c.thickness + 0.4);
        if !free(world, p - Vec3::Y * 0.8, Vec3::new(0.15, 0.7, 0.15)) {
            return None;
        }
    }
    // Headroom to walk it.
    if !free(world, mid + Vec3::Y * 1.05, Vec3::new(0.2, 0.95, 0.2)) {
        return None;
    }
    Some(Fixture::Beam { a, b })
}

fn drainpipe(world: &MeshWorld, c: &Candidate) -> Option<Fixture> {
    let (lo, hi) = if c.a.y <= c.b.y { (c.a, c.b) } else { (c.b, c.a) };
    let d = hi - lo;
    if c.thickness > 0.1 || d.length() < 2.5 || horiz(d).length() > d.y * 5f32.to_radians().tan() {
        return None;
    }
    let mid = (lo + hi) * 0.5;
    // The wall behind it: the nearest solid round the pipe within 30 cm, with room on the
    // other side.
    let mut best: Option<(f32, Vec3)> = None;
    for k in 0..16 {
        let a = k as f32 * std::f32::consts::TAU / 16.0;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let start = mid + dir * (c.thickness + 0.02);
        let Some(h) = world.sweep(Vec3::new(0.01, 0.3, 0.01), start, dir * 0.3) else { continue };
        if !free(world, mid - dir * 0.62, Vec3::new(0.3, 0.8, 0.3)) {
            continue;
        }
        if best.is_none_or(|b| h.t < b.0) {
            best = Some((h.t, -dir));
        }
    }
    let normal = best?.1;
    // The top to climb out onto: a floor behind it.
    let back = Vec3::new(hi.x, hi.y + 0.2, hi.z) - normal * (c.thickness + 0.35);
    let top = *tops_below(world, back, 0.1, back.y, lo.y + 1.5).first()?;
    Some(Fixture::Ladder(crate::climb::Ladder { base: lo, top, normal, pipe: true, exit: true }))
}

/// The candidates that work as Mirror's Edge's fixtures. Pieces of one bar or cable that meet
/// end to end come as several candidates; each is judged by itself.
pub fn classify(world: &MeshWorld, candidates: &[Candidate]) -> Vec<Fixture> {
    let mut out: Vec<Fixture> = vec![];
    for c in candidates {
        if !(c.a.is_finite() && c.b.is_finite() && c.thickness.is_finite()) {
            continue;
        }
        let f = zipline(world, c).or_else(|| swing_pole(world, c)).or_else(|| beam(world, c)).or_else(|| drainpipe(world, c));
        let Some(f) = f else { continue };
        // The same thing twice (overlapping shapes): keep one.
        let key = |f: &Fixture| match *f {
            Fixture::ZipLine { a, b } | Fixture::SwingPole { a, b } | Fixture::Beam { a, b } => (a + b) * 0.5,
            Fixture::Ladder(l) => l.base,
            _ => Vec3::ZERO,
        };
        if out.iter().any(|o| std::mem::discriminant(o) == std::mem::discriminant(&f) && key(o).distance(key(&f)) < 0.3) {
            continue;
        }
        out.push(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ground(y: f32) -> Vec<[Vec3; 3]> {
        MeshWorld::oriented_box(Vec3::new(0.0, y - 0.5, 0.0), Vec3::new(40.0, 0.5, 40.0), 0.0)
    }

    fn cap(a: Vec3, b: Vec3, r: f32) -> Candidate {
        Candidate { a, b, thickness: r, capsule: true }
    }

    #[test]
    fn a_long_sloping_cable_is_a_zipline() {
        let w = MeshWorld::new(ground(0.0), vec![]);
        // Mirror's Edge's own: 2.8 m above where you jump from.
        let c = cap(Vec3::new(0.0, 2.8, 0.0), Vec3::new(0.0, 0.8, -15.0), 0.02);
        assert!(matches!(classify(&w, &[c])[..], [Fixture::ZipLine { a, .. }] if a.y == 2.8));
        // Too thick (a sloping beam), too short, or too steep: not one.
        for c in [
            cap(Vec3::new(0.0, 4.0, 0.0), Vec3::new(0.0, 1.5, -15.0), 0.15),
            cap(Vec3::new(0.0, 4.0, 0.0), Vec3::new(0.0, 3.5, -3.0), 0.02),
            cap(Vec3::new(0.0, 9.0, 0.0), Vec3::new(0.0, 1.0, -6.0), 0.02),
        ] {
            assert!(classify(&w, &[c]).is_empty(), "{c:?}");
        }
    }

    #[test]
    fn a_high_bar_is_a_swing_pole_but_a_handrail_isnt() {
        let w = MeshWorld::new(ground(0.0), vec![]);
        let bar = cap(Vec3::new(-2.0, 3.0, 0.0), Vec3::new(2.0, 3.0, 0.0), 0.05);
        assert!(matches!(classify(&w, &[bar])[..], [Fixture::SwingPole { .. }]));
        let rail = cap(Vec3::new(-2.0, 1.0, 0.0), Vec3::new(2.0, 1.0, 0.0), 0.04);
        assert!(classify(&w, &[rail]).is_empty());
    }

    #[test]
    fn a_pipe_up_a_wall_is_a_drainpipe_but_a_lamp_post_isnt() {
        // A building 5 m high, its face at z = 0 (out along +Z), a pipe 15 cm out from it.
        let mut tris = ground(0.0);
        tris.extend(MeshWorld::oriented_box(Vec3::new(0.0, 2.5, -3.0), Vec3::new(4.0, 2.5, 3.0), 0.0));
        let w = MeshWorld::new(tris, vec![]);
        let pipe = cap(Vec3::new(0.0, 0.0, 0.15), Vec3::new(0.0, 5.6, 0.15), 0.05);
        let found = classify(&w, &[pipe]);
        assert!(matches!(found[..], [Fixture::Ladder(l)] if l.pipe && (l.top - 5.0).abs() < 0.05 && l.normal.z > 0.9), "{found:?}");
        // Standing by itself: no wall, no top.
        let post = cap(Vec3::new(10.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), 0.06);
        assert!(classify(&w, &[post]).is_empty());
    }

    #[test]
    fn a_plank_over_a_gap_is_a_beam_but_a_walkway_isnt() {
        // Two roofs 6 m up with a gap between; a plank across it.
        let mut tris = ground(0.0);
        tris.extend(MeshWorld::oriented_box(Vec3::new(0.0, 3.0, 6.0), Vec3::new(3.0, 3.0, 3.0), 0.0));
        tris.extend(MeshWorld::oriented_box(Vec3::new(0.0, 3.0, -6.0), Vec3::new(3.0, 3.0, 3.0), 0.0));
        let w = MeshWorld::new(tris.clone(), vec![]);
        let plank = Candidate { a: Vec3::new(0.0, 6.0, 3.0), b: Vec3::new(0.0, 6.0, -3.0), thickness: 0.15, capsule: false };
        assert!(matches!(classify(&w, &[plank])[..], [Fixture::Beam { .. }]));
        // The same plank lying on a floor is just floor.
        let low = Candidate { a: Vec3::new(10.0, 0.1, 3.0), b: Vec3::new(10.0, 0.1, -3.0), thickness: 0.15, capsule: false };
        assert!(classify(&w, &[low]).is_empty());
    }

    /// A zipline found this way is ridden: run at it, jump, and she's on.
    #[test]
    fn faith_rides_a_found_zipline() {
        use crate::{Controller, Event, Input, State, Tuning};
        let tris = ground(0.0);
        let found = classify(&MeshWorld::new(tris.clone(), vec![]), &[cap(Vec3::new(0.0, 2.8, -4.0), Vec3::new(0.0, 0.8, -19.0), 0.02)]);
        assert_eq!(found.len(), 1);
        let w = MeshWorld::new(tris, found);
        let mut c = Controller::new(Tuning::default(), Vec3::new(0.0, 0.0, 2.0), 0.0);
        c.state = State::Ground;
        let mut jumped = false;
        let mut zipped = false;
        for _ in 0..240 {
            let jump = !jumped && c.feet.z < -4.5;
            jumped |= jump;
            let i = Input { move_axis: glam::Vec2::new(0.0, 1.0), jump_pressed: jump, jump_held: jumped, ..Default::default() };
            c.step(1.0 / 60.0, &i, &w);
            zipped |= c.events.iter().any(|e| matches!(e, Event::ZipStart));
        }
        assert!(zipped, "never got on: ended {:?} at {}", c.state.name(), c.feet);
    }

    /// How long finding them takes on a city's worth of collision (run with --ignored).
    #[test]
    #[ignore]
    fn timing() {
        let mut tris = ground(0.0);
        let mut seed = 1u32;
        let mut r = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        for _ in 0..8000 {
            let c = Vec3::new(r() * 60.0 - 30.0, r() * 12.0, r() * 60.0 - 30.0);
            tris.extend(MeshWorld::oriented_box(c, Vec3::new(r() * 2.0, r() * 2.0, r() * 2.0), r() * 6.0));
        }
        let w = MeshWorld::new(tris, vec![]);
        let cands: Vec<Candidate> = (0..600)
            .map(|_| {
                let a = Vec3::new(r() * 60.0 - 30.0, r() * 10.0, r() * 60.0 - 30.0);
                Candidate { a, b: a + Vec3::new(r() * 8.0 - 4.0, r() * 0.2, r() * 8.0 - 4.0), thickness: r() * 0.2, capsule: r() > 0.5 }
            })
            .collect();
        let t = std::time::Instant::now();
        let found = classify(&w, &cands);
        eprintln!("{} triangles, {} candidates -> {} fixtures in {:?}", w.tris.len(), cands.len(), found.len(), t.elapsed());
    }
}
