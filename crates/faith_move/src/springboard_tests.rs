//! The Springboard map: every red step-and-block springboards you where it
//! should, from a jump pressed anywhere in the last second of the run-up, and
//! none of the grey near misses do.

use crate::course_tests::{once, toward_x, Sim};
use crate::springboard::{self, z, BLOCK_Z, HIGH_DECK, LANE_X, LOW_DECK, STEP_Z, UP_DECK};
use crate::*;

fn sim(lane: usize) -> Sim {
    Sim::on(springboard::springboard(), lane)
}

/// Sprint down `lane`, press jump when the front of the body is `before` metres
/// from the step, and keep running until standing on the ground past `stop_z`.
fn springboard_lane(lane: usize, before: f32, stop_z: f32) -> Sim {
    let mut s = sim(lane);
    let mut j = false;
    s.run(8.0, |c, _| {
        if c.feet.z < stop_z && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, LANE_X[lane]);
        i.jump_pressed = once(&mut j, c.feet.z - 0.3 < STEP_Z + before);
        i
    });
    s
}

/// The press distances a person might use at a sprint: right at the step, up to
/// five metres out. ME's CheckDistanceTime is a second, and after this 20 m run-up you're doing
/// about 6.3 m/s (the sprint takes 7 s to reach 7.2), so the step is in reach from ~6 m.
const PRESS_AT: [f32; 6] = [0.5, 1.0, 2.0, 3.0, 4.0, 5.0];

fn check_lane(lane: usize, stop_z: f32, ok: impl Fn(&Sim) -> bool) {
    for before in PRESS_AT {
        let s = springboard_lane(lane, before, stop_z);
        assert!(s.has(Event::SpringBoard), "lane {}, jump {before} m out: {:?}", lane + 1, s.events);
        assert!(ok(&s), "lane {}, jump {before} m out: ended at {:?} {:?}", lane + 1, s.c.feet, s.events);
    }
}

#[test]
fn lane_1_up_onto_the_deck() {
    check_lane(0, z::DECK_START - 1.0, |s| (s.c.feet.y - UP_DECK).abs() < 0.05);
}

#[test]
fn lane_2_lowest() {
    check_lane(1, z::DECK_START - 1.0, |s| (s.c.feet.y - LOW_DECK).abs() < 0.05);
}

#[test]
fn lane_3_highest() {
    check_lane(2, z::DECK_START - 1.0, |s| (s.c.feet.y - HIGH_DECK).abs() < 0.05);
}

#[test]
fn lane_4_over_the_pit() {
    check_lane(3, z::PIT_END - 0.5, |s| s.c.feet.z < z::PIT_END && s.c.feet.y.abs() < 0.05);
}

/// Holding jump down the run-up springboards by itself.
#[test]
fn holding_jump_springboards() {
    let mut s = sim(0);
    s.run(8.0, |c, _| {
        if c.feet.z < z::DECK_START - 1.0 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, LANE_X[0]);
        i.jump_held = c.feet.z < STEP_Z + 10.0 && c.state == State::Ground;
        i
    });
    assert!(s.has(Event::SpringBoard), "{:?}", s.events);
    assert!((s.c.feet.y - UP_DECK).abs() < 0.05, "on the deck? {:?}", s.c.feet);
}

/// Too far out (well over a second away) it's just a jump.
#[test]
fn too_early_is_a_plain_jump() {
    let mut s = sim(0);
    let mut j = false;
    s.run(2.2, |c, _| {
        let mut i = toward_x(c, LANE_X[0]);
        i.jump_pressed = once(&mut j, c.feet.z < STEP_Z + 12.0);
        i
    });
    assert!(s.has(Event::Jump) && !s.has(Event::SpringBoard), "{:?}", s.events);
}

/// Standing still at the step you'd never reach it (ME divides the distance by your speed),
/// so there's nothing to launch from. Any run-up at all will do, though: there's no minimum.
#[test]
fn not_from_a_standstill() {
    let mut s = sim(0);
    s.c.feet.z = STEP_Z + 0.6;
    let mut j = false;
    s.run(2.0, |_, _| {
        let mut i = Input::default();
        i.move_axis.y = 1.0;
        i.jump_pressed = once(&mut j, true);
        i
    });
    assert!(!s.has(Event::SpringBoard), "{:?}", s.events);
}

/// Lane 5: vault the rail, climb the stepless block, mantle onto the deep one.
/// No springboard anywhere.
#[test]
fn lane_5_near_misses() {
    let mut s = sim(4);
    let (mut rail, mut lone, mut deep) = (false, false, false);
    s.run(12.0, |c, _| {
        if c.feet.z < z::DEEP - 1.0 && c.feet.y > 1.0 {
            return Input::default(); // standing on the deep block
        }
        let mut i = toward_x(c, LANE_X[4]);
        i.jump_pressed |= once(&mut rail, c.feet.z - 0.3 < z::RAIL + 0.6);
        i.jump_pressed |= once(&mut lone, c.feet.z < z::LONE + 2.0 && c.state == State::Ground);
        i.jump_pressed |= once(&mut deep, c.feet.z < z::DEEP + 1.0 && c.state == State::Ground);
        i
    });
    assert!(!s.has(Event::SpringBoard), "{:?}", s.events);
    assert!(s.has(Event::Vault), "vaulted the rail: {:?}", s.events);
    let climbs = s.events.iter().filter(|e| matches!(e, Event::Mantle | Event::PullUp)).count();
    assert!(climbs >= 2, "climbed both blocks: {:?}", s.events);
    assert!((s.c.feet.y - 1.2).abs() < 0.05, "standing on the deep block: {:?} {:?}", s.c.feet, s.events);
}

/// Walking around the range never moves your checkpoint: R always takes you
/// back to the lane you picked.
#[test]
fn checkpoint_stays_on_the_picked_lane() {
    let level = springboard::springboard();
    for &x in &LANE_X {
        assert_eq!(level.checkpoint_at(glam::Vec3::new(x, 0.0, springboard::START_Z)), Some(0));
    }
    let _ = BLOCK_Z;
}

