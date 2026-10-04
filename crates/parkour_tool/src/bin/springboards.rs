//! Where Faith springboards in a surveyed Skyrim city: Mirror's Edge's springboard (a step of
//! 64 cm, give or take 20, with something higher 112 uu behind it) looked for everywhere she
//! can run, and each place found run with faith_move (sprinting at it, jump held).
//!
//! Usage: springboards <survey.bin>

use faith_move::world::tops_below;
use faith_move::{Controller, Event, Input, MeshWorld, State, Tuning};
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
    let tu = Tuning { kill_y: None, respawn_on_death: false, ..Tuning::default() };

    // Where she can run: the middles of upward-facing triangles, one per half metre.
    let mut seen = std::collections::HashSet::new();
    let mut spots = vec![];
    for t in &tris {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        if n.length_squared() < 1e-8 || n.normalize().y.abs() < 0.85 {
            continue;
        }
        let c = (t[0] + t[1] + t[2]) / 3.0;
        if seen.insert(((c.x * 2.0) as i32, (c.y * 2.0) as i32, (c.z * 2.0) as i32)) {
            spots.push(c);
        }
    }
    let (lo, hi) = (tu.springboard_step_height - tu.springboard_slack, tu.springboard_step_height + tu.springboard_slack);
    let spacing = tu.springboard_step_spacing;
    let top_at = |p: Vec3, from: f32, to: f32| tops_below(&world, p, 0.05, from, to).first().copied();
    let mut candidates = vec![];
    for &p in &spots {
        for k in 0..16 {
            let a = k as f32 * std::f32::consts::TAU / 16.0;
            let f = Vec3::new(-a.sin(), 0.0, -a.cos());
            for i in 3..15 {
                let q = p + f * (i as f32 * 0.1);
                let Some(step) = top_at(q, p.y + hi + 0.02, p.y + 0.2) else { continue };
                let h = step - p.y;
                if h < lo || h > hi {
                    continue;
                }
                let back = q + f * spacing;
                if let Some(block) = top_at(back, p.y + tu.springboard_hi + 0.1, step + 0.05) {
                    let rise = block - p.y;
                    if rise >= tu.springboard_lo && rise <= tu.springboard_hi {
                        candidates.push((p, a, i as f32 * 0.1));
                    }
                }
                break;
            }
        }
    }
    // Run each: start 4 m back, sprinting, jump held.
    let mut works = vec![];
    for &(p, yaw, dist) in &candidates {
        let f = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        let start = p - f * (4.0 - dist).max(1.0);
        let Some(floor) = top_at(start + Vec3::Y * 0.5, start.y + 0.5, start.y - 0.5) else { continue };
        let mut c = Controller::new(tu.clone(), Vec3::new(start.x, floor, start.z), yaw);
        c.state = State::Ground;
        c.vel = f * 6.5;
        let mut sprang = false;
        for _ in 0..90 {
            let i = Input { move_axis: Vec2::new(0.0, 1.0), jump_held: true, ..Default::default() };
            c.step(1.0 / 60.0, &i, &world);
            if c.events.iter().any(|e| matches!(e, Event::SpringBoard)) {
                sprang = true;
                break;
            }
        }
        if sprang && !works.iter().any(|w: &Vec3| w.distance(p) < 2.0) {
            works.push(p);
        }
    }
    println!(
        "{}: {} triangles, {} places to run; {} rough step-and-block spots (measured from anywhere on the step), {} where she really springboards",
        path,
        tris.len(),
        spots.len(),
        candidates.len(),
        works.len()
    );
    for w in works.iter().take(20) {
        println!("  at Skyrim ({:.0} {:.0} {:.0})", w.x * UNITS, -w.z * UNITS, w.y * UNITS);
    }
}
