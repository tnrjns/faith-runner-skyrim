//! The Rooftops map, section by section, beaten with scripted input.

use glam::Vec2;

use crate::course_tests::{once, toward_x, AngleIn, Sim};
use crate::rooftops::{self, z, BILLBOARD_X, R2_Y, R3_Y, R4_Y, R6_Y, R7_Y};
use crate::*;

fn sim(cp: usize) -> Sim {
    Sim::on(rooftops::rooftops(), cp)
}

fn fwd() -> Input {
    Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() }
}

#[test]
fn r1_rail_skylight_pipe_gap() {
    let mut s = sim(0);
    let (mut v, mut m, mut sl, mut g) = (false, false, false, false);
    s.run(10.0, |c, _| {
        if c.feet.z < z::R2_START - 2.0 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed |= once(&mut v, c.feet.z < z::RAIL + 1.0);
        i.jump_pressed |= once(&mut m, c.feet.z < z::SKYLIGHT + 0.8 && c.feet.z > z::SKYLIGHT - 1.0);
        i.crouch_pressed |= once(&mut sl, c.feet.z < z::PIPE + 2.6 && c.state == State::Ground);
        // Hold crouch from the press on (you can't coil off a plain drop, so holding it in the
        // air beforehand would just land you crouched instead of sliding).
        i.crouch_held = sl && c.feet.z > z::PIPE - 0.8;
        i.jump_pressed |= once(&mut g, c.feet.z < z::R1_END + 0.5 && c.state == State::Ground);
        i
    });
    assert!(s.has(Event::Vault), "{:?}", s.events);
    assert!(s.has(Event::Mantle) || s.has(Event::Vault), "{:?}", s.events);
    assert!(s.has(Event::Slide), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("R2 Billboards"), "ended at {:?} {:?}", s.c.feet, s.events);
}

#[test]
fn r2_billboard_wallrun() {
    let mut s = sim(1);
    let mut j = false;
    let mut angle = AngleIn::default();
    s.run(7.0, |c, _| {
        if c.feet.z < z::R3_START - 3.0 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, BILLBOARD_X - 0.6);
        i.jump_pressed = once(&mut j, c.feet.z < z::R2_END + 1.3);
        angle.apply(c, &mut i, 25.0);
        i
    });
    assert!(s.has(Event::WallRunStart), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("R3 Tanks"), "ended at {:?}", s.c.feet);
    assert!((s.c.feet.y - R3_Y).abs() < 0.05);
}

#[test]
fn r2_girder_walk() {
    let mut s = sim(1);
    let mut v = false;
    s.run(9.0, |c, _| {
        if c.feet.z < z::R3_START - 2.0 {
            return Input::default();
        }
        // Vault the low wall, line up, then walk the girder slow and straight.
        let mut i = toward_x(c, -6.0);
        i.jump_pressed = once(&mut v, c.feet.z < z::LOW_WALL + 0.9);
        if c.feet.z < z::R2_END + 4.0 {
            i.move_axis = Vec2::new(((-6.0 - c.feet.x) * 2.0).clamp(-0.3, 0.3), 0.6);
        }
        i
    });
    assert!(s.has(Event::Vault), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "fell off at {:?} {:?}", s.c.feet, s.events);
    assert_eq!(s.checkpoint(), Some("R3 Tanks"), "ended at {:?}", s.c.feet);
}

#[test]
fn r3_climb_to_upper() {
    let mut s = sim(2);
    let mut j = false;
    s.run(7.0, |c, _| {
        if c.feet.y > R4_Y - 0.1 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::CLIMB + 1.2);
        i
    });
    assert!(s.has(Event::WallClimbStart), "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("R4 Upper"), "ended at {:?} {:?}", s.c.feet, s.events);
}

#[test]
fn r3_crate_route() {
    let mut s = sim(2);
    let (mut a, mut b) = (false, false);
    s.run(8.0, |c, _| {
        if c.feet.y > R4_Y - 0.1 && c.state == State::Ground {
            return Input::default();
        }
        if matches!(c.state, State::LedgeHang { .. } | State::Traverse(_)) {
            return fwd();
        }
        let mut i = toward_x(c, -6.0);
        i.jump_pressed |= once(&mut a, c.feet.z < z::CLIMB + 4.0 && c.feet.y < R3_Y + 0.5);
        i.jump_pressed |= once(&mut b, c.feet.y > R3_Y + 1.2 && c.state == State::Ground && c.feet.z < z::CLIMB + 1.5);
        i
    });
    assert!(s.has(Event::Mantle) || s.has(Event::Vault), "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("R4 Upper"), "ended at {:?} {:?}", s.c.feet, s.events);
}

#[test]
fn r4_slalom_and_girder() {
    let mut s = sim(3);
    s.run(9.0, |c, _| {
        if c.feet.z < z::R5_START - 2.0 {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        if c.feet.z < z::R4_END + 3.0 {
            i.move_axis = Vec2::new((-c.feet.x * 2.0).clamp(-0.3, 0.3), 0.7);
        }
        i
    });
    assert_eq!(s.deaths, 0, "{:?} at {:?}", s.events, s.c.feet);
    assert!(s.c.feet.z < z::R5_START && (s.c.feet.y - R4_Y).abs() < 0.05, "{:?}", s.c.feet);
}

#[test]
fn r5_drop_needs_roll() {
    // Start on R5 (spawn just past the girder).
    let mut level = rooftops::rooftops();
    level.checkpoints[3].spawn = glam::Vec3::new(0.0, R4_Y, z::R5_START - 1.0);
    let mut s = Sim::on(level, 3);
    s.run(4.0, |c, _| {
        let mut i = toward_x(c, 0.0);
        i.crouch_pressed = c.state == State::Air && c.feet.y < R6_Y + 1.0 && c.vel.y < 0.0;
        i
    });
    assert!(s.has(Event::Roll), "{:?}", s.events);
    assert!(!s.has(Event::HardLand), "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("R6 Plaza"));
}

#[test]
fn r6_duct_vents_climb_finish() {
    let mut s = sim(4);
    let (mut sl, mut va, mut vb, mut cl) = (false, false, false, false);
    s.run(12.0, |c, _| {
        if s_done(c) {
            return Input::default();
        }
        if matches!(c.state, State::LedgeHang { .. } | State::Traverse(_)) {
            return fwd();
        }
        let mut i = toward_x(c, 0.0);
        i.crouch_pressed |= once(&mut sl, c.feet.z < z::DUCT + 2.8 && c.state == State::Ground);
        i.crouch_held = c.feet.z < z::DUCT + 2.8 && c.feet.z > z::DUCT - 1.0;
        i.jump_pressed |= once(&mut va, c.feet.z < z::VENT_A + 0.9 && c.feet.z > z::VENT_A - 0.5);
        i.jump_pressed |= once(&mut vb, c.feet.z < z::VENT_B + 0.9 && c.feet.z > z::VENT_B - 0.5);
        i.jump_pressed |= once(&mut cl, c.feet.z < z::FINISH_WALL + 1.2 && c.feet.y < R6_Y + 0.5);
        i
    });
    fn s_done(c: &Controller) -> bool {
        c.feet.z < z::FINISH + 1.0
    }
    assert!(s.has(Event::Slide), "{:?}", s.events);
    assert!(s.has(Event::WallClimbStart), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert!(s.level.in_finish(s.c.feet), "finish? feet {:?} events {:?}", s.c.feet, s.events);
    assert!((s.c.feet.y - R7_Y).abs() < 0.05);
}

#[test]
fn checkpoints_are_grounded() {
    let level = rooftops::rooftops();
    let w = level.world();
    for cp in &level.checkpoints {
        let mut c = Controller::new(Tuning::default(), cp.spawn + glam::Vec3::Y * 0.01, cp.yaw);
        for _ in 0..30 {
            c.step(1.0 / 60.0, &Input::default(), &w);
        }
        assert_eq!(c.state, State::Ground, "{} spawn not grounded: {:?}", cp.name, c.feet);
        assert_eq!(level.checkpoint_at(c.feet).map(|i| level.checkpoints[i].name), Some(cp.name));
    }
}

#[test]
fn falling_off_is_fatal() {
    let mut s = sim(1);
    s.run(6.0, |c, _| toward_x(c, -12.0)); // straight off the left side
    assert!(s.deaths > 0 || s.c.feet.y > R2_Y - 0.5, "{:?}", s.c.feet);
}
