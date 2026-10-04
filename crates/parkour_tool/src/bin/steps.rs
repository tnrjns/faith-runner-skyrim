//! The stairs in a surveyed Skyrim city: each flight found (three or more risers of 8-60 cm alike,
//! evenly spaced, flat treads), how high and deep its steps are, and how Faith walks and runs up it with faith_move
//! (whether she gets up, how much speed she keeps, how many steps she climbs by the walk's own
//! step-up and how far the mesh smoothing has to carry).
//!
//! Usage: steps <survey.bin>

use faith_move::world::tops_below;
use faith_move::{Controller, Input, MeshWorld, State, Tuning, World};
use glam::{Vec2, Vec3};

const UNITS: f32 = 70.0;

fn main() {
    let path = std::env::args().nth(1).expect("survey.bin");
    let b = std::fs::read(&path).expect("read survey");
    assert_eq!(&b[0..4], b"FSV1", "not a survey");
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let f32_at = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let count = u32_at(20) as usize;
    // Skyrim (Z up) -> faith_move (Y up): (x, z, -y), metres.
    let tris: Vec<[Vec3; 3]> = (0..count)
        .map(|i| {
            let o = 24 + i * 36;
            let p = |k: usize| Vec3::new(f32_at(o + k * 12), f32_at(o + k * 12 + 8), -f32_at(o + k * 12 + 4)) / UNITS;
            [p(0), p(1), p(2)]
        })
        .collect();
    let world = MeshWorld::new(tris.clone(), vec![]);
    // As the plugin runs her: no kill floor (cities sit at any height), auto step-up on.
    let tu = Tuning { kill_y: None, respawn_on_death: false, auto_step_up: true, ..Tuning::default() };
    let top_at = |p: Vec3, from: f32, to: f32| tops_below(&world, p, 0.05, from, to).first().copied();

    // Level spots to start from, one per metre.
    let mut seen = std::collections::HashSet::new();
    let mut spots = vec![];
    for t in &tris {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        if n.length_squared() < 1e-8 || n.normalize().y < 0.95 {
            continue;
        }
        let c = (t[0] + t[1] + t[2]) / 3.0;
        if seen.insert((c.x.floor() as i32, (c.y * 2.0).floor() as i32, c.z.floor() as i32)) {
            spots.push(c);
        }
    }

    // A flight: walking along f, the floor rises in like steps (3-60 cm) at like spacing.
    #[derive(Clone)]
    struct Flight {
        start: Vec3,
        dir: Vec3,
        rises: Vec<f32>,
        depth: f32,
    }
    // Each spot, each way: on all cores, then one per staircase in order.
    let search = |p: Vec3| -> Vec<Flight> {
        let mut out = vec![];
        for k in 0..16 {
            let a = k as f32 * std::f32::consts::TAU / 16.0;
            let f = Vec3::new(-a.sin(), 0.0, -a.cos());
            let mut y = p.y;
            let mut last_edge = 0.0;
            let mut rises = vec![];
            let mut gaps = vec![];
            let mut d = 0.05;
            while d < 6.0 {
                let q = p + f * d;
                let Some(top) = top_at(Vec3::new(q.x, y + 0.65, q.z), y + 0.65, y - 0.1) else { break };
                let rise = top - y;
                // Treads are flat: anything between 1.5 and 8 cm is a slope, not a stair.
                if rise.abs() > 0.015 && rise.abs() < 0.08 {
                    break;
                }
                if rise >= 0.08 {
                    if rise > 0.6 {
                        break;
                    }
                    if !rises.is_empty() {
                        gaps.push(d - last_edge);
                    }
                    rises.push(rise);
                    last_edge = d;
                    y = top;
                } else if rise < -0.015 {
                    break;
                }
                d += 0.02;
            }
            if rises.len() < 3 {
                continue;
            }
            let mean = rises.iter().sum::<f32>() / rises.len() as f32;
            let depth = gaps.iter().sum::<f32>() / gaps.len() as f32;
            let alike = rises.iter().all(|r| (r - mean).abs() < 0.05) && gaps.iter().all(|g| (g - depth).abs() < 0.08);
            if !alike || depth < 0.15 || depth > 0.8 {
                continue;
            }
            out.push(Flight { start: p, dir: f, rises, depth });
        }
        out
    };
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = spots.len().div_ceil(cores).max(1);
    let found: Vec<Flight> = std::thread::scope(|sc| {
        let handles: Vec<_> = spots.chunks(chunk).map(|part| sc.spawn(|| part.iter().flat_map(|&p| search(p)).collect::<Vec<_>>())).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let mut flights: Vec<Flight> = vec![];
    for fl in found {
        // One per staircase: skip flights starting near one already found going the same way.
        if flights.iter().any(|o| o.dir.dot(fl.dir) > 0.9 && o.start.distance(fl.start) < 2.5) {
            continue;
        }
        flights.push(fl);
    }
    println!("{} flights", flights.len());
    let mut all: Vec<f32> = flights.iter().flat_map(|f| f.rises.iter().copied()).collect();
    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if all.is_empty() {
        return;
    }
    let pct = |q: f32| all[((all.len() - 1) as f32 * q) as usize] * 100.0;
    println!(
        "risers: {} — min {:.1} cm, median {:.1}, 90% {:.1}, max {:.1}; walk steps up to {:.0} cm",
        all.len(),
        pct(0.0),
        pct(0.5),
        pct(0.9),
        pct(1.0),
        tu.step_height * 100.0
    );
    let depths: Vec<f32> = flights.iter().map(|f| f.depth).collect();
    println!("treads: median {:.0} cm", {
        let mut d = depths.clone();
        d.sort_by(|a, b| a.partial_cmp(b).unwrap());
        d[d.len() / 2] * 100.0
    });

    // Walk (and run) up each: from the floor at its foot, already going at the input's speed;
    // her speed on it against the same input on the flat.
    for (label, push) in [("walking", 0.4f32), ("running", 1.0)] {
        let input = Input { move_axis: Vec2::new(0.0, push), ..Default::default() };
        let flat_speed = {
            let mut w = faith_move::BoxWorld::default();
            w.add(faith_move::Aabb::new(Vec3::new(-100.0, -1.0, -100.0), Vec3::new(100.0, 0.0, 100.0)));
            let mut c = Controller::new(tu.clone(), Vec3::ZERO, 0.0);
            c.state = State::Ground;
            for _ in 0..240 {
                c.step(1.0 / 60.0, &input, &w);
            }
            c.horizontal_speed()
        };
        let (mut up, mut kept, mut n, mut max_mesh) = (0, 0.0, 0, 0.0f32);
        for fl in &flights {
            let yaw = (-fl.dir.x).atan2(-fl.dir.z);
            // Back off the first riser by her half width (the flight's start is just before it),
            // onto the same floor.
            let back = fl.start - fl.dir * (tu.half_width + 0.05);
            let Some(floor) = top_at(back, fl.start.y + 0.1, fl.start.y - 0.1) else { continue };
            let start = Vec3::new(back.x, floor, back.z);
            let mut c = Controller::new(tu.clone(), start, yaw);
            c.state = State::Ground;
            c.vel = fl.dir * flat_speed;
            let goal = fl.start.y + fl.rises.iter().sum::<f32>() - 0.05;
            let mut frames = 0;
            let debug = std::env::var("STEPS_DEBUG").ok().and_then(|v| {
                let mut it = v.split(',').map(|x| x.parse::<f32>().ok());
                Some((it.next()??, it.next()??))
            });
            let debug = debug.is_some_and(|(x, z)| (fl.start.x - x).abs() < 0.05 && (fl.start.z - z).abs() < 0.05) && label == "walking";
            if debug {
                for k in 0..40 {
                    let q = fl.start + fl.dir * (k as f32 * 0.05);
                    let tops = tops_below(&world, q, 0.05, fl.start.y + 1.5, fl.start.y - 0.5);
                    println!("    profile {:.2} m: {:?}", k as f32 * 0.05, tops.iter().map(|t| (t - fl.start.y) * 100.0).collect::<Vec<_>>());
                }
            }
            for _ in 0..600 {
                c.step(1.0 / 60.0, &input, &world);
                if debug && frames == 30 {
                    let body = faith_move::Body { half_width: tu.half_width, height: tu.stand_height };
                    let half = body.aabb(Vec3::ZERO).size() * 0.5;
                    let centre = c.feet + Vec3::Y * half.y;
                    println!("    up 0.35: {:?}", world.sweep(half, centre, Vec3::Y * tu.step_height));
                    let raised = centre + Vec3::Y * tu.step_height;
                    println!("    then on 10 cm: {:?}", world.sweep(half, raised, fl.dir * 0.1));
                    println!("    level on 10 cm: {:?}", world.sweep(half, centre, fl.dir * 0.1));
                    let side = Vec3::Y.cross(fl.dir).normalize();
                    for sx in [-0.3f32, -0.2, -0.1, 0.0, 0.1, 0.2, 0.3] {
                        for d in [0.05f32, 0.15, 0.3] {
                            let q = c.feet + fl.dir * (half.z + d) + side * sx;
                            let tops = tops_below(&world, q, 0.02, c.feet.y + 2.3, c.feet.y - 0.2);
                            println!("    side {sx:+.1} ahead {d:.2}: {:?}", tops.iter().map(|t| ((t - c.feet.y) * 100.0).round()).collect::<Vec<_>>());
                        }
                    }
                    println!("    half {half:?}");
                    // The slide, sweep by sweep.
                    let mut f = c.feet;
                    let mut rem = fl.dir * 0.03;
                    for k in 0..4 {
                        let me = body.aabb(f);
                        let h = world.sweep(me.size() * 0.5, me.center(), rem);
                        println!("    sweep {k} from {:?} rem {rem:?}: {h:?}", f - fl.start);
                        match h {
                            None => break,
                            Some(h) => {
                                f += rem * h.t;
                                rem *= 1.0 - h.t;
                                let into = rem.dot(h.normal);
                                if into < 0.0 { rem -= h.normal * into; }
                            }
                        }
                    }
                    let mut f2 = c.feet;
                    let n = faith_move::world::slide_move(&world, body, &mut f2, fl.dir * 0.03);
                    println!("    slide_move -> {:?} {n:?}", f2 - fl.start);
                }
                if debug && frames < 160 && frames % 6 == 0 {
                    println!("    f{frames} feet {:?} vel {:?} {:?}", c.feet - fl.start, c.vel, c.state);
                }
                max_mesh = max_mesh.max(c.mesh_offset.abs());
                frames += 1;
                if c.feet.y >= goal {
                    break;
                }
            }
            n += 1;
            if c.feet.y >= goal {
                up += 1;
                let along = (c.feet - start).dot(fl.dir);
                kept += along / (frames as f32 / 60.0) / flat_speed;
            } else if std::env::var_os("STEPS_VERBOSE").is_some() {
                let max = fl.rises.iter().cloned().fold(0.0, f32::max);
                println!(
                    "  {label} stuck: flight at {:?} dir {:?}, {} risers up to {:.0} cm, treads {:.0} cm; she got {:.2} m of {:.2} up, {:?}",
                    fl.start,
                    fl.dir,
                    fl.rises.len(),
                    max * 100.0,
                    fl.depth * 100.0,
                    c.feet.y - fl.start.y,
                    goal - fl.start.y,
                    c.state
                );
            }
        }
        println!(
            "{label} ({flat_speed:.1} m/s on the flat): up {up} of {n} flights, at {:.0}% of that speed on the ones she climbed, mesh carried up to {:.0} cm",
            kept / up.max(1) as f32 * 100.0,
            max_mesh * 100.0
        );
    }
}
