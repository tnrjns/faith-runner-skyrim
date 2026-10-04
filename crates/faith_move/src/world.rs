//! Collision world.
//!
//! Everything the controller knows about the level goes through the [`World`] trait: sweep
//! the player's box through it (first contact, with the surface normal) and ask whether a
//! box is clear. Those are the two questions a game engine's physics answers natively (UE3's
//! extent traces, Havok's linear casts and penetration queries), so porting the controller to
//! another engine means answering them, nothing else. Walls, floors and ledges can be at any
//! angle; [`BoxWorld`] (axis-aligned boxes: the test courses, the Mirror's Edge maps) and
//! [`MeshWorld`] (triangles) are two implementations.

use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min: min.min(max), max: min.max(max) }
    }

    pub fn from_center_size(center: Vec3, size: Vec3) -> Self {
        let h = size * 0.5;
        Self::new(center - h, center + h)
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    /// Strict overlap (touching faces do not count).
    pub fn overlaps(&self, o: &Aabb) -> bool {
        const E: f32 = 1e-4;
        self.min.x < o.max.x - E
            && self.max.x > o.min.x + E
            && self.min.y < o.max.y - E
            && self.max.y > o.min.y + E
            && self.min.z < o.max.z - E
            && self.max.z > o.min.z + E
    }

    pub fn translated(&self, d: Vec3) -> Self {
        Self { min: self.min + d, max: self.max + d }
    }

    pub fn union(&self, o: &Aabb) -> Self {
        Self { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
}

/// Things you grab, ride or balance on that aren't plain boxes
/// (Mirror's Edge places these as TdZipLine, TdSwingVolume and
/// TdBalanceWalkVolume actors).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fixture {
    /// Zipline cable from `a` (the high end) down to `b`.
    ZipLine { a: Vec3, b: Vec3 },
    /// Horizontal swing bar from `a` to `b`.
    SwingPole { a: Vec3, b: Vec3 },
    /// Balance beam: the centre line of its top, `a` to `b`. The beam itself
    /// is an ordinary solid box.
    Beam { a: Vec3, b: Vec3 },
    /// A door you barge or kick open (TdMove_Barge). Solid until opened; `n` is the side you
    /// come at it from (it swings away from you).
    Door { b: Aabb, n: Vec3 },
    /// Barbed wire (TdBarbedWireVolume): walk into it and you stumble.
    BarbedWire { b: Aabb },
    /// A soft landing object (mattress, cardboard: TdMove_Landing.IsLandingOnSoftObject). The
    /// pad itself is an ordinary solid box; this marks its top as soft.
    SoftPad { b: Aabb },
    /// A ladder or drainpipe (TdLadderVolume).
    Ladder(crate::climb::Ladder),
}

/// Where a swept box first touched something.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepHit {
    /// How much of the move was made before the contact (0..1).
    pub t: f32,
    /// The surface's normal at the contact, facing back at the box.
    pub normal: Vec3,
    /// The contact point, on the surface.
    pub point: Vec3,
}

/// The level, as the controller sees it.
pub trait World {
    /// Sweep a box (half size `half`, centred at `start`) along `delta` and return the first
    /// solid it runs into before the end. Solids it starts inside (beyond touching) don't
    /// count, like the game's extent traces.
    fn sweep(&self, half: Vec3, start: Vec3, delta: Vec3) -> Option<SweepHit>;

    /// Whether anything solid is inside `region` (touching its faces doesn't count).
    fn overlaps(&self, region: &Aabb) -> bool;

    /// Ziplines, swing poles and balance beams.
    fn fixtures(&self) -> &[Fixture] {
        &[]
    }

    fn is_free(&self, region: &Aabb) -> bool {
        !self.overlaps(region)
    }
}

/// Two worlds as one: a host's still collision and what moves in it (doors, gates, drawbridges),
/// read again every frame. The fixtures are the first one's.
pub struct Layered<'a> {
    pub still: &'a dyn World,
    pub moving: &'a dyn World,
}

impl World for Layered<'_> {
    fn sweep(&self, half: Vec3, start: Vec3, delta: Vec3) -> Option<SweepHit> {
        match (self.still.sweep(half, start, delta), self.moving.sweep(half, start, delta)) {
            (Some(a), Some(b)) => Some(if b.t < a.t { b } else { a }),
            (a, b) => a.or(b),
        }
    }

    fn overlaps(&self, region: &Aabb) -> bool {
        self.still.overlaps(region) || self.moving.overlaps(region)
    }

    fn fixtures(&self) -> &[Fixture] {
        self.still.fixtures()
    }
}

/// How far a box may be inside something and still only be touching it.
const TOUCH: f32 = 1e-4;
/// How far a box may have sunk into a triangle surface and still be held by it (MeshWorld).
const SKIN: f32 = 0.05;
/// How high a lip in the floor the box is lifted over (slide_move).
const LIP: f32 = 0.05;

/// Sweep a box against one axis-aligned box: (fraction, normal) of the first contact. The
/// usual slab test on the box grown by `half`, with the overlap judged strictly (TOUCH): a
/// box only touching sideways is passed, one touching ahead is hit at once.
fn sweep_aabb(b: &Aabb, half: Vec3, c: Vec3, delta: Vec3) -> Option<(f32, Vec3)> {
    let lo = b.min - half;
    let hi = b.max + half;
    let (mut enter_s, mut exit_s) = (f32::NEG_INFINITY, f32::INFINITY);
    let mut enter = 0.0f32;
    let mut normal = Vec3::ZERO;
    for a in 0..3 {
        let d = delta[a];
        if d.abs() < 1e-9 {
            if c[a] <= lo[a] + TOUCH || c[a] >= hi[a] - TOUCH {
                return None;
            }
            continue;
        }
        // Strict slab (shrunk by TOUCH) decides whether the paths overlap; the true faces
        // say when the contact is.
        let (sa, sb) = ((lo[a] + TOUCH - c[a]) / d, (hi[a] - TOUCH - c[a]) / d);
        let (se, sx) = if sa < sb { (sa, sb) } else { (sb, sa) };
        let face = if d > 0.0 { (lo[a] - c[a]) / d } else { (hi[a] - c[a]) / d };
        if se > enter_s {
            enter_s = se;
            enter = face;
            normal = Vec3::ZERO;
            normal[a] = -d.signum();
        }
        exit_s = exit_s.min(sx);
    }
    if normal == Vec3::ZERO || enter_s >= exit_s || enter_s >= 1.0 || exit_s <= 0.0 || enter_s <= 0.0 {
        return None;
    }
    let t = enter.max(0.0);
    (t < 1.0).then_some((t, normal))
}

fn contact_on_box(b: &Aabb, half: Vec3, c: Vec3, delta: Vec3, t: f32, n: Vec3) -> Vec3 {
    let at = c + delta * t;
    let mut p = at.clamp(b.min, b.max);
    for a in 0..3 {
        if n[a] != 0.0 {
            p[a] = at[a] - n[a] * half[a];
        }
    }
    p
}

/// Boxes, scanned in full for the greybox levels (a few hundred solids), through a grid for a
/// real map ([`BoxWorld::indexed`], hundreds of thousands).
#[derive(Default, Clone, Debug)]
pub struct BoxWorld {
    pub boxes: Vec<Aabb>,
    pub fixtures: Vec<Fixture>,
    pub(crate) grid: Option<Grid>,
}

/// Items by the ground cells (x, z) they cover; the few huge ones are always checked.
#[derive(Default, Clone, Debug)]
pub(crate) struct Grid {
    cell: f32,
    cells: std::collections::HashMap<(i32, i32), Vec<u32>>,
    large: Vec<u32>,
}

impl Grid {
    fn build(cell: f32, items: impl Iterator<Item = Aabb>) -> Self {
        let mut grid = Grid { cell, ..Default::default() };
        for (i, b) in items.enumerate() {
            let (x0, x1, z0, z1) = grid.range(&b);
            if (x1 - x0 + 1) as i64 * (z1 - z0 + 1) as i64 > 256 {
                grid.large.push(i as u32);
                continue;
            }
            for x in x0..=x1 {
                for z in z0..=z1 {
                    grid.cells.entry((x, z)).or_default().push(i as u32);
                }
            }
        }
        grid
    }

    fn range(&self, b: &Aabb) -> (i32, i32, i32, i32) {
        let c = |v: f32| (v / self.cell).floor() as i32;
        (c(b.min.x), c(b.max.x), c(b.min.z), c(b.max.z))
    }

    /// Indices of the items whose cells `region` touches (a superset of what it overlaps).
    fn candidates(&self, region: &Aabb) -> Vec<u32> {
        let (x0, x1, z0, z1) = self.range(region);
        let mut ids: Vec<u32> = self.large.clone();
        for x in x0..=x1 {
            for z in z0..=z1 {
                if let Some(v) = self.cells.get(&(x, z)) {
                    ids.extend_from_slice(v);
                }
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

impl BoxWorld {
    pub fn add(&mut self, b: Aabb) -> &mut Self {
        self.boxes.push(b);
        self.grid = None;
        self
    }

    /// A world of many boxes, with a grid of `cell`-metre squares to find them by.
    pub fn indexed(boxes: Vec<Aabb>, fixtures: Vec<Fixture>, cell: f32) -> Self {
        let grid = Grid::build(cell, boxes.iter().copied());
        BoxWorld { boxes, fixtures, grid: Some(grid) }
    }

    /// The boxes overlapping `region`.
    pub fn query(&self, region: &Aabb, out: &mut Vec<Aabb>) {
        match &self.grid {
            None => out.extend(self.boxes.iter().filter(|b| b.overlaps(region)).copied()),
            Some(g) => out.extend(g.candidates(region).into_iter().map(|i| self.boxes[i as usize]).filter(|b| b.overlaps(region))),
        }
    }
}

/// The first contact of a box swept through some boxes.
fn sweep_boxes<'a>(boxes: impl Iterator<Item = &'a Aabb>, half: Vec3, start: Vec3, delta: Vec3) -> Option<SweepHit> {
    let from = Aabb::new(start - half, start + half);
    let mut best: Option<(f32, Vec3, Aabb)> = None;
    for b in boxes {
        if b.overlaps(&from) {
            continue;
        }
        if let Some((t, n)) = sweep_aabb(b, half, start, delta) {
            if best.is_none_or(|(bt, _, _)| t < bt) {
                best = Some((t, n, *b));
            }
        }
    }
    best.map(|(t, n, b)| SweepHit { t, normal: n, point: contact_on_box(&b, half, start, delta, t, n) })
}

impl World for BoxWorld {
    fn sweep(&self, half: Vec3, start: Vec3, delta: Vec3) -> Option<SweepHit> {
        let a = Aabb::new(start - half, start + half);
        let region = a.union(&a.translated(delta));
        let mut boxes = Vec::new();
        // Candidates touching the swept region (with a hair, for touching faces).
        let grown = Aabb::new(region.min - Vec3::splat(0.01), region.max + Vec3::splat(0.01));
        self.query(&grown, &mut boxes);
        sweep_boxes(boxes.iter(), half, start, delta)
    }

    fn overlaps(&self, region: &Aabb) -> bool {
        match &self.grid {
            None => self.boxes.iter().any(|b| b.overlaps(region)),
            Some(g) => g.candidates(region).into_iter().any(|i| self.boxes[i as usize].overlaps(region)),
        }
    }

    fn fixtures(&self) -> &[Fixture] {
        &self.fixtures
    }
}

/// A world with some doors closed: the doors are extra solid boxes.
pub struct WithDoors<'a> {
    pub world: &'a dyn World,
    pub closed: Vec<Aabb>,
}

impl World for WithDoors<'_> {
    fn sweep(&self, half: Vec3, start: Vec3, delta: Vec3) -> Option<SweepHit> {
        let a = self.world.sweep(half, start, delta);
        let b = sweep_boxes(self.closed.iter(), half, start, delta);
        match (a, b) {
            (Some(a), Some(b)) => Some(if b.t < a.t { b } else { a }),
            (a, b) => a.or(b),
        }
    }

    fn overlaps(&self, region: &Aabb) -> bool {
        self.world.overlaps(region) || self.closed.iter().any(|b| b.overlaps(region))
    }

    fn fixtures(&self) -> &[Fixture] {
        self.world.fixtures()
    }
}

/// Triangles at any angle: what a game engine's static geometry is. Sweeps step the box along
/// the path (2 cm) and narrow the first contact down by bisection; overlaps are the
/// separating-axis box / triangle test.
#[derive(Default, Clone, Debug)]
pub struct MeshWorld {
    pub tris: Vec<[Vec3; 3]>,
    pub fixtures: Vec<Fixture>,
    grid: Option<Grid>,
}

impl MeshWorld {
    pub fn new(tris: Vec<[Vec3; 3]>, fixtures: Vec<Fixture>) -> Self {
        let grid = Grid::build(4.0, tris.iter().map(tri_bounds));
        MeshWorld { tris, fixtures, grid: Some(grid) }
    }

    /// Two triangles making the quad `a b c d` (in order round it).
    pub fn quad(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> [[Vec3; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    /// The twelve triangles of a box rotated by `yaw` about the vertical, centred at `c`.
    pub fn oriented_box(c: Vec3, half: Vec3, yaw: f32) -> Vec<[Vec3; 3]> {
        let r = glam::Quat::from_rotation_y(yaw);
        let p = |x: f32, y: f32, z: f32| c + r * Vec3::new(x * half.x, y * half.y, z * half.z);
        let v = [
            p(-1., -1., -1.), p(1., -1., -1.), p(1., 1., -1.), p(-1., 1., -1.),
            p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.),
        ];
        let faces = [[0, 1, 2, 3], [5, 4, 7, 6], [4, 0, 3, 7], [1, 5, 6, 2], [3, 2, 6, 7], [4, 5, 1, 0]];
        faces.iter().flat_map(|f| Self::quad(v[f[0]], v[f[1]], v[f[2]], v[f[3]])).collect()
    }

    fn near(&self, region: &Aabb) -> Vec<usize> {
        match &self.grid {
            Some(g) => g.candidates(region).into_iter().map(|i| i as usize).collect(),
            None => (0..self.tris.len()).collect(),
        }
    }
}

fn tri_bounds(t: &[Vec3; 3]) -> Aabb {
    Aabb { min: t[0].min(t[1]).min(t[2]), max: t[0].max(t[1]).max(t[2]) }
}

/// Separating-axis test: does the triangle cut into the box (centre `c`, half size `h`)?
/// Akenine-Moller's box / triangle overlap, strict (touching doesn't count).
fn tri_box_overlap(t: &[Vec3; 3], c: Vec3, h: Vec3) -> bool {
    let v = [t[0] - c, t[1] - c, t[2] - c];
    let e = [v[1] - v[0], v[2] - v[1], v[0] - v[2]];
    let sep = |axis: Vec3| -> bool {
        if axis.length_squared() < 1e-12 {
            return false;
        }
        let axis = axis.normalize();
        let p = [v[0].dot(axis), v[1].dot(axis), v[2].dot(axis)];
        let r = h.x * axis.x.abs() + h.y * axis.y.abs() + h.z * axis.z.abs();
        let (mn, mx) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        mn >= r - TOUCH || mx <= -r + TOUCH
    };
    for a in [Vec3::X, Vec3::Y, Vec3::Z] {
        if sep(a) {
            return false;
        }
    }
    let n = e[0].cross(e[1]);
    if sep(n) {
        return false;
    }
    for a in [Vec3::X, Vec3::Y, Vec3::Z] {
        for ed in e {
            if sep(a.cross(ed)) {
                return false;
            }
        }
    }
    true
}

impl World for MeshWorld {
    fn sweep(&self, half: Vec3, start: Vec3, delta: Vec3) -> Option<SweepHit> {
        let a = Aabb::new(start - half, start + half);
        let region = a.union(&a.translated(delta));
        let grown = Aabb::new(region.min - Vec3::splat(0.01), region.max + Vec3::splat(0.01));
        let near: Vec<&[Vec3; 3]> = self.near(&grown).into_iter().map(|i| &self.tris[i]).filter(|t| tri_bounds(t).overlaps(&grown)).collect();
        // A surface the box already cuts into: what it starts inside doesn't count (it can
        // always get out), except a surface it has only sunk into by a hair (rounding far from
        // the origin, a refreshed world): moving further into that is blocked straight away,
        // so the box can't sink on through it.
        let mut skin: Option<(f32, Vec3)> = None;
        for t in near.iter().filter(|t| tri_box_overlap(t, start, half)) {
            let mut n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
            if n == Vec3::ZERO {
                continue;
            }
            if n.dot(start - t[0]) < 0.0 {
                n = -n;
            }
            let support = half.x * n.x.abs() + half.y * n.y.abs() + half.z * n.z.abs();
            let depth = support - n.dot(start - t[0]);
            let toward = -n.dot(delta);
            if depth < SKIN && toward > 1e-7 && skin.is_none_or(|(b, _)| toward > b) {
                skin = Some((toward, n));
            }
        }
        if let Some((_, n)) = skin {
            let support = half.x * n.x.abs() + half.y * n.y.abs() + half.z * n.z.abs();
            return Some(SweepHit { t: 0.0, normal: n, point: start - n * support });
        }
        let tris: Vec<&[Vec3; 3]> = near.into_iter().filter(|t| !tri_box_overlap(t, start, half)).collect();
        if tris.is_empty() {
            return None;
        }
        let hit_at = |s: f32| tris.iter().find(|t| tri_box_overlap(t, start + delta * s, half)).copied();
        let len = delta.length();
        // Steps no longer than the box is thick along the path, so a thin box can't skip
        // over a surface between two of them.
        let dir = delta / len.max(1e-9);
        let thick = 2.0 * (half.x * dir.x.abs() + half.y * dir.y.abs() + half.z * dir.z.abs());
        let step = 0.02f32.min(thick * 0.9).max(1e-4);
        let steps = (len / step).ceil().max(1.0) as usize;
        let mut prev = 0.0f32;
        for i in 1..=steps {
            let s = i as f32 / steps as f32;
            if hit_at(s).is_some() {
                let (mut lo, mut hi) = (prev, s);
                for _ in 0..14 {
                    let mid = (lo + hi) * 0.5;
                    if hit_at(mid).is_some() {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                // Of the triangles touched (several at an edge), the one met most head-on.
                let at = start + delta * lo;
                // Surfaces met within a hair of each other count as met together (the
                // bisection settles on whichever overlap test flips first, to rounding).
                let probe = start + delta * (hi + 4.0 * TOUCH / len.max(1e-9)).min(1.0);
                let mut best: Option<(f32, Vec3)> = None;
                for tri in tris.iter().filter(|t| tri_box_overlap(t, probe, half)) {
                    let mut n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
                    if n.dot(at - tri[0]) < 0.0 {
                        n = -n;
                    }
                    let head_on = -n.dot(delta);
                    if best.is_none_or(|(b, _)| head_on > b) {
                        best = Some((head_on, n));
                    }
                }
                let (_, n) = best?;
                // Bisection stops where the overlap test flips: TOUCH deep. Back off along
                // the path to just outside the surface, so sliding along it from here is
                // clear.
                let toward = -n.dot(delta);
                let t = if toward > 1e-6 { (lo - 2.0 * TOUCH / toward).max(0.0) } else { lo };
                let at = start + delta * t;
                let support = half.x * n.x.abs() + half.y * n.y.abs() + half.z * n.z.abs();
                return Some(SweepHit { t, normal: n, point: at - n * support });
            }
            prev = s;
        }
        None
    }

    fn overlaps(&self, region: &Aabb) -> bool {
        let (c, h) = (region.center(), region.size() * 0.5);
        self.near(region).into_iter().any(|i| {
            let t = &self.tris[i];
            tri_bounds(t).overlaps(region) && tri_box_overlap(t, c, h)
        })
    }

    fn fixtures(&self) -> &[Fixture] {
        &self.fixtures
    }
}

/// Closest point on segment `a`–`b` to `p`, as (point, distance along from `a`).
pub fn closest_on_segment(a: Vec3, b: Vec3, p: Vec3) -> (Vec3, f32) {
    let ab = b - a;
    let len = ab.length();
    if len < 1e-6 {
        return (a, 0.0);
    }
    let u = ab / len;
    let s = (p - a).dot(u).clamp(0.0, len);
    (a + u * s, s)
}

/// Player collider: feet position is the bottom-center of the box.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub half_width: f32,
    pub height: f32,
}

impl Body {
    pub fn aabb(&self, feet: Vec3) -> Aabb {
        Aabb::new(
            feet - Vec3::new(self.half_width, 0.0, self.half_width),
            feet + Vec3::new(self.half_width, self.height, self.half_width),
        )
    }

    /// How far the box reaches out from its centre line toward horizontal direction `n`.
    pub fn reach_toward(&self, n: Vec3) -> f32 {
        self.half_width * (n.x.abs() + n.z.abs())
    }
}

/// Result of a single-axis move.
#[derive(Default, Clone, Copy, Debug)]
pub struct AxisHit {
    pub blocked: bool,
    /// What stopped it (when blocked).
    pub normal: Vec3,
}

/// Move `feet` along one axis by `d`, stopping at the first thing in the way.
pub fn move_axis(world: &dyn World, body: Body, feet: &mut Vec3, axis: usize, d: f32) -> AxisHit {
    if d == 0.0 {
        return AxisHit::default();
    }
    let me = body.aabb(*feet);
    let mut delta = Vec3::ZERO;
    delta[axis] = d;
    match world.sweep(me.size() * 0.5, me.center(), delta) {
        Some(h) => {
            feet[axis] += d * h.t;
            AxisHit { blocked: true, normal: h.normal }
        }
        None => {
            feet[axis] += d;
            AxisHit::default()
        }
    }
}

/// Surfaces whose normal points at least this far up are ground you can walk on (about 45
/// degrees).
pub const WALKABLE: f32 = 0.7;

/// Move `feet` horizontally by `delta`, sliding along whatever it runs into (up to three
/// contacts: the leftover motion loses its part into each surface). Returns the horizontal
/// normals of what it touched.
pub fn slide_move(world: &dyn World, body: Body, feet: &mut Vec3, delta: Vec3) -> Vec<Vec3> {
    let mut rem = Vec3::new(delta.x, 0.0, delta.z);
    let mut normals = vec![];
    for _ in 0..3 {
        if rem.length_squared() < 1e-12 {
            break;
        }
        let me = body.aabb(*feet);
        match world.sweep(me.size() * 0.5, me.center(), rem) {
            None => {
                *feet += rem;
                break;
            }
            Some(h) => {
                let progress = rem.length() * h.t;
                *feet += rem * h.t;
                rem *= 1.0 - h.t;
                if h.normal.y >= WALKABLE {
                    // Ground you can walk up: carry on along it, up the slope.
                    let into = rem.dot(h.normal);
                    if into < 0.0 {
                        rem -= h.normal * into;
                    } else if progress < 0.001 {
                        // Stopped dead by the edge of something flat just above the feet (a
                        // seam where two floors meet a centimetre apart): over it, as a foot
                        // would. The walk settles her onto it after.
                        let lifted = move_axis(world, body, feet, 1, LIP);
                        if !lifted.blocked {
                            continue;
                        }
                        break;
                    }
                    continue;
                }
                let n = Vec3::new(h.normal.x, 0.0, h.normal.z).normalize_or_zero();
                if n == Vec3::ZERO {
                    break;
                }
                normals.push(n);
                let into = rem.dot(n);
                if into < 0.0 {
                    rem -= n * into;
                }
            }
        }
    }
    normals
}

/// The horizontal part of a surface normal, if it's a wall (steeper than ~45 degrees).
fn wall_normal(n: Vec3) -> Option<Vec3> {
    let h = Vec3::new(n.x, 0.0, n.z);
    (n.y.abs() < 0.7 && h.length_squared() > 1e-6).then(|| h.normalize())
}

/// A wall face in front of the body, found by sweeping a slice of it (from `y_lo` to `y_hi`
/// above the feet) along `dir` for `reach`: its horizontal normal (facing us within ~80
/// degrees) and the gap between the body and it.
pub fn probe_wall(
    world: &dyn World,
    body: Body,
    feet: Vec3,
    dir: Vec3,
    reach: f32,
    y_lo: f32,
    y_hi: f32,
) -> Option<(Vec3, f32)> {
    let dir = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    let mut me = body.aabb(feet);
    me.min.y = feet.y + y_lo;
    me.max.y = feet.y + y_hi;
    let h = world.sweep(me.size() * 0.5, me.center(), dir * reach)?;
    let n = wall_normal(h.normal)?;
    if n.dot(dir) > -0.2 {
        return None;
    }
    Some((n, wall_gap(body, feet, n, h.point)))
}

/// Gap between the body and the plane through `point` with horizontal normal `n`.
fn wall_gap(body: Body, feet: Vec3, n: Vec3, point: Vec3) -> f32 {
    let d = Vec3::new(feet.x - point.x, 0.0, feet.z - point.z).dot(n);
    (d - body.reach_toward(n)).max(0.0)
}

/// Sweep a small box (half size `half`) from `start` along `dir` for up to `reach`; the first
/// solid it enters gives (where the box stopped, the surface's normal).
pub fn trace_box(world: &dyn World, start: Vec3, dir: Vec3, reach: f32, half: Vec3) -> Option<(Vec3, Vec3)> {
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO || reach <= 0.0 {
        return None;
    }
    let h = world.sweep(half, start, dir * reach)?;
    Some((start + dir * reach * h.t, h.normal))
}

/// A wall face found by [`trace_wall`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallHit {
    /// Outward face normal (horizontal).
    pub n: Vec3,
    /// Where the body's centre line, straight along the normal, meets the face (at the trace
    /// height).
    pub at: Vec3,
    /// Gap between the body and the face.
    pub gap: f32,
}

/// Sweep a thin slice of the body (`y` above the feet, ±2 cm like ME's extent traces)
/// horizontally along `dir` for up to `reach` and return the first wall it runs into, at any
/// angle to `dir`, including ones it only grazes.
pub fn trace_wall(world: &dyn World, body: Body, feet: Vec3, dir: Vec3, reach: f32, y: f32) -> Option<WallHit> {
    let dir = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
    if dir == Vec3::ZERO || reach <= 0.0 {
        return None;
    }
    let half = Vec3::new(body.half_width, 0.02, body.half_width);
    let c = feet + Vec3::Y * y;
    let h = world.sweep(half, c, dir * reach)?;
    let n = wall_normal(h.normal)?;
    let p = Vec3::new(c.x, c.y, c.z);
    let at = p - n * Vec3::new(p.x - h.point.x, 0.0, p.z - h.point.z).dot(n);
    Some(WallHit { n, at, gap: wall_gap(body, feet, n, h.point) })
}

/// Heights of the tops of the surfaces under a column (half size `half` across) at `p`,
/// highest first, from `y_hi` down to `y_lo`: each downward sweep finds the next one, then
/// carries on from just inside it (what it starts inside doesn't count).
pub fn tops_below(world: &dyn World, p: Vec3, half_xz: f32, y_hi: f32, y_lo: f32) -> Vec<f32> {
    let half = Vec3::new(half_xz, 0.005, half_xz);
    let mut out = vec![];
    let mut y = y_hi;
    for _ in 0..8 {
        let d = y_lo - y;
        if d >= 0.0 {
            break;
        }
        let c = Vec3::new(p.x, y + half.y, p.z);
        match world.sweep(half, c, Vec3::Y * d) {
            Some(h) => {
                let top = y + d * h.t;
                if h.normal.y > 0.7 {
                    out.push(top);
                }
                y = top - 0.03;
            }
            None => break,
        }
    }
    out
}

/// Height of the top of whatever stands at `p` (x, z): a thin trace down from `y1` to `y0`,
/// like the game's ledge traces, so the first surface under `y1` (`y0` if there's none).
/// Traces only see surfaces, so `y1` should be above anything that counts.
pub fn column_top(world: &dyn World, p: Vec3, y0: f32, y1: f32) -> f32 {
    tops_below(world, p, 0.01, y1, y0).first().copied().unwrap_or(y0)
}

/// Find a climbable top edge on the wall with outward normal `n` that the body is up against,
/// whose height above `feet.y` is within `[lo, hi]`. Returns the world-space height of the
/// ledge top. Like TdPawn's FindLedge it traces down just inside the wall; it needs room to
/// crouch on top, and nothing overhanging where the hands go.
pub fn find_ledge(
    world: &dyn World,
    body: Body,
    feet: Vec3,
    n: Vec3,
    lo: f32,
    hi: f32,
    crouch_height: f32,
) -> Option<f32> {
    let hw = body.half_width;
    let n = Vec3::new(n.x, 0.0, n.z).normalize_or_zero();
    // Column just inside the wall face, in front of the player.
    let inside = feet - n * (body.reach_toward(n) + 0.02 + hw);
    let me = body.aabb(feet);
    let mut tops = tops_below(world, inside, hw, feet.y + hi + 0.05, feet.y + lo - 0.05);
    tops.retain(|t| *t >= feet.y + lo && *t <= feet.y + hi);
    tops.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for t in tops {
        // Room to crouch on top of the ledge.
        let stand = Aabb::new(Vec3::new(inside.x - hw, t + 0.01, inside.z - hw), Vec3::new(inside.x + hw, t + crouch_height, inside.z + hw));
        if !world.is_free(&stand) {
            continue;
        }
        // Hands/head space on our side of the wall, just above the lip.
        let mut hands = me;
        hands.min.y = t.max(feet.y + 0.1);
        hands.max.y = t + 0.35;
        if !world.is_free(&hands) {
            continue;
        }
        return Some(t);
    }
    None
}
