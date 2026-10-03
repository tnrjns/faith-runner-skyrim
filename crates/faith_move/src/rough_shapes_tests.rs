//! Rough, real-world shapes as triangles (the kind of collision a host game has, nothing
//! built for parkour): thin fences, crates, sloped roof edges, bevelled wall tops, tilted
//! rocks. Each has to work as the ledge, vault or mantle it looks like.

use glam::{Vec2, Vec3};

use crate::world::MeshWorld;
use crate::*;

const DT: f32 = 1.0 / 60.0;

fn floor() -> Vec<[Vec3; 3]> {
    MeshWorld::oriented_box(Vec3::new(0.0, -0.5, 0.0), Vec3::new(50.0, 0.5, 50.0), 0.0)
}

/// A box from `lo` to `hi`.
fn cuboid(lo: Vec3, hi: Vec3) -> Vec<[Vec3; 3]> {
    MeshWorld::oriented_box((lo + hi) * 0.5, (hi - lo) * 0.5, 0.0)
}

/// A solid between z0 (front) and z1 (back), x within ±w, whose top runs from y0 at the front
/// to y1 at the back: a wall with a sloped top, a tilted rock.
fn wedge(z0: f32, z1: f32, w: f32, y0: f32, y1: f32) -> Vec<[Vec3; 3]> {
    let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    let (fl, fr, bl, br) = (v(-w, 0.0, z0), v(w, 0.0, z0), v(-w, 0.0, z1), v(w, 0.0, z1));
    let (ftl, ftr, btl, btr) = (v(-w, y0, z0), v(w, y0, z0), v(-w, y1, z1), v(w, y1, z1));
    let mut t = vec![];
    t.extend(MeshWorld::quad(fl, fr, ftr, ftl)); // front
    t.extend(MeshWorld::quad(br, bl, btl, btr)); // back
    t.extend(MeshWorld::quad(ftl, ftr, btr, btl)); // top
    t.extend(MeshWorld::quad(bl, fl, ftl, btl)); // left
    t.extend(MeshWorld::quad(fr, br, btr, ftr)); // right
    t
}

/// Sprint at the shape (down -Z from the origin), jumping `jump_at` metres before `front`;
/// what happened and where she ended up.
fn attempt(shape: Vec<[Vec3; 3]>, front: f32, jump_at: f32, secs: f32) -> (Vec<Event>, Controller) {
    let mut tris = floor();
    tris.extend(shape);
    let w = MeshWorld::new(tris, vec![]);
    let mut c = Controller::new(Tuning::default(), Vec3::new(0.0, 0.0, 6.0), 0.0);
    c.state = State::Ground;
    let mut ev = vec![];
    let mut t = 0.0;
    let mut jumped = false;
    while t < secs {
        let jump = !jumped && c.feet.z < front + jump_at;
        jumped |= jump;
        let i = Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: jump, jump_held: jumped, ..Default::default() };
        c.step(DT, &i, &w);
        ev.extend(c.events.iter().copied());
        t += DT;
    }
    (ev, c)
}

fn has(ev: &[Event], e: Event) -> bool {
    ev.iter().any(|x| std::mem::discriminant(x) == std::mem::discriminant(&e))
}

fn summary(name: &str, ev: &[Event], c: &Controller) -> String {
    let kinds: Vec<String> = ev
        .iter()
        .map(|e| format!("{e:?}").split(|ch: char| ch == ' ' || ch == '{' || ch == '(').next().unwrap_or("").to_string())
        .collect();
    format!("{name}: {kinds:?} -> at ({:.2}, {:.2}, {:.2}) {}", c.feet.x, c.feet.y, c.feet.z, c.state.name())
}

/// What each shape does today (run with --nocapture).
#[test]
#[ignore]
fn report() {
    let cases: Vec<(&str, Vec<[Vec3; 3]>, f32, f32)> = vec![
        ("fence 1.0 m, 8 cm thick", cuboid(Vec3::new(-4.0, 0.0, -0.08), Vec3::new(4.0, 1.0, 0.0)), 0.0, 1.0),
        ("crate 0.9 m", cuboid(Vec3::new(-0.45, 0.0, -0.9), Vec3::new(0.45, 0.9, 0.0)), 0.0, 1.0),
        ("wall 1.3 m, 0.4 thick", cuboid(Vec3::new(-4.0, 0.0, -0.4), Vec3::new(4.0, 1.3, 0.0)), 0.0, 1.0),
        ("wall 3.0 m, 35 deg roof on top", wedge(0.0, -4.0, 4.0, 3.0, 3.0 + 4.0 * 35f32.to_radians().tan()), 0.0, 1.2),
        ("wall 2.6 m, bevelled top", {
            let mut t = cuboid(Vec3::new(-4.0, 0.0, -3.0), Vec3::new(4.0, 2.4, 0.0));
            t.extend(wedge(-0.2, -3.0, 4.0, 2.4, 2.6));
            t
        }, 0.0, 1.2),
        ("rock 1.0-1.4 m, tilted top", wedge(0.0, -2.0, 1.5, 1.0, 1.4), 0.0, 1.0),
        ("rock 1.6-2.2 m, tilted top", wedge(0.0, -2.5, 2.0, 1.6, 2.2), 0.0, 1.2),
    ];
    for (name, shape, front, jump_at) in cases {
        let (ev, c) = attempt(shape, front, jump_at, 3.0);
        eprintln!("{}", summary(name, &ev, &c));
    }
}

fn shapes() -> Vec<(&'static str, Vec<[Vec3; 3]>, f32)> {
    vec![
        ("fence", cuboid(Vec3::new(-4.0, 0.0, -0.08), Vec3::new(4.0, 1.0, 0.0)), 1.0),
        ("crate", cuboid(Vec3::new(-0.45, 0.0, -0.9), Vec3::new(0.45, 0.9, 0.0)), 1.0),
        ("roof", wedge(0.0, -4.0, 4.0, 3.0, 3.0 + 4.0 * 35f32.to_radians().tan()), 1.2),
        ("rock", wedge(0.0, -2.5, 2.0, 1.6, 2.2), 1.2),
    ]
}

/// A thin fence and a crate are vaulted.
#[test]
fn fences_and_crates_vault() {
    for (name, shape, jump_at) in shapes().into_iter().take(2) {
        let (ev, c) = attempt(shape, 0.0, jump_at, 3.0);
        assert!(has(&ev, Event::Vault), "{}", summary(name, &ev, &c));
        assert!(c.feet.z < -2.0 && c.feet.y.abs() < 0.05, "{}", summary(name, &ev, &c));
    }
}

/// A wall topped with a 35 degree roof: grabbed, and pulled up onto the slope.
#[test]
fn pulls_up_onto_a_sloped_roof() {
    let (name, shape, jump_at) = shapes().remove(2);
    let (ev, c) = attempt(shape, 0.0, jump_at, 3.0);
    assert!(has(&ev, Event::LedgeGrab) && has(&ev, Event::PullUp), "{}", summary(name, &ev, &c));
    assert!(c.feet.y > 3.0 && c.feet.z < 0.0, "on the roof: {}", summary(name, &ev, &c));
}

/// A tall tilted rock is climbed onto, never vaulted into.
#[test]
fn never_ends_up_inside_a_rock() {
    let (name, shape, jump_at) = shapes().remove(3);
    let (ev, c) = attempt(shape, 0.0, jump_at, 3.0);
    let inside = c.feet.z < 0.0 && c.feet.z > -2.5 && c.feet.y < 1.5;
    assert!(!inside, "inside the rock: {}", summary(name, &ev, &c));
}
