//! The Moves map's special moves, each beaten (and failed) with scripted input.

use glam::Vec2;

use crate::course_tests::{once, toward_x, Sim};
use crate::moves::{self, z, M2_Y, M4_Y};
use crate::*;

fn sim(cp: usize) -> Sim {
    Sim::on(moves::moves(), cp)
}

#[test]
fn springboard_clears_the_wide_gap() {
    let mut s = sim(0);
    let mut j = false;
    s.run(8.0, |c, _| {
        if c.feet.z < z::M2_START - 2.0 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::STEP_START + 3.0);
        i
    });
    assert!(s.has(Event::SpringBoard), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("M2 Balance"), "ended at {:?} {:?}", s.c.feet, s.events);
}

#[test]
fn springboard_launches_higher_than_a_jump() {
    let mut s = sim(0);
    let mut j = false;
    let mut peak = 0.0f32;
    s.run(7.0, |c, _| {
        peak = peak.max(c.feet.y);
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::STEP_START + 3.0);
        i
    });
    // A plain jump from the block's top would rise 1.2 m; the springboard ~2.7.
    assert!(peak > moves::BLOCK_TOP + 2.3, "peak {peak}");
}

/// Holding jump the whole run-up springboards by itself at the block.
#[test]
fn holding_jump_springboards() {
    let mut s = sim(0);
    s.run(8.0, |c, _| {
        if c.feet.z < z::M2_START - 2.0 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        // Hold from halfway down the run-up (not pressed at any exact moment).
        i.jump_held = c.feet.z < z::STEP_START + 8.0 && c.state == State::Ground;
        i
    });
    assert!(s.has(Event::SpringBoard), "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("M2 Balance"), "ended at {:?}", s.c.feet);
}

/// Without the springboard, the only way up is to climb the wall.
#[test]
fn plain_jump_needs_a_wallclimb_to_reach_m2() {
    let mut s = sim(0);
    let mut j = false;
    s.run(6.0, |c, _| {
        // Go round the block and jump at the roof edge instead.
        let mut i = toward_x(c, 4.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::M1_END + 0.4 && c.state == State::Ground);
        i
    });
    assert!(!s.has(Event::SpringBoard));
    if s.checkpoint() == Some("M2 Balance") {
        assert!(s.has(Event::WallClimbStart) && s.has(Event::PullUp), "jumped straight up there: {:?}", s.events);
    }
}

/// Keep the lean centred: push against it.
fn counter(c: &Controller) -> f32 {
    match c.state {
        State::Balance { lean, .. } => (-lean * 4.0).clamp(-1.0, 1.0),
        _ => 0.0,
    }
}

#[test]
fn balance_beam_with_counter_steering() {
    let mut s = sim(1);
    s.run(14.0, |c, _| {
        if c.feet.z < z::M3_START - 2.0 && c.state == State::Ground {
            return Input::default();
        }
        if matches!(c.state, State::Balance { .. }) {
            return Input { move_axis: Vec2::new(counter(c), 1.0), ..Default::default() };
        }
        let mut i = toward_x(c, 0.0);
        i.move_axis.y = 0.6;
        i
    });
    assert!(s.has(Event::BalanceStart), "{:?}", s.events);
    assert!(!s.has(Event::BalanceFall), "{:?}", s.events);
    assert_eq!(s.deaths, 0);
    assert_eq!(s.checkpoint(), Some("M3 Swing"), "ended at {:?}", s.c.feet);
}

/// TdMove_Balance has no random wobble: the lean comes from how you step on, where you look
/// and your input. Look off to one side and don't counter: the lean runs away and you fall.
#[test]
fn balance_beam_looking_away_without_counter_falls() {
    let mut s = sim(1);
    let mut looked = 0.0f32;
    s.run(14.0, |c, _| {
        if matches!(c.state, State::Balance { .. }) {
            let mut i = Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() };
            if looked < 0.35 {
                i.look.x = 0.02;
                looked += 0.02;
            }
            return i;
        }
        let mut i = toward_x(c, 0.0);
        i.move_axis.y = 0.6;
        i
    });
    assert!(s.has(Event::BalanceStart), "{:?}", s.events);
    assert!(s.has(Event::BalanceFall), "never lost balance");
    assert!(looked > 0.0);
    // The beam is 1.5 m over the roof gap: you fall to the street.
    assert!(s.deaths > 0 || s.c.feet.y < M2_Y - 1.0);
}

#[test]
fn swing_pole_across_the_gap() {
    let mut s = sim(2);
    let (mut j, mut off) = (false, false);
    s.run(12.0, |c, _| {
        if c.feet.z < z::M4_START - 2.0 && c.state == State::Ground {
            return Input::default();
        }
        if let State::Swing { angle, rate, .. } = c.state {
            let mut i = Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() };
            i.jump_pressed = once(&mut off, rate > 0.5 && angle > 0.45);
            return i;
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::M3_END + 0.5 && c.state == State::Ground);
        i
    });
    assert!(s.has(Event::SwingStart), "{:?}", s.events);
    assert!(s.has(Event::SwingJump), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("M4 Zipline"), "ended at {:?}", s.c.feet);
}

#[test]
fn zipline_down_to_the_finish_roof() {
    let mut s = sim(3);
    let mut j = false;
    let mut top_speed = 0.0f32;
    s.run(12.0, |c, _| {
        if let State::ZipLine { speed, .. } = c.state {
            top_speed = top_speed.max(speed);
            return Input::default();
        }
        if c.feet.z < z::M5_START {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::ZIP_START + 1.8 && c.state == State::Ground);
        i
    });
    assert!(s.has(Event::ZipStart), "{:?}", s.events);
    assert!(s.has(Event::ZipEnd { hit_wall: false }), "{:?}", s.events);
    assert!(top_speed > 8.0, "zip too slow: {top_speed}");
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("M5 Finish"), "ended at {:?}", s.c.feet);
    let _ = M4_Y;
}

#[test]
fn melee_picks_the_attack_from_the_move() {
    let mut s = sim(4);
    let kinds = |s: &Sim| -> Vec<MeleeKind> {
        s.events.iter().filter_map(|e| if let Event::Melee { kind, .. } = e { Some(*kind) } else { None }).collect()
    };
    // Standing punch (pressed for one frame: a second press inside TdMove_Melee's 0.33 s window
    // would queue another punch).
    let mut p = false;
    s.run(0.3, |_, _| Input { melee_pressed: once(&mut p, true), ..Default::default() });
    // A punch running too, after building up speed.
    let mut k = false;
    s.run(2.0, |c, _| {
        let mut i = toward_x(c, 0.0);
        i.melee_pressed = once(&mut k, c.horizontal_speed() > 4.0);
        i
    });
    // Jump kick.
    let (mut jj, mut kk) = (false, false);
    // (Not in the jump's first 0.1 s: TdMove_MeleeAir.CanDoMove.) Turned away from the door
    // ahead: at it, it's the air barge.
    s.c.yaw += std::f32::consts::PI;
    let mut air = 0;
    s.run(1.5, |c, _| {
        let mut i = Input::default();
        i.jump_pressed = once(&mut jj, c.state == State::Ground);
        air = if c.state == State::Air { air + 1 } else { 0 };
        i.melee_pressed = once(&mut kk, air > 8);
        i
    });
    // Running, it's a punch too: Faith has no running kick (TdMove_Melee for any ground speed).
    assert_eq!(kinds(&s), vec![MeleeKind::Punch, MeleeKind::Punch, MeleeKind::AirKick], "{:?}", s.events);
    assert!(s.c.melee.is_none(), "attack never ended");
}
