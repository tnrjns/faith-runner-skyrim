//! Turns a survey of a Skyrim city's collision (the plugin's F10: FaithSurvey_<worldspace>.bin)
//! into parkour fixes:
//! - **Faith-only collision patch** (<worldspace>.bin for Data\SKSE\Plugins\FaithParkour): thin
//!   invisible fillers over the seams between rooftops she'd otherwise trip in or snag on.
//! - **Visible connectors** (placements JSON for the ESP generator): where two rooftops are close
//!   but Faith can't make the jump, a plank or scaffold bridge across.
//!
//! "Can't make the jump" is decided by Faith's own movement: every gap (distance, height) is run
//! with faith_move on a two-platform test, sprinting up to the edge and jumping.
//!
//! Usage: parkour_tool <survey.bin> <out dir> [riften|solitude]

use std::collections::HashMap;
use std::io::Write;

use faith_move::{Aabb, BoxWorld, Controller, Input, MeshWorld, State, Tuning, World};
use glam::{Vec2, Vec3};

const UNITS: f32 = 70.0;
const CELL: f32 = 0.5;

struct Survey {
    world: u32,
    tris: Vec<[Vec3; 3]>,
}

fn read_survey(path: &str) -> Survey {
    let b = std::fs::read(path).expect("read survey");
    assert_eq!(&b[0..4], b"FSV1", "not a survey");
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let f32_at = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let world = u32_at(4);
    let count = u32_at(20) as usize;
    let mut tris = Vec::with_capacity(count);
    for i in 0..count {
        let o = 24 + i * 36;
        let p = |k: usize| Vec3::new(f32_at(o + k * 12), f32_at(o + k * 12 + 4), f32_at(o + k * 12 + 8)) / UNITS;
        tris.push([p(0), p(1), p(2)]);
    }
    Survey { world, tris }
}

/// Can Faith get from a platform to one `d` metres further on and `h` higher (sprinting up to the
/// edge, jumping at it, grabbing and pulling up if she has to)? Run in faith_move.
fn reachable(d: f32, h: f32) -> bool {
    // faith_move's frame: Y up. A: x in [-30, 0], top 0. B: x in [d, d + 15], top h.
    let w = 6.0;
    let boxes = vec![
        Aabb::new(Vec3::new(-30.0, -20.0, -w), Vec3::new(0.0, 0.0, w)),
        Aabb::new(Vec3::new(d, -20.0, -w), Vec3::new(d + 15.0, h, w)),
    ];
    let world = BoxWorld::indexed(boxes, vec![], 4.0);
    // Facing +x: forward(yaw) = (-sin yaw, 0, -cos yaw).
    let mut c = Controller::new(Tuning::default(), Vec3::new(-25.0, 0.0, 0.0), -std::f32::consts::FRAC_PI_2);
    c.state = State::Ground;
    let dt = 1.0 / 60.0;
    let mut jumped = false;
    for _ in 0..(8.0 / dt) as usize {
        let jump = !jumped && c.feet.x > -0.35 && c.state == State::Ground;
        jumped |= jump;
        let input = Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: jump, jump_held: jumped, ..Default::default() };
        c.step(dt, &input, &world);
        if c.feet.x > d + 0.3 && (c.feet.y - h).abs() < 0.15 && c.state == State::Ground {
            return true;
        }
        if c.feet.y < h.min(0.0) - 3.0 {
            return false;
        }
    }
    false
}

/// reachable() on a grid: distance 0..14 m, height -10..+5 m, every 0.25 m.
struct Reach {
    grid: Vec<bool>,
}

impl Reach {
    const DS: usize = 57;
    const HS: usize = 61;
    fn build() -> Self {
        let mut grid = vec![false; Self::DS * Self::HS];
        for i in 0..Self::DS {
            for j in 0..Self::HS {
                grid[i * Self::HS + j] = reachable(i as f32 * 0.25, -10.0 + j as f32 * 0.25);
            }
        }
        Reach { grid }
    }
    fn can(&self, d: f32, h: f32) -> bool {
        // Conservative: the next grid point out (further, higher).
        let i = (d / 0.25).ceil().max(0.0) as usize;
        let j = ((h + 10.0) / 0.25).ceil().max(0.0) as usize;
        i < Self::DS && j < Self::HS && self.grid[i * Self::HS + j]
    }
    fn max_distance(&self, h: f32) -> f32 {
        (0..Self::DS).rev().find(|&i| self.can(i as f32 * 0.25, h)).map_or(0.0, |i| i as f32 * 0.25)
    }
}

/// The top and bottom walkable surfaces over a grid of 0.5 m cells (host frame, metres, Z up).
struct Heights {
    x0: f32,
    y0: f32,
    nx: usize,
    ny: usize,
    top: Vec<f32>,
    base: Vec<f32>,
}

fn heights(tris: &[[Vec3; 3]]) -> Heights {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for t in tris {
        for p in t {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    let nx = ((hi.x - lo.x) / CELL).ceil() as usize + 1;
    let ny = ((hi.y - lo.y) / CELL).ceil() as usize + 1;
    let mut top = vec![f32::NAN; nx * ny];
    let mut base = vec![f32::NAN; nx * ny];
    for t in tris {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let len = n.length();
        if len < 1e-6 || (n.z / len).abs() < 0.7 {
            continue; // walls and steep roofs: not stood on
        }
        let (tlo, thi) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
        let (i0, i1) = (((tlo.x - lo.x) / CELL).floor() as usize, ((thi.x - lo.x) / CELL).ceil() as usize);
        let (j0, j1) = (((tlo.y - lo.y) / CELL).floor() as usize, ((thi.y - lo.y) / CELL).ceil() as usize);
        for j in j0..=j1.min(ny - 1) {
            for i in i0..=i1.min(nx - 1) {
                let p = Vec2::new(lo.x + (i as f32 + 0.5) * CELL, lo.y + (j as f32 + 0.5) * CELL);
                // Barycentric in xy.
                let (a, b, c) = (t[0].truncate(), t[1].truncate(), t[2].truncate());
                let v0 = b - a;
                let v1 = c - a;
                let v2 = p - a;
                let den = v0.x * v1.y - v1.x * v0.y;
                if den.abs() < 1e-9 {
                    continue;
                }
                let v = (v2.x * v1.y - v1.x * v2.y) / den;
                let w = (v0.x * v2.y - v2.x * v0.y) / den;
                if v < -1e-4 || w < -1e-4 || v + w > 1.0001 {
                    continue;
                }
                let z = t[0].z + v * (t[1].z - t[0].z) + w * (t[2].z - t[0].z);
                let k = j * nx + i;
                if top[k].is_nan() || z > top[k] {
                    top[k] = z;
                }
                if base[k].is_nan() || z < base[k] {
                    base[k] = z;
                }
            }
        }
    }
    Heights { x0: lo.x, y0: lo.y, nx, ny, top, base }
}

fn find(parent: &mut [usize], x: usize) -> usize {
    let mut r = x;
    while parent[r] != r {
        r = parent[r];
    }
    let mut x = x;
    while parent[x] != r {
        let n = parent[x];
        parent[x] = r;
        x = n;
    }
    r
}

#[derive(Clone, Copy)]
struct Link {
    a: usize,
    b: usize,
    pa: Vec3,
    pb: Vec3,
    gap: f32,
    h: f32,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let survey = read_survey(&args[1]);
    let out = std::path::Path::new(&args[2]);
    let city = args.get(3).cloned().unwrap_or_else(|| match survey.world {
        0x16BB4 => "riften".into(),
        0x37EDF => "solitude".into(),
        _ => "other".into(),
    });
    std::fs::create_dir_all(out).unwrap();
    let mut report = String::new();
    macro_rules! say {
        ($($t:tt)*) => {{ let s = format!($($t)*); println!("{s}"); report.push_str(&s); report.push('\n'); }};
    }
    say!("survey: worldspace {:08X} ({city}), {} triangles", survey.world, survey.tris.len());

    let t0 = std::time::Instant::now();
    let reach = Reach::build();
    say!(
        "Faith's reach (sprinting, from faith_move): {:.2} m flat, {:.2} m up 1 m, {:.2} m down 3 m, {:.2} m down 6 m ({:.1} s)",
        reach.max_distance(0.0),
        reach.max_distance(1.0),
        reach.max_distance(-3.0),
        reach.max_distance(-6.0),
        t0.elapsed().as_secs_f32()
    );

    let hm = heights(&survey.tris);
    let roof = |k: usize| !hm.top[k].is_nan() && hm.top[k] - hm.base[k] >= 2.5;
    // Rooftop patches: neighbouring roof cells less than 0.4 m apart in height.
    let n = hm.nx * hm.ny;
    let mut parent: Vec<usize> = (0..n).collect();
    for j in 0..hm.ny {
        for i in 0..hm.nx {
            let k = j * hm.nx + i;
            if !roof(k) {
                continue;
            }
            for (di, dj) in [(1usize, 0usize), (0, 1), (1, 1)] {
                let (ii, jj) = (i + di, j + dj);
                if ii >= hm.nx || jj >= hm.ny {
                    continue;
                }
                let kk = jj * hm.nx + ii;
                if roof(kk) && (hm.top[k] - hm.top[kk]).abs() <= 0.4 {
                    let (ra, rb) = (find(&mut parent, k), find(&mut parent, kk));
                    if ra != rb {
                        parent[ra] = rb;
                    }
                }
            }
        }
    }
    let mut size: HashMap<usize, usize> = HashMap::new();
    for k in 0..n {
        if roof(k) {
            *size.entry(find(&mut parent, k)).or_default() += 1;
        }
    }
    let min_cells = (6.0 / (CELL * CELL)) as usize;
    let patches: HashMap<usize, usize> = size.iter().filter(|(_, s)| **s >= min_cells).enumerate().map(|(i, (r, _))| (*r, i)).collect();
    say!("rooftops: {} patches of 6 m² or more", patches.len());

    // Edge points of each patch, one per 1 m square.
    let mut edges: Vec<(usize, Vec3)> = vec![];
    let mut seen = std::collections::HashSet::new();
    for j in 1..hm.ny - 1 {
        for i in 1..hm.nx - 1 {
            let k = j * hm.nx + i;
            if !roof(k) {
                continue;
            }
            let r = find(&mut parent, k);
            let Some(&p) = patches.get(&r) else { continue };
            let edge = [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)].iter().any(|(di, dj)| {
                let kk = ((j as i64 + dj) as usize) * hm.nx + (i as i64 + di) as usize;
                !roof(kk) || find(&mut parent, kk) != r
            });
            if edge && seen.insert((p, i / 2, j / 2)) {
                edges.push((p, Vec3::new(hm.x0 + (i as f32 + 0.5) * CELL, hm.y0 + (j as f32 + 0.5) * CELL, hm.top[k])));
            }
        }
    }
    // Closest edge points between patches (within 12 m).
    let bucket = |p: Vec3| ((p.x / 2.0).floor() as i64, (p.y / 2.0).floor() as i64);
    let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, (_, p)) in edges.iter().enumerate() {
        buckets.entry(bucket(*p)).or_default().push(i);
    }
    let mut best: HashMap<(usize, usize), Link> = HashMap::new();
    for (pa, a) in &edges {
        let (bx, by) = bucket(*a);
        for dx in -6..=6 {
            for dy in -6..=6 {
                let Some(list) = buckets.get(&(bx + dx, by + dy)) else { continue };
                for &k in list {
                    let (pb, b) = edges[k];
                    if pb == *pa {
                        continue;
                    }
                    let gap = (b - *a).truncate().length() - CELL;
                    if gap > 12.0 {
                        continue;
                    }
                    let key = (*pa.min(&pb), *pa.max(&pb));
                    let l = Link { a: *pa, b: pb, pa: *a, pb: b, gap: gap.max(0.0), h: b.z - a.z };
                    if best.get(&key).is_none_or(|o| l.gap < o.gap) {
                        best.insert(key, l);
                    }
                }
            }
        }
    }
    say!("{} pairs of rooftops within 12 m", best.len());

    // Nothing in the way between the two edges (a body-sized sweep at chest height).
    let mesh = MeshWorld::new(survey.tris.clone(), vec![]);
    let clear = |l: &Link| {
        let start = l.pa + Vec3::Z * 1.0;
        let end = Vec3::new(l.pb.x, l.pb.y, l.pa.z.max(l.pb.z) + 1.0);
        mesh.sweep(Vec3::new(0.2, 0.2, 0.4), start, end - start).is_none_or(|hit| hit.t > 0.97)
    };

    let mut links: Vec<Link> = best.values().copied().collect();
    links.sort_by(|a, b| a.gap.partial_cmp(&b.gap).unwrap());
    // Which rooftops Faith can already get between (either way).
    let mut net: Vec<usize> = (0..patches.len()).collect();
    let mut seams = vec![];
    for l in &links {
        if (reach.can(l.gap, l.h) || reach.can(l.gap, -l.h)) && clear(l) {
            let (ra, rb) = (find(&mut net, l.a), find(&mut net, l.b));
            net[ra] = rb;
            // A crack she'd trip or snag in: filled, for her only.
            if l.gap < 0.8 && l.h.abs() < 0.4 {
                seams.push(*l);
            }
        }
    }
    // Bridges where a jump is just out of reach: shortest first, joining what isn't joined.
    let (max_len, max_rise) = (12.0, 1.0);
    let mut bridges = vec![];
    for l in &links {
        if bridges.len() >= 30 {
            break;
        }
        if l.gap > max_len || l.h.abs() > max_rise || !clear(l) {
            continue;
        }
        let (ra, rb) = (find(&mut net, l.a), find(&mut net, l.b));
        if ra == rb {
            continue;
        }
        net[ra] = rb;
        bridges.push(*l);
    }
    say!("{} cracks between rooftops filled for Faith, {} bridges placed", seams.len(), bridges.len());

    // ---- getting up there: which rooftop networks can't be reached from the street?
    const FRAME: f32 = 192.0 / UNITS; // StockadeScaffoldBase0Sided01's height
    const HALF: Vec2 = Vec2::new(130.0 / UNITS, 135.0 / UNITS);
    let climb = (0..60).rev().map(|j| j as f32 * 0.25).find(|&h| reach.can(0.5, h)).unwrap_or(0.0);
    // The ground where you'd stand (or a tower would): the highest walkable surface under a
    // tower-sized footprint, below the roof (most of the footprint must have one).
    let ground_at = |p: Vec2, below: f32| -> Option<f32> {
        let (i0, i1) = (((p.x - HALF.x - hm.x0) / CELL) as i64, ((p.x + HALF.x - hm.x0) / CELL) as i64);
        let (j0, j1) = (((p.y - HALF.y - hm.y0) / CELL) as i64, ((p.y + HALF.y - hm.y0) / CELL) as i64);
        let (mut g, mut have, mut all) = (f32::MIN, 0, 0);
        for j in j0..=j1 {
            for i in i0..=i1 {
                all += 1;
                if i < 0 || j < 0 || i >= hm.nx as i64 || j >= hm.ny as i64 {
                    continue;
                }
                let k = j as usize * hm.nx + i as usize;
                // The surfaces here: the top one if it's below the roof, else the base.
                let z = if !hm.top[k].is_nan() && hm.top[k] < below - 0.5 { hm.top[k] } else { hm.base[k] };
                if !z.is_nan() && z < below - 0.5 {
                    g = g.max(z);
                    have += 1;
                }
            }
        }
        (have * 3 >= all * 2).then_some(g)
    };
    // Out from a roof edge, the direction with the lowest climb up to it (and that climb).
    let street = |e: Vec3| -> Option<(f32, Vec2, f32)> {
        (0..16)
            .filter_map(|a| {
                let ang = a as f32 * std::f32::consts::TAU / 16.0;
                let out = Vec2::new(ang.cos(), ang.sin());
                let g = ground_at(e.truncate() + out * (HALF.x + 0.6), e.z)?;
                Some((e.z - g, out, g))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
    };
    let mut reachable_net = std::collections::HashSet::new();
    let mut lowest: HashMap<usize, (f32, Vec3)> = HashMap::new();
    for (p, e) in &edges {
        let Some((rise, _, _)) = street(*e) else { continue };
        let net = find(&mut net, *p);
        if rise <= climb {
            reachable_net.insert(net);
        }
        let entry = lowest.entry(net).or_insert((f32::MAX, *e));
        if rise < entry.0 {
            *entry = (rise, *e);
        }
    }
    let nets: std::collections::HashSet<usize> = (0..patches.len()).map(|p| find(&mut net, p)).collect();
    let stuck: Vec<_> = lowest.iter().filter(|(n, _)| !reachable_net.contains(n)).collect();
    say!(
        "climbing: Faith gets {:.2} m up a wall from the ground; {} rooftop networks, {} reachable from the street, {} not",
        climb,
        nets.len(),
        nets.iter().filter(|n| reachable_net.contains(n)).count(),
        stuck.len()
    );
    let mut rises: Vec<f32> = stuck.iter().map(|(_, (r, _))| *r).collect();
    rises.sort_by(|a, b| a.partial_cmp(b).unwrap());
    say!("  the stuck ones' lowest climbs: {:?}", rises.iter().take(25).map(|r| (r * 10.0).round() / 10.0).collect::<Vec<_>>());

    // ---- access: a staircase of scaffold towers against the wall below each stuck network's
    // lowest edge: the one by the wall within a climb of the roof, then one frame (2.74 m)
    // shorter each along the wall, down to one frame, climbable from the street.
    let mut stuck_sorted: Vec<(usize, f32, Vec3)> = stuck.iter().map(|(n, (r, e))| (**n, *r, *e)).collect();
    stuck_sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let mut towers: Vec<(Vec3, Vec2, u32)> = vec![]; // (base centre on the ground, facing, frames)
    let mut accessed = 0;
    let mut why = [0usize; 5]; // off the map, -, too low/high, in the way, ok
    for (_, rise, e) in stuck_sorted {
        if accessed >= 15 || rise > climb + 4.0 * FRAME {
            continue;
        }
        let roof_z = e.z;
        let mut done = false;
        'dirs: for a in 0..16 {
            let ang = a as f32 * std::f32::consts::TAU / 16.0;
            let out = Vec2::new(ang.cos(), ang.sin());
            let along = Vec2::new(-out.y, out.x);
            let first = e.truncate() + out * (HALF.x + 0.6);
            let Some(g0) = ground_at(first, roof_z) else {
                why[0] += 1;
                continue;
            };
            let frames0 = ((roof_z - climb - g0) / FRAME).ceil().max(1.0) as u32;
            if frames0 > 5 || g0 + frames0 as f32 * FRAME > roof_z - 0.3 {
                why[2] += 1;
                continue;
            }
            let mut spots = vec![];
            for s in 0..frames0 {
                let frames = frames0 - s;
                let c = first + along * (s as f32 * HALF.y * 2.0);
                let Some(g) = ground_at(c, roof_z) else {
                    why[0] += 1;
                    continue 'dirs;
                };
                let top = g + frames as f32 * FRAME;
                // Every step climbable: the roof from the first tower, each tower from the next,
                // and the street from the last.
                let above = spots.last().map_or(roof_z, |(c, _, f): &(Vec3, Vec2, u32)| c.z + *f as f32 * FRAME);
                if above - top > climb || top >= above - 0.3 || (frames == 1 && top - g > climb) {
                    why[2] += 1;
                    continue 'dirs;
                }
                let lo = Vec3::new(c.x - HALF.x, c.y - HALF.y, g + 0.2);
                let hi = Vec3::new(c.x + HALF.x, c.y + HALF.y, top + 1.8);
                if mesh.overlaps(&Aabb::new(lo, hi)) {
                    why[3] += 1;
                    continue 'dirs;
                }
                spots.push((Vec3::new(c.x, c.y, g), out, frames));
            }
            why[4] += 1;
            towers.extend(spots);
            done = true;
            break;
        }
        if done {
            accessed += 1;
        }
    }
    say!("access: {} stuck rooftop networks given scaffold towers ({} towers); spots rejected: {} off the map, {} not open ground, {} wrong height, {} in the way; {} fine", accessed, towers.len(), why[0], why[1], why[2], why[3], why[4]);

    // ---- the Faith-only patch: a thin filler over each crack (game units).
    let mut patch = vec![];
    patch.extend_from_slice(b"FPK1");
    patch.extend_from_slice(&(seams.len() as u32).to_le_bytes());
    patch.extend_from_slice(&0u32.to_le_bytes());
    for l in &seams {
        let dir = (l.pb - l.pa).truncate();
        let len = dir.length().max(0.1);
        let yaw = dir.y.atan2(dir.x);
        let top = l.pa.z.max(l.pb.z);
        let c = (l.pa + l.pb) * 0.5;
        let vals = [c.x * UNITS, c.y * UNITS, (top - 0.05) * UNITS, (len * 0.5 + 0.3) * UNITS, 0.5 * UNITS, 0.05 * UNITS, yaw];
        for v in vals {
            patch.extend_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(out.join(format!("{:08X}.bin", survey.world)), &patch).unwrap();

    // ---- the visible connectors, for the ESP generator (game units; yaw = the bridge's direction).
    let mut json = String::from("[\n");
    for (i, l) in bridges.iter().enumerate() {
        let dir = (l.pb - l.pa).truncate();
        let c = (l.pa + l.pb) * 0.5;
        let top = l.pa.z.max(l.pb.z);
        json.push_str(&format!(
            "  {{\"world\": {}, \"city\": \"{city}\", \"x\": {:.1}, \"y\": {:.1}, \"top\": {:.1}, \"dirx\": {:.4}, \"diry\": {:.4}, \"length\": {:.1}}}{}\n",
            survey.world,
            c.x * UNITS,
            c.y * UNITS,
            top * UNITS,
            dir.x / dir.length(),
            dir.y / dir.length(),
            (dir.length() + 1.5) * UNITS,
            if i + 1 < bridges.len() { "," } else { "" }
        ));
        say!(
            "  bridge {}: {:.1} m across, {:+.1} m, at ({:.0}, {:.0}, {:.0}) units",
            i + 1,
            l.gap,
            l.h,
            c.x * UNITS,
            c.y * UNITS,
            top * UNITS
        );
    }
    if !towers.is_empty() && json.ends_with("}\n") {
        json.truncate(json.len() - 1);
        json.push_str(",\n");
    }
    for (i, (c, out, frames)) in towers.iter().enumerate() {
        json.push_str(&format!(
            "  {{\"world\": {}, \"city\": \"{city}\", \"kind\": \"tower\", \"x\": {:.1}, \"y\": {:.1}, \"ground\": {:.1}, \"dirx\": {:.4}, \"diry\": {:.4}, \"frames\": {frames}}}{}\n",
            survey.world,
            c.x * UNITS,
            c.y * UNITS,
            c.z * UNITS,
            out.x,
            out.y,
            if i + 1 < towers.len() { "," } else { "" }
        ));
    }
    json.push_str("]\n");
    std::fs::write(out.join(format!("placements_{city}.json")), json).unwrap();
    std::fs::File::create(out.join(format!("report_{city}.txt"))).unwrap().write_all(report.as_bytes()).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reach table agrees with Faith's known jumps: a short gap is made, a huge one isn't,
    /// and dropping down reaches further than jumping up.
    #[test]
    fn reach_matches_faith() {
        assert!(reachable(2.0, 0.0), "2 m flat");
        assert!(!reachable(13.0, 0.0), "13 m flat");
        assert!(!reachable(3.0, 7.0), "7 m up");
        let flat = (0..56).rev().map(|i| i as f32 * 0.25).find(|&d| reachable(d, 0.0)).unwrap();
        let down = (0..56).rev().map(|i| i as f32 * 0.25).find(|&d| reachable(d, -4.0)).unwrap();
        eprintln!("max flat {flat} m, down 4 m {down} m");
        assert!(down >= flat, "dropping reaches further: {down} vs {flat}");
    }
}
