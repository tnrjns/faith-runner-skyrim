//! The controller on triangle geometry at any angle ([`MeshWorld`]): the scenes of the box
//! tests rebuilt out of triangles and turned off the world grid, which has to make no
//! difference, plus slopes and walls you meet at an angle.

use glam::{Quat, Vec2, Vec3};

use crate::course_tests::AngleIn;
use crate::world::{Aabb, BoxWorld, MeshWorld, World};
use crate::*;

const DT: f32 = 1.0 / 60.0;
/// Every scene turned this far off the grid.
const TURN: f32 = 27.0 * std::f32::consts::PI / 180.0;

fn run(c: &mut Controller, w: &dyn World, secs: f32, mut f: impl FnMut(&Controller, Vec3) -> Input) -> Vec<Event> {
    let mut t = 0.0;
    let mut ev = Vec::new();
    let back = Quat::from_rotation_y(-TURN);
    while t < secs {
        // The scene's own frame, so the scripts read like the box tests'.
        let local = back * c.feet;
        let i = f(c, local);
        c.step(DT, &i, w);
        ev.extend(c.events.iter().copied());
        t += DT;
    }
    ev
}

fn has(ev: &[Event], e: Event) -> bool {
    ev.iter().any(|x| std::mem::discriminant(x) == std::mem::discriminant(&e))
}

fn fwd() -> Input {
    Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() }
}

/// Boxes (min, max, in the scene's frame) as triangles, the whole scene turned by TURN about
/// the origin.
fn turned(boxes: &[(Vec3, Vec3)]) -> MeshWorld {
    let r = Quat::from_rotation_y(TURN);
    let mut tris = vec![];
    for (lo, hi) in boxes {
        let c = (*lo + *hi) * 0.5;
        tris.extend(MeshWorld::oriented_box(r * c, (*hi - *lo) * 0.5, TURN));
    }
    MeshWorld::new(tris, vec![])
}

fn floor_box() -> (Vec3, Vec3) {
    (Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0))
}

/// Facing down the scene's -Z, turned with it.
fn ctrl_at(local: Vec3) -> Controller {
    let mut c = Controller::new(Tuning::default(), Quat::from_rotation_y(TURN) * local, TURN);
    c.state = State::Ground;
    c
}

/// Box and triangle sweeps of the same boxes agree: same contact (to a millimetre) and normal.
#[test]
fn mesh_and_box_sweeps_agree() {
    let mut seed = 11u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1 << 24) as f32
    };
    let mut boxes = vec![];
    let mut tris = vec![];
    for _ in 0..60 {
        let c = Vec3::new(rnd() * 20.0 - 10.0, rnd() * 4.0, rnd() * 20.0 - 10.0);
        let h = Vec3::new(rnd() * 1.5 + 0.2, rnd() * 1.0 + 0.2, rnd() * 1.5 + 0.2);
        boxes.push(Aabb::new(c - h, c + h));
        tris.extend(MeshWorld::oriented_box(c, h, 0.0));
    }
    let bw = BoxWorld { boxes: boxes.clone(), ..Default::default() };
    let mw = MeshWorld::new(tris, vec![]);
    let half = Vec3::new(0.3, 0.9, 0.3);
    let (mut hits, mut tested) = (0, 0);
    for _ in 0..400 {
        let start = Vec3::new(rnd() * 24.0 - 12.0, rnd() * 4.0, rnd() * 24.0 - 12.0);
        let me = Aabb::new(start - half, start + half);
        if bw.overlaps(&me) {
            continue;
        }
        let delta = Vec3::new(rnd() * 8.0 - 4.0, rnd() * 2.0 - 1.0, rnd() * 8.0 - 4.0);
        tested += 1;
        let (a, b) = (bw.sweep(half, start, delta), mw.sweep(half, start, delta));
        match (a, b) {
            (Some(a), Some(b)) => {
                hits += 1;
                // Compared across the surface (along the path a grazing contact is ill-conditioned).
                let gap = ((a.t - b.t) * delta.dot(a.normal)).abs().min(((a.t - b.t) * delta.dot(b.normal)).abs());
                assert!(gap < 0.001, "contact differs by {gap} m from {start:?} along {delta:?}: {a:?} vs {b:?}");
                // At an edge either face is right; otherwise they must match.
                let edge = a.normal.dot(delta) < 0.0 && b.normal.dot(delta) < 0.0;
                assert!(a.normal.dot(b.normal) > 0.99 || edge, "{:?} vs {:?}", a.normal, b.normal);
            }
            (None, None) => {}
            (a, b) => panic!("from {start:?} along {delta:?}: boxes {a:?}, triangles {b:?}"),
        }
    }
    assert!(tested > 200 && hits > 40, "{tested} sweeps, {hits} hits");
}

/// Wallclimb, hang and pull up onto a 3.3 m wall turned off the grid.
#[test]
fn climbs_a_turned_wall() {
    let w = turned(&[floor_box(), (Vec3::new(-5.0, 0.0, -10.0), Vec3::new(5.0, 3.3, -4.0))]);
    let mut c = ctrl_at(Vec3::ZERO);
    let mut done = false;
    let ev = run(&mut c, &w, 4.0, |c, p| {
        done |= c.feet.y > 3.2 && c.state == State::Ground;
        if done {
            return Input::default();
        }
        Input { jump_pressed: p.z < -3.0 && p.z > -3.5, ..fwd() }
    });
    assert!(has(&ev, Event::WallClimbStart), "events {ev:?}");
    assert!(has(&ev, Event::LedgeGrab), "events {ev:?}");
    assert!(has(&ev, Event::PullUp), "events {ev:?}");
    assert!((c.feet.y - 3.3).abs() < 0.05, "on top: y = {}", c.feet.y);
}

/// Wallrun along a wall turned off the grid.
#[test]
fn wallruns_along_a_turned_wall() {
    let w = turned(&[floor_box(), (Vec3::new(1.0, 0.0, -40.0), Vec3::new(1.5, 5.0, -3.0))]);
    let mut c = ctrl_at(Vec3::new(0.3, 0.0, 0.0));
    let mut run_start = None;
    let mut travel = 0.0f32;
    let mut pressed = false;
    let mut angle = AngleIn::default();
    let ev = run(&mut c, &w, 4.0, |c, p| {
        if let State::WallRun { normal, .. } = c.state {
            // The wall's own normal, not a grid axis.
            let n_local = Quat::from_rotation_y(-TURN) * normal;
            assert!(n_local.dot(Vec3::NEG_X) > 0.99, "wall normal {n_local:?}");
            let s = *run_start.get_or_insert(p.z);
            travel = travel.max(s - p.z);
        }
        let jump = !pressed && p.z < -9.0;
        pressed |= jump;
        let mut i = Input { jump_pressed: jump, ..fwd() };
        angle.apply(c, &mut i, 25.0);
        i
    });
    assert!(has(&ev, Event::WallRunStart), "{ev:?}");
    assert!(travel > 5.0, "wallran {travel} m");
}

/// Vault a waist-high rail turned off the grid, keeping momentum.
#[test]
fn vaults_a_turned_rail() {
    let w = turned(&[floor_box(), (Vec3::new(-5.0, 0.0, -5.2), Vec3::new(5.0, 1.0, -5.0))]);
    let mut c = ctrl_at(Vec3::ZERO);
    let ev = run(&mut c, &w, 2.5, |_, p| Input { jump_pressed: p.z < -4.2 && p.z > -4.9, ..fwd() });
    let p = Quat::from_rotation_y(-TURN) * c.feet;
    assert!(has(&ev, Event::Vault), "events {ev:?}");
    assert!(p.z < -6.0, "past the rail: {p:?}");
    assert!(c.horizontal_speed() > 4.0, "kept momentum: {}", c.horizontal_speed());
}

/// Running into a wall at a glancing angle slides along it instead of stopping dead.
#[test]
fn slides_along_a_wall_at_an_angle() {
    // A wall 20 degrees off the line of the run (in the grid frame: no TURN here): its long
    // axis (local x) turned 70 degrees from x, crossing the run 6 m ahead.
    let wall = MeshWorld::oriented_box(Vec3::new(0.0, 2.0, -6.0), Vec3::new(20.0, 2.0, 0.2), 70f32.to_radians());
    let mut tris = wall;
    tris.extend(MeshWorld::oriented_box(Vec3::new(0.0, -0.5, 0.0), Vec3::new(50.0, 0.5, 50.0), 0.0));
    let w = MeshWorld::new(tris, vec![]);
    let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
    c.state = State::Ground;
    let mut touched = None;
    let mut t = 0.0;
    while t < 3.0 {
        c.step(DT, &fwd(), &w);
        t += DT;
        if touched.is_none() && c.feet.z < -4.0 {
            touched = Some(c.feet);
        }
    }
    let at = touched.expect("reached the wall");
    assert!(c.horizontal_speed() > 3.0, "still running along it: {}", c.horizontal_speed());
    assert!((c.feet - at).length() > 2.0, "slid {} m", (c.feet - at).length());
}

/// Walk up a 20 degree ramp made of two triangles.
#[test]
fn walks_up_a_triangle_ramp() {
    let slope = 20f32.to_radians();
    let len = 10.0;
    let rise = len * slope.tan();
    let a = Vec3::new(-3.0, 0.0, -2.0);
    let b = Vec3::new(3.0, 0.0, -2.0);
    let cc = Vec3::new(3.0, rise, -2.0 - len);
    let d = Vec3::new(-3.0, rise, -2.0 - len);
    let mut tris: Vec<[Vec3; 3]> = MeshWorld::quad(a, b, cc, d).to_vec();
    tris.extend(MeshWorld::oriented_box(Vec3::new(0.0, -0.5, 0.0), Vec3::new(50.0, 0.5, 50.0), 0.0));
    let w = MeshWorld::new(tris, vec![]);
    let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
    c.state = State::Ground;
    let mut t = 0.0;
    let mut best = 0.0f32;
    while t < 3.0 {
        c.step(DT, &fwd(), &w);
        t += DT;
        best = best.max(c.feet.y);
    }
    assert!(best > rise * 0.6, "climbed to {best} of {rise}");
}

/// Sunk a hair into the ground (rounding far from the origin, a world refreshed under you), you
/// stand on it instead of dropping through; from deep inside something you still get out.
#[test]
fn a_hair_into_the_ground_holds() {
    let w = turned(&[floor_box()]);
    let mut c = ctrl_at(Vec3::ZERO);
    c.feet.y = -0.01; // 1 cm into the floor
    c.state = State::Air;
    run(&mut c, &w, 1.0, |_, _| Input::default());
    assert!(c.feet.y > -0.02 && c.state == State::Ground, "fell to {} ({})", c.feet.y, c.state.name());
    // Far from the origin too (2.5 km out, where a float only resolves a quarter millimetre).
    let far = Vec3::new(2500.0, 0.0, -1400.0);
    let w = MeshWorld::new(MeshWorld::oriented_box(far + Vec3::new(0.0, -0.5, 0.0), Vec3::new(200.0, 0.5, 200.0), 0.3), vec![]);
    let mut c = Controller::new(Tuning::default(), far + Vec3::Y * 0.5, 0.0);
    let mut fell = false;
    for i in 0..600 {
        let input = Input { move_axis: Vec2::new((i as f32 * 0.05).sin(), 1.0), jump_pressed: i % 90 == 0, ..Default::default() };
        c.step(1.0 / 60.0, &input, &w);
        fell |= c.feet.y < -0.3;
    }
    assert!(!fell, "fell through at {:?}", c.feet);
}
