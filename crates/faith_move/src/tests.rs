//! Scripted-input tests: each one builds a tiny level, drives the controller
//! with fake input, and checks the move happened.

use glam::{Vec2, Vec3};

use crate::course_tests::AngleIn;
use crate::*;

const DT: f32 = 1.0 / 60.0;

fn floor() -> BoxWorld {
    let mut w = BoxWorld::default();
    w.add(Aabb::new(Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0)));
    w
}

/// yaw = 0 faces -Z.
fn ctrl_at(p: Vec3) -> Controller {
    let mut c = Controller::new(Tuning::default(), p, 0.0);
    c.state = State::Ground;
    c
}

fn fwd() -> Input {
    Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() }
}

/// Run for `secs`, calling `f` each frame to build input. Returns all events.
fn run(c: &mut Controller, w: &BoxWorld, secs: f32, mut f: impl FnMut(f32, &Controller) -> Input) -> Vec<Event> {
    let mut t = 0.0;
    let mut ev = Vec::new();
    while t < secs {
        let i = f(t, c);
        c.step(DT, &i, w);
        ev.extend(c.events.iter().copied());
        t += DT;
    }
    ev
}

fn has(ev: &[Event], e: Event) -> bool {
    ev.iter().any(|x| std::mem::discriminant(x) == std::mem::discriminant(&e))
}

#[test]
fn falls_and_lands() {
    let w = floor();
    let mut c = Controller::new(Tuning::default(), Vec3::new(0.0, 1.0, 0.0), 0.0);
    let ev = run(&mut c, &w, 1.0, |_, _| Input::default());
    assert!(has(&ev, Event::Land { impact: 0.0, fall: 0.0 }));
    assert_eq!(c.state, State::Ground);
    assert!(c.feet.y.abs() < 1e-3, "feet {}", c.feet.y);
}

#[test]
fn sprint_builds_up() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    // TdPawn.SpeedCurve_LightWeapon: 4 m/s at 0.4 s, 5.2 at 1 s, 6.5 at 3.5 s, 7.2 at 7 s.
    let mut t = 0.0;
    for (at, want) in [(1.0, 5.2), (3.5, 6.5), (7.0, 7.2)] {
        run(&mut c, &w, at - t, |_, _| fwd());
        t = at;
        let v = c.horizontal_speed();
        assert!((v - want).abs() < 0.3, "at {at} s: {v} m/s, want about {want}");
    }
}

#[test]
fn jump_arc_and_land() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    let mut peak: f32 = 0.0;
    let ev = run(&mut c, &w, 1.2, |t, c| {
        peak = peak.max(c.feet.y);
        Input { jump_pressed: t == 0.0, ..Default::default() }
    });
    assert!(has(&ev, Event::Jump));
    // Apex = v² / 2g with ME's jump speed and gravity (about 1.2 m).
    let tu = Tuning::default();
    let expected = tu.jump_speed * tu.jump_speed / (2.0 * tu.gravity);
    assert!((peak - expected).abs() < 0.05, "peak {peak}, expected {expected}");
    assert_eq!(c.state, State::Ground);
}

#[test]
fn vaults_over_waist_high_rail() {
    let mut w = floor();
    // Thin rail 1.0 high, across our path at z = -5.
    w.add(Aabb::new(Vec3::new(-5.0, 0.0, -5.2), Vec3::new(5.0, 1.0, -5.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    let ev = run(&mut c, &w, 2.5, |_, c| Input {
        jump_pressed: c.feet.z < -4.2 && c.feet.z > -4.9,
        ..fwd()
    });
    assert!(has(&ev, Event::Vault), "events {ev:?}");
    assert!(c.feet.z < -6.0, "should be past the rail, z = {}", c.feet.z);
    assert!(c.horizontal_speed() > 4.0, "kept momentum: {}", c.horizontal_speed());
}

#[test]
fn mantles_onto_deep_box() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-5.0, 0.0, -9.0), Vec3::new(5.0, 1.1, -3.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    let ev = run(&mut c, &w, 2.0, |_, c| {
        if c.feet.y > 1.0 {
            return Input::default(); // made it up; stop before running off the far side
        }
        Input { jump_pressed: c.feet.z < -2.3 && c.feet.z > -2.9, ..fwd() }
    });
    assert!(has(&ev, Event::Mantle), "events {ev:?}");
    assert!((c.feet.y - 1.1).abs() < 0.05, "on top: y = {}", c.feet.y);
}

#[test]
fn wallclimb_grab_and_pull_up() {
    let mut w = floor();
    // Wall 3.3 tall ahead: too high to jump onto, reachable by climbing.
    w.add(Aabb::new(Vec3::new(-5.0, 0.0, -10.0), Vec3::new(5.0, 3.3, -4.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    let mut done = false;
    let ev = run(&mut c, &w, 4.0, |_, c| {
        done |= c.feet.y > 3.2 && c.state == State::Ground;
        if done {
            return Input::default();
        }
        Input { jump_pressed: c.feet.z < -3.0 && c.feet.z > -3.5, ..fwd() }
    });
    assert!(has(&ev, Event::WallClimbStart), "events {ev:?}");
    assert!(has(&ev, Event::LedgeGrab), "events {ev:?}");
    assert!(has(&ev, Event::PullUp), "events {ev:?}");
    assert!((c.feet.y - 3.3).abs() < 0.05, "on top: y = {}", c.feet.y);
}

#[test]
fn hang_then_drop() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-5.0, 0.0, -10.0), Vec3::new(5.0, 3.3, -4.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    let mut grabbed = false;
    let ev = run(&mut c, &w, 4.0, |_, c| {
        if matches!(c.state, State::LedgeHang { .. }) {
            grabbed = true;
            return Input { crouch_pressed: true, ..Default::default() };
        }
        // Holding forward on a ledge climbs up, so let go once we're on the wall.
        if grabbed || matches!(c.state, State::WallClimb { .. }) {
            return Input::default();
        }
        Input { jump_pressed: c.feet.z < -3.0 && c.feet.z > -3.5, ..fwd() }
    });
    assert!(has(&ev, Event::LedgeGrab));
    assert!(!has(&ev, Event::PullUp));
    assert!(c.feet.y < 0.01, "dropped back down: {}", c.feet.y);
}

#[test]
fn wallclimb_turn_kick() {
    let mut w = floor();
    // Very tall wall: no ledge to grab.
    w.add(Aabb::new(Vec3::new(-5.0, 0.0, -10.0), Vec3::new(5.0, 20.0, -4.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    let mut turned = false;
    let ev = run(&mut c, &w, 3.0, |_, c| {
        if let State::WallClimb { t, .. } = c.state {
            if t > 0.15 && !turned {
                turned = true;
                return Input { turn_pressed: true, ..Default::default() };
            }
        }
        if matches!(c.state, State::WallClimbTurned { t, .. } if t > 0.2) {
            return Input { jump_pressed: true, ..Default::default() };
        }
        if turned {
            return Input::default();
        }
        Input { jump_pressed: c.feet.z < -3.0 && c.feet.z > -3.5, ..fwd() }
    });
    assert!(has(&ev, Event::WallClimbStart), "{ev:?}");
    assert!(has(&ev, Event::Turn180), "{ev:?}");
    assert!(has(&ev, Event::WallKick), "{ev:?}");
    assert!(c.feet.z > -1.5, "kicked back away from wall: z = {}", c.feet.z);
}

#[test]
fn wallrun_along_wall() {
    let mut w = floor();
    // Long wall on our right (+X side), running along -Z.
    w.add(Aabb::new(Vec3::new(1.0, 0.0, -40.0), Vec3::new(1.5, 5.0, -3.0)));
    let mut c = ctrl_at(Vec3::new(0.6, 0.0, 0.0));
    let mut max_air_z_travel = 0.0f32;
    let mut run_start_z = None;
    let mut pressed = false;
    let mut angle = AngleIn::default();
    let ev = run(&mut c, &w, 4.0, |_, c| {
        if let State::WallRun { .. } = c.state {
            let s = *run_start_z.get_or_insert(c.feet.z);
            max_air_z_travel = max_air_z_travel.max(s - c.feet.z);
        }
        // Run alongside the wall, jump, and angle into it.
        let jump = !pressed && c.feet.z < -9.0;
        pressed |= jump;
        let mut i = Input { jump_pressed: jump, ..fwd() };
        angle.apply(c, &mut i, 25.0);
        i
    });
    assert!(has(&ev, Event::WallRunStart), "{ev:?}");
    assert!(max_air_z_travel > 5.0, "wallran {max_air_z_travel} m");
}

#[test]
fn wallrun_jump_off() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(1.0, 0.0, -40.0), Vec3::new(1.5, 5.0, -3.0)));
    let mut c = ctrl_at(Vec3::new(0.6, 0.0, 0.0));
    let mut jumped = false;
    let mut pressed = false;
    let mut angle = AngleIn::default();
    let ev = run(&mut c, &w, 3.5, |_, c| {
        let mut i = fwd();
        i.jump_pressed = !pressed && c.feet.z < -9.0;
        pressed |= i.jump_pressed;
        angle.apply(c, &mut i, 25.0);
        if let State::WallRun { t, .. } = c.state {
            if t > 0.4 && !jumped {
                jumped = true;
                i.jump_pressed = true;
            }
        }
        if jumped {
            i.move_axis = Vec2::new(0.0, 1.0);
        }
        i
    });
    assert!(has(&ev, Event::WallJump), "{ev:?}");
    assert!(c.feet.x < 0.0, "pushed away from the wall: x = {}", c.feet.x);
}

#[test]
fn slide_under_low_bar() {
    let mut w = floor();
    // Bar from 1.3 up: can't run under it standing (1.8 tall); sliding you're 1.22.
    w.add(Aabb::new(Vec3::new(-5.0, 1.3, -16.0), Vec3::new(5.0, 3.0, -14.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    // The slide brakes out at 2.5 m/s (TdMove_Slide.SlideAbortSpeed) under the bar; crouch on.
    let ev = run(&mut c, &w, 5.0, |_, c| {
        let slide = c.feet.z < -11.5;
        Input { crouch_pressed: slide && c.feet.z > -11.7, crouch_held: slide && c.feet.z > -16.3, ..fwd() }
    });
    assert!(has(&ev, Event::Slide), "{ev:?}");
    assert!(c.feet.z < -16.2, "made it under: z = {}", c.feet.z);
}

#[test]
fn hard_landing_vs_roll() {
    let tower = |w: &mut BoxWorld| {
        // 6 m: past ME's 5.3 m hard-landing height.
        w.add(Aabb::new(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 6.0, 2.0)));
    };
    // No roll: stunned.
    let mut w = floor();
    tower(&mut w);
    let mut c = ctrl_at(Vec3::new(0.0, 6.0, 0.0));
    let ev = run(&mut c, &w, 2.5, |_, _| fwd());
    assert!(has(&ev, Event::HardLand), "{ev:?}");

    // Tap crouch just before touchdown: roll, keep speed.
    let mut c = ctrl_at(Vec3::new(0.0, 6.0, 0.0));
    let mut speed_after = 0.0;
    let ev = run(&mut c, &w, 2.5, |_, c| {
        if matches!(c.state, State::Roll { .. }) {
            speed_after = c.horizontal_speed();
        }
        Input { crouch_pressed: matches!(c.state, State::Air) && c.feet.y < 1.2 && c.vel.y < 0.0, ..fwd() }
    });
    assert!(has(&ev, Event::Roll), "{ev:?}");
    assert!(!has(&ev, Event::HardLand));
    assert!(speed_after > 4.0, "{speed_after}");
}

#[test]
fn lethal_fall_respawns() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 20.0, 2.0)));
    let mut c = ctrl_at(Vec3::new(0.0, 20.0, 0.0));
    c.spawn = Vec3::new(0.0, 20.0, 0.0);
    let ev = run(&mut c, &w, 3.0, |_, _| fwd());
    assert!(has(&ev, Event::Death), "{ev:?}");
}

#[test]
fn steps_up_small_ledges() {
    let mut w = floor();
    for i in 0..5 {
        let z = -3.0 - i as f32 * 0.4;
        // Each step runs from z back to -30, one 0.25 m riser taller than the last.
        w.add(Aabb::new(Vec3::new(-2.0, 0.0, z), Vec3::new(2.0, 0.25 * (i + 1) as f32, -30.0)));
    }
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, 2.0, |_, _| fwd());
    assert!(c.feet.y > 1.0, "climbed stairs: y = {}", c.feet.y);
}


#[test]
fn dodge_boost_to_max_speed() {
    // Run down -Z, look 90° right, dodge left (back along the run): instant top speed.
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, 0.5, |_, _| fwd());
    let before = c.horizontal_speed();
    c.yaw = -std::f32::consts::FRAC_PI_2; // looking right (+X)
    let ev = run(&mut c, &w, DT, |_, _| Input {
        move_axis: Vec2::new(-1.0, 0.0),
        jump_pressed: true,
        ..Default::default()
    });
    assert!(has(&ev, Event::Dodge { dir: Vec3::ZERO }), "{ev:?}");
    let boosted = c.horizontal_speed();
    let tu = Tuning::default();
    assert!(boosted >= tu.sprint_speed - 0.1, "before {before}, after dodge {boosted}");
    assert!(c.vel.z < -6.5, "launched along the original run: {:?}", c.vel);

    // Turn back mid-air, hold forward: the speed carries into a full sprint.
    c.yaw = 0.0;
    run(&mut c, &w, 1.0, |_, _| fwd());
    assert_eq!(c.state, State::Ground);
    assert!(c.horizontal_speed() > tu.sprint_speed - 0.2, "kept speed: {}", c.horizontal_speed());
}

/// W + A (or D) and jump dodges, like the game: you don't have to let go of W.
#[test]
fn dodge_works_while_holding_forward() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    let ev = run(&mut c, &w, DT, |_, _| Input {
        move_axis: Vec2::new(-1.0, 1.0),
        jump_pressed: true,
        ..Default::default()
    });
    assert!(has(&ev, Event::Dodge { dir: Vec3::ZERO }), "{ev:?}");
}

/// TdMove_DodgeJump flies on its own: no steering until it hands over to falling (below
/// ExitToFallingZSpeed), then air control as usual.
#[test]
fn dodge_cannot_be_steered() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, DT, |_, _| Input { move_axis: Vec2::new(1.0, 0.0), jump_pressed: true, ..Default::default() });
    let launch = c.vel;
    assert!(launch.x > 5.0, "dodged right: {launch:?}");
    // Hold the opposite way while still rising.
    let mut steps = 0;
    while c.vel.y > -Tuning::default().dodge_exit_fall_speed + 0.2 && steps < 60 {
        run(&mut c, &w, DT, |_, _| Input { move_axis: Vec2::new(-1.0, -1.0), ..Default::default() });
        steps += 1;
    }
    assert!((c.vel.x - launch.x).abs() < 1e-3 && (c.vel.z - launch.z).abs() < 1e-3, "{launch:?} -> {:?}", c.vel);
}

/// RedoMoveTime: straight off a dodge's landing, jump + strafe is a plain jump.
#[test]
fn dodge_redo_time() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, DT, |_, _| Input { move_axis: Vec2::new(1.0, 0.0), jump_pressed: true, ..Default::default() });
    let mut steps = 0;
    while c.state != State::Ground && steps < 200 {
        run(&mut c, &w, DT, |_, _| Input::default());
        steps += 1;
    }
    let ev = run(&mut c, &w, DT, |_, _| Input { move_axis: Vec2::new(-1.0, 0.0), jump_pressed: true, ..Default::default() });
    assert!(has(&ev, Event::Jump) && !has(&ev, Event::Dodge { dir: Vec3::ZERO }), "{ev:?}");
}

/// A stick pushed most of the way sideways (not bMoveActionMax) jumps instead.
#[test]
fn partial_stick_strafe_jumps() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    let ev = run(&mut c, &w, DT, |_, _| Input { move_axis: Vec2::new(0.9, 0.0), jump_pressed: true, ..Default::default() });
    assert!(has(&ev, Event::Jump) && !has(&ev, Event::Dodge { dir: Vec3::ZERO }), "{ev:?}");
}

/// A little steering on a stick while jumping is still a jump.
#[test]
fn slight_steer_jump_is_not_a_dodge() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    let ev = run(&mut c, &w, DT, |_, _| Input {
        move_axis: Vec2::new(-0.4, 1.0),
        jump_pressed: true,
        ..Default::default()
    });
    assert!(has(&ev, Event::Jump) && !has(&ev, Event::Dodge { dir: Vec3::ZERO }), "{ev:?}");
}

#[test]
fn wallrun_dodge_away_from_wall() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(1.0, 0.0, -40.0), Vec3::new(1.5, 5.0, -3.0)));
    let mut c = ctrl_at(Vec3::new(0.6, 0.0, 0.0));
    let (mut pressed, mut dodged) = (false, false);
    let mut angle = AngleIn::default();
    let ev = run(&mut c, &w, 3.0, |_, c| {
        let mut i = fwd();
        i.jump_pressed = !pressed && c.feet.z < -9.0;
        pressed |= i.jump_pressed;
        angle.apply(c, &mut i, 25.0);
        if dodged {
            return Input::default();
        }
        if let State::WallRun { t, .. } = c.state {
            if t > 0.2 {
                dodged = true;
                // Wall is on the right (+X): strafe left, away from it.
                i = Input { move_axis: Vec2::new(-1.0, 0.0), jump_pressed: true, ..Default::default() };
            }
        }
        i
    });
    assert!(has(&ev, Event::WallRunDodge { dir: Vec3::ZERO }), "{ev:?}");
    assert!(c.feet.x < -1.0, "flew off the wall sideways: {:?}", c.feet);
}

#[test]
fn wallclimb_dodge_sideways() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-10.0, 0.0, -10.0), Vec3::new(10.0, 20.0, -4.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    let mut dodged = false;
    let mut vy_after = 0.0;
    let ev = run(&mut c, &w, 1.5, |_, c| {
        if matches!(c.state, State::WallClimb { t, .. } if t > 0.05) && !dodged {
            dodged = true;
            return Input { move_axis: Vec2::new(1.0, 0.0), jump_pressed: true, ..Default::default() };
        }
        if dodged {
            if vy_after == 0.0 { vy_after = c.vel.y; }
            return Input::default();
        }
        Input { jump_pressed: c.feet.z < -3.0 && c.feet.z > -3.5, ..fwd() }
    });
    assert!(has(&ev, Event::WallClimbDodge { dir: Vec3::ZERO }), "{ev:?}");
    assert!(vy_after > 5.0, "big upward kick: {vy_after}");
    assert!(c.feet.x > 0.5, "moved right along the wall: {:?}", c.feet);
}

#[test]
fn camera_bob_and_shake() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    let mut fx = CameraFx::default();
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    let mut t = 0.0;
    while t < 3.0 {
        c.step(DT, &fwd(), &w);
        let shot = fx.update(DT, &c, &fwd());
        if t > 2.0 {
            let rel = shot.view.eye.y - c.view().eye.y;
            lo = lo.min(rel);
            hi = hi.max(rel);
        }
        t += DT;
    }
    assert!(hi - lo > 0.04, "head bobs while sprinting: {}", hi - lo);

    // A hard landing kicks the camera down and shakes it.
    let mut w2 = floor();
    w2.add(Aabb::new(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 6.0, 2.0)));
    let mut c = ctrl_at(Vec3::new(0.0, 6.0, 0.0));
    let mut fx = CameraFx::default();
    let mut max_shake: f32 = 0.0;
    let mut min_pitch: f32 = 0.0;
    t = 0.0;
    while t < 2.5 {
        c.step(DT, &fwd(), &w2);
        let shot = fx.update(DT, &c, &fwd());
        max_shake = max_shake.max(shot.shake);
        min_pitch = min_pitch.min(shot.view.pitch);
        t += DT;
    }
    assert!(max_shake > 0.5, "shake {max_shake}");
    assert!(min_pitch < -0.05, "pitch kick {min_pitch}");
}

#[test]
fn dodge_camera_stays_finite() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    let mut fx = CameraFx::default();
    let mut t = 0.0;
    let mut dodged = false;
    while t < 2.0 {
        let mut i = fwd();
        if c.feet.z < -3.0 && !dodged {
            dodged = true;
            c.yaw = -std::f32::consts::FRAC_PI_2;
            i = Input { move_axis: Vec2::new(-1.0, 0.0), jump_pressed: true, ..Default::default() };
        } else if dodged {
            i = Input::default();
        }
        c.step(DT, &i, &w);
        let shot = fx.update(DT, &c, &i);
        let v = shot.view;
        assert!(
            v.eye.is_finite() && v.yaw.is_finite() && v.pitch.is_finite() && v.roll.is_finite() && v.fov_deg.is_finite(),
            "non-finite view at t={t}: {v:?} state {:?} vel {:?} events {:?}",
            c.state, c.vel, c.events
        );
        t += DT;
    }
    assert!(dodged);
}

#[test]
fn camera_survives_long_frames() {
    // 4 fps with a dodge and a hard landing: nothing may go non-finite.
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 6.0, 2.0)));
    let mut c = ctrl_at(Vec3::new(0.0, 6.0, 0.0));
    let mut fx = CameraFx::default();
    for k in 0..40 {
        let i = if k == 20 {
            Input { move_axis: Vec2::new(1.0, 0.0), jump_pressed: true, ..Default::default() }
        } else {
            fwd()
        };
        c.step(0.25, &i, &w);
        let v = fx.update(0.25, &c, &i).view;
        assert!(v.eye.is_finite() && v.pitch.is_finite() && v.roll.is_finite() && v.yaw.is_finite(), "{v:?}");
    }
}

// ---------------------------------------------------------------- rules from TdGame.u's scripts

/// TdMove_Landing.SubtractLandingSpeed: landing a plain jump caps your speed at what you jumped
/// with minus 65 uu/s, so the jump's +100 uu/s forward boost doesn't stick.
#[test]
fn jump_landing_hands_back_the_boost() {
    let w = floor();
    // Full sprint takes 7 s (SpeedCurve_LightWeapon): start far back so there's floor for it.
    let mut c = ctrl_at(Vec3::new(0.0, 0.0, 45.0));
    run(&mut c, &w, 7.5, |_, _| fwd());
    let tu = Tuning::default();
    let before = c.horizontal_speed();
    assert!(before > tu.sprint_speed - 0.05, "sprinting: {before}");
    let mut in_air_max = 0.0f32;
    let mut landed = None;
    run(&mut c, &w, 1.5, |t, c| {
        if c.state == State::Air {
            in_air_max = in_air_max.max(c.horizontal_speed());
        } else if in_air_max > 0.0 && landed.is_none() {
            landed = Some(c.horizontal_speed());
        }
        Input { jump_pressed: t == 0.0, ..fwd() }
    });
    assert!(in_air_max > before + 0.9, "JumpAddXY in the air: {in_air_max}");
    let after = landed.expect("landed");
    let cap = before - tu.landing_speed_reduction;
    assert!((after - cap).abs() < 0.1, "landed at {after}, expected about {cap}");
}

/// The cap is only for plain jumps: a dodge keeps its speed (TdMove_DodgeJump isn't in
/// SubtractLandingSpeed's list).
#[test]
fn dodge_landing_keeps_its_speed() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, 0.5, |_, _| fwd());
    c.yaw = -std::f32::consts::FRAC_PI_2;
    run(&mut c, &w, DT, |_, _| Input { move_axis: Vec2::new(-1.0, 0.0), jump_pressed: true, ..Default::default() });
    let launched = c.horizontal_speed();
    let mut landed = None;
    // Turn to face the way you're flying and keep running into the landing (letting go would
    // brake: CalcVelocity's braking is strong).
    let v = c.vel;
    c.yaw = (-v.x).atan2(-v.z);
    run(&mut c, &w, 1.0, |_, c| {
        if c.state == State::Ground && landed.is_none() {
            landed = Some(c.horizontal_speed());
        }
        fwd()
    });
    let landed = landed.expect("landed");
    let cap = Tuning::default().sprint_speed;
    assert!(landed > launched.min(cap) - 0.3, "launched {launched}, landed {landed}");
}

fn tower() -> BoxWorld {
    let mut w = floor();
    // 6 m: past ME's 5.3 m hard-landing height.
    w.add(Aabb::new(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 6.0, 2.0)));
    w
}

/// Drop off the tower, pressing crouch as the feet pass each of `heights`. Returns
/// (rolled, hard landing).
fn drop_pressing_crouch_at(heights: &[f32]) -> (bool, bool) {
    let w = tower();
    let mut c = ctrl_at(Vec3::new(0.0, 6.0, 0.0));
    let mut done = vec![false; heights.len()];
    let ev = run(&mut c, &w, 2.5, |_, c| {
        let mut i = fwd();
        if matches!(c.state, State::Air) && c.vel.y < 0.0 {
            for (k, &h) in heights.iter().enumerate() {
                if !done[k] && c.feet.y < h {
                    done[k] = true;
                    i.crouch_pressed = true;
                }
            }
        }
        i
    });
    (has(&ev, Event::Roll), has(&ev, Event::HardLand))
}

/// TdPawn.CanSkillRoll: crouch has to be pressed in the last 0.2 s before touching down, and a
/// press only counts 0.6 s after the last one did, so an early press (or mashing) doesn't roll.
#[test]
fn roll_needs_a_late_crouch_press() {
    // About 0.1 s out: rolls.
    assert_eq!(drop_pressing_crouch_at(&[1.2]), (true, false));
    // About 0.35 s out: too early.
    assert_eq!(drop_pressing_crouch_at(&[4.0]), (false, true));
    // Early, then again in time but within 0.6 s of the first: the second press doesn't count.
    assert_eq!(drop_pressing_crouch_at(&[4.0, 1.2]), (false, true));
}

/// TdMove_Landing.LandHard: a dead stop.
#[test]
fn hard_landing_stops_dead() {
    let w = tower();
    let mut c = ctrl_at(Vec3::new(0.0, 6.0, 0.0));
    let mut stunned_speed = None;
    let ev = run(&mut c, &w, 2.5, |_, c| {
        if matches!(c.state, State::Stunned { .. }) {
            stunned_speed = Some(stunned_speed.unwrap_or(0.0f32).max(c.horizontal_speed()));
        }
        fwd()
    });
    assert!(has(&ev, Event::HardLand), "{ev:?}");
    assert_eq!(stunned_speed, Some(0.0));
}

/// TdMove_Slide.CanDoMove wants 350 uu/s along the view, not just speed in any direction.
#[test]
fn slide_needs_forward_speed() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, 2.0, |_, _| fwd());
    assert!(c.horizontal_speed() > 5.0);
    // Look 90° to the side and crouch: all that speed is sideways.
    let ev = run(&mut c, &w, DT, |_, _| Input {
        look: Vec2::new(std::f32::consts::FRAC_PI_2, 0.0),
        crouch_pressed: true,
        crouch_held: true,
        ..Default::default()
    });
    assert!(!has(&ev, Event::Slide), "{ev:?}");
}

/// TdMove_Slide: at least 0.5 s even if you let go at once, and StopMove halves your velocity.
#[test]
fn slide_lasts_half_a_second_and_halves_speed_on_exit() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, 2.0, |_, _| fwd());
    let mut slide_time = 0.0;
    let mut last_slide_speed = 0.0;
    let mut exit_speed = None;
    let ev = run(&mut c, &w, 1.5, |t, c| {
        if let State::Slide { t } = c.state {
            slide_time = t;
            last_slide_speed = c.horizontal_speed();
        } else if slide_time > 0.0 && exit_speed.is_none() {
            exit_speed = Some(c.horizontal_speed());
        }
        Input { crouch_pressed: t == 0.0, crouch_held: t == 0.0, ..Default::default() }
    });
    assert!(has(&ev, Event::Slide), "{ev:?}");
    assert!(slide_time >= 0.45, "slid for {slide_time} s");
    let exit = exit_speed.expect("slide ended");
    assert!((exit - last_slide_speed * 0.5).abs() < 0.3, "slide {last_slide_speed} → {exit}");
}

/// Wallrun along the wall on our right (+X) and jump off 0.3 s in, optionally turning to look
/// straight out from the wall on the same frame. Returns the velocity at take-off.
fn wallrun_take_off(look_out: bool) -> Vec3 {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(1.0, 0.0, -40.0), Vec3::new(1.5, 5.0, -3.0)));
    let mut c = ctrl_at(Vec3::new(0.6, 0.0, 0.0));
    let (mut pressed, mut out) = (false, None);
    let mut angle = AngleIn::default();
    run(&mut c, &w, 3.0, |_, c| {
        if out.is_none() && c.events.iter().any(|e| matches!(e, Event::WallJump)) {
            out = Some(c.vel);
        }
        if out.is_some() {
            return Input::default();
        }
        let mut i = fwd();
        i.jump_pressed = !pressed && c.feet.z < -9.0;
        pressed |= i.jump_pressed;
        angle.apply(c, &mut i, 25.0);
        if let State::WallRun { t, .. } = c.state {
            if t > 0.3 {
                i.jump_pressed = true;
                // Facing -Z along the wall, or turned left 90° to -X, straight out from it.
                let want = if look_out { std::f32::consts::FRAC_PI_2 } else { 0.0 };
                i.look = Vec2::new(want - c.yaw, 0.0);
            }
        }
        i
    });
    out.expect("jumped off the wall")
}

/// TdMove_WallrunJump: jumping off looking along the wall keeps your speed along it; looking
/// straight out trades it (down to WallRunningPushForwardSpeedMin) for a much harder, higher push.
#[test]
fn wallrun_jump_off_trades_forward_speed_for_push() {
    let tu = Tuning::default();
    let along = wallrun_take_off(false);
    let out = wallrun_take_off(true);
    assert!(-along.z > 4.0, "kept along-wall speed: {along:?}");
    assert!((along.x + tu.wallrun_jump_out).abs() < 0.2, "base push only: {along:?}");
    assert!(-out.z < -along.z * 0.15, "looking out keeps about 10%: {out:?} vs {along:?}");
    assert!((out.x + tu.wallrun_jump_out + tu.wallrun_jump_out_look_add).abs() < 0.2, "full push: {out:?}");
    assert!(out.y > along.y, "and higher: {out:?} vs {along:?}");
}

/// TdMove_WallClimb.ReachedWall: the climb boost grows with your run-up.
#[test]
fn wallclimb_boost_grows_with_run_up() {
    let peak = |start_z: f32| {
        let mut w = floor();
        w.add(Aabb::new(Vec3::new(-5.0, 0.0, -10.0), Vec3::new(5.0, 20.0, -4.0)));
        let mut c = ctrl_at(Vec3::new(0.0, 0.0, start_z));
        // Rise from where the climb started (a faster run-up meets the wall lower down).
        let (mut start, mut top) = (None, 0.0f32);
        let ev = run(&mut c, &w, 3.0 + start_z / 7.0, |_, c| {
            if let State::WallClimb { .. } = c.state {
                let s = *start.get_or_insert(c.feet.y);
                top = top.max(c.feet.y - s);
            }
            Input { jump_pressed: c.feet.z < -3.0 && c.feet.z > -3.5, ..fwd() }
        });
        assert!(has(&ev, Event::WallClimbStart), "{ev:?}");
        top
    };
    // The boost is sqrt(4 x height x WallClimbingGravity) under WallClimbingGravity, so you rise
    // twice `height`. Two steps' run-up (no sprint built): only the upward-speed part,
    // AddOnSpeedZHeight.
    let short = peak(-1.5);
    // Full sprint: plus all of AddOnSpeed2DHeight.
    let sprint = peak(30.0);
    let tu = Tuning::default();
    let z_only = 2.0 * tu.wallclimb_add_z_height;
    assert!((short - z_only).abs() < 0.25, "short run-up climbs {short} m, expected about {z_only}");
    let full = 2.0 * (tu.wallclimb_add_z_height + tu.wallclimb_add_xy_height);
    assert!((sprint - full).abs() < 0.25, "sprinting climbs {sprint} m, expected about {full}");
}

/// TdMove_Coil.CanDoMove: not off a plain drop, only out of a jump, moving forward.
#[test]
fn coil_needs_a_jump() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-5.0, 0.0, -2.0), Vec3::new(5.0, 3.0, 5.0)));
    // Run off the edge holding crouch: no coil.
    let mut c = ctrl_at(Vec3::new(0.0, 3.0, 0.0));
    let mut coiled = false;
    run(&mut c, &w, 1.2, |_, c| {
        coiled |= c.is_coiled();
        Input { crouch_held: c.feet.z < -2.1, ..fwd() }
    });
    assert!(!coiled, "coiled off a plain drop");
    // Jump, then hold crouch: coils.
    let mut c = ctrl_at(Vec3::new(0.0, 3.0, 4.0));
    let mut coiled = false;
    run(&mut c, &w, 1.0, |t, c| {
        coiled |= c.is_coiled();
        Input { jump_pressed: t > 0.4 && t < 0.42, crouch_pressed: t > 0.5 && t < 0.52, crouch_held: t > 0.5, ..fwd() }
    });
    assert!(coiled, "didn't coil after a jump");
}

/// Jump mid-slide does nothing.
#[test]
fn jump_does_nothing_mid_slide() {
    // PlayerWalking's slide case only handles stop-crouch and melee: no jumping out of a slide.
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, 2.0, |_, _| fwd());
    let ev = run(&mut c, &w, 0.6, |t, _| {
        Input { crouch_pressed: t == 0.0, crouch_held: true, jump_pressed: (0.2..0.22).contains(&t), ..fwd() }
    });
    assert!(has(&ev, Event::Slide) && !has(&ev, Event::Jump), "{ev:?}");
}

/// Q only turns you where Mirror's Edge has a turn move: not mid-slide or mid-wallrun, where
/// the view would swing round through a body that's locked to the slide.
#[test]
fn no_180_turn_mid_slide() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, 2.0, |_, _| fwd());
    run(&mut c, &w, 0.2, |t, _| Input { crouch_pressed: t == 0.0, crouch_held: true, ..fwd() });
    assert!(matches!(c.state, State::Slide { .. }));
    let ev = run(&mut c, &w, 0.3, |t, _| Input { turn_pressed: t == 0.0, crouch_held: true, ..fwd() });
    assert!(!has(&ev, Event::Turn180), "{ev:?}");
    assert!(c.yaw.abs() < 0.01, "yaw {}", c.yaw);
}

/// Wallrun detection (native FindWallForward / FindWallSide): running dead parallel past a
/// wall and jumping finds nothing, since the forward trace never reaches it, but jumping
/// while strafing toward it (MoveActionHint, within 30 units of the take-off) does.
fn parallel_jump_by_wall(strafe: f32) -> Vec<Event> {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(1.0, 0.0, -40.0), Vec3::new(1.5, 5.0, -3.0)));
    let mut c = ctrl_at(Vec3::new(0.6, 0.0, 0.0));
    let mut pressed = false;
    run(&mut c, &w, 2.0, |_, c| {
        let jump = !pressed && c.feet.z < -9.0;
        pressed |= jump;
        // Holding W+D through the take-off, from just before it.
        let x = if c.feet.z < -8.6 { strafe } else { 0.0 };
        Input { jump_pressed: jump, move_axis: Vec2::new(x, 1.0), ..Default::default() }
    })
}

#[test]
fn parallel_jump_past_a_wall_is_no_wallrun() {
    let ev = parallel_jump_by_wall(0.0);
    assert!(has(&ev, Event::Jump) && !has(&ev, Event::WallRunStart), "{ev:?}");
}

#[test]
fn strafing_into_the_wall_on_the_jump_wallruns() {
    // Past MoveActionHint's 0.3, short of a dodge jump.
    let ev = parallel_jump_by_wall(0.4);
    assert!(has(&ev, Event::WallRunStart), "{ev:?}");
}

/// Hanging from a ledge 3 m up on a wall whose face is at z = -3, x from -2 to `wall_end`.
fn hanging(wall_end: f32) -> (Controller, BoxWorld) {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-2.0, 0.0, -5.0), Vec3::new(wall_end, 3.0, -3.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    let tu = Tuning::default();
    c.feet = Vec3::new(0.0, 3.0 - tu.hang_hands_above_feet, -3.0 + tu.half_width + 0.001);
    c.state = State::LedgeHang { normal: Vec3::Z, ledge_y: 3.0, turned: false };
    (c, w)
}

/// TdMove_Grab: no shimmy for the first 0.6 s, then 60-unit hand-over-hand steps, one per
/// 1.07 s cycle, for as long as D is held, stopping where the ledge does.
#[test]
fn shimmy_steps_along_the_ledge() {
    let (mut c, w) = hanging(2.0);
    let right = Input { move_axis: Vec2::new(1.0, 0.0), ..Default::default() };
    run(&mut c, &w, 0.55, |_, _| right);
    assert!(c.feet.x.abs() < 1e-4, "too soon: {:?}", c.feet);
    // Started at 0.6 s; after 1.07 s more, exactly one step.
    run(&mut c, &w, 0.6 + 1.07 - 0.55 + 0.02, |_, _| right);
    assert!((c.feet.x - 0.6).abs() < 0.03, "one step: {:?}", c.feet);
    // Let go mid-step: the step still finishes.
    run(&mut c, &w, 0.3, |_, _| right);
    run(&mut c, &w, 1.2, |_, _| Input::default());
    assert!((c.feet.x - 1.2).abs() < 0.03, "two steps: {:?}", c.feet);
    assert!(matches!(c.state, State::LedgeHang { .. }), "{:?}", c.state);
    // A wall across the ledge at x = 2 stops you there (body edge against it).
    let mut w = w;
    w.add(Aabb::new(Vec3::new(2.0, 0.0, -5.0), Vec3::new(3.0, 6.0, 0.0)));
    run(&mut c, &w, 3.0, |_, _| right);
    assert!(c.feet.x <= 1.7 + 1e-3 && c.feet.x > 1.5, "{:?}", c.feet);
    assert!(matches!(c.state, State::LedgeHang { normal, .. } if normal == Vec3::Z), "{:?}", c.state);
}

/// CanShimmyAroundCorner: where the ledge turns an outside corner you go round it, ending
/// up facing the side face.
#[test]
fn shimmy_round_an_outside_corner() {
    let (mut c, w) = hanging(0.6);
    let right = Input { move_axis: Vec2::new(1.0, 0.0), ..Default::default() };
    let ev = run(&mut c, &w, 4.0, |_, _| right);
    assert!(has(&ev, Event::ShimmyCorner), "{ev:?}");
    let State::LedgeHang { normal, .. } = c.state else { panic!("{:?}", c.state) };
    assert!(normal.abs_diff_eq(Vec3::X, 1e-4), "{normal:?}");
    assert!(c.feet.x > 0.6, "round the corner: {:?}", c.feet);
    assert!((c.yaw - std::f32::consts::FRAC_PI_2).abs() < 0.05, "facing the side face: {}", c.yaw);
}

/// TdMove_WallRun's turn: Q on a wallrun eases the view round to look straight out from the
/// wall (SetLookAtTargetAngle, 0.15 s rate) and keeps you running; the jump is then the full
/// push-off across to the opposite wall (bTurned90FromWall), however far the view has come.
#[test]
fn wallrun_turn_then_jump_across() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(1.0, 0.0, -40.0), Vec3::new(1.5, 5.0, -3.0)));
    let mut c = ctrl_at(Vec3::new(0.6, 0.0, 0.0));
    let (mut pressed, mut turned, mut jumped) = (false, false, false);
    let mut angle = AngleIn::default();
    let mut facing_out = None;
    let ev = run(&mut c, &w, 3.0, |_, c| {
        let mut i = fwd();
        i.jump_pressed = !pressed && c.feet.z < -9.0;
        pressed |= i.jump_pressed;
        angle.apply(c, &mut i, 25.0);
        if let State::WallRun { t, normal } = c.state {
            if t > 0.2 && !turned {
                turned = true;
                i.turn_pressed = true;
            } else if turned && t > 0.45 && !jumped {
                facing_out = Some(forward_of(c.yaw).dot(normal));
                jumped = true;
                i.jump_pressed = true;
            }
        }
        i
    });
    assert!(has(&ev, Event::WallRunTurn) && !has(&ev, Event::Turn180), "{ev:?}");
    // 0.25 s in, the look-at has covered all but ~17% of the 90 degrees (ME's ease-out).
    assert!(facing_out.is_some_and(|d| (0.9..0.99).contains(&d)), "mostly looking out: {facing_out:?}");
    assert!(has(&ev, Event::WallJump) && !has(&ev, Event::WallKick), "{ev:?}");
    assert!(c.feet.x < -2.0, "pushed well out from the wall: {:?}", c.feet);
}

/// A jump straight after Q still pushes off at full strength, square to the wall.
#[test]
fn wallrun_turn_jump_is_full_push_at_once() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(1.0, 0.0, -40.0), Vec3::new(1.5, 5.0, -3.0)));
    let mut c = ctrl_at(Vec3::new(0.6, 0.0, 0.0));
    let (mut pressed, mut turned, mut jumped) = (false, false, false);
    let mut angle = AngleIn::default();
    let mut out_speed = None;
    run(&mut c, &w, 3.0, |_, c| {
        if jumped && out_speed.is_none() {
            out_speed = Some(-c.vel.x);
        }
        let mut i = fwd();
        i.jump_pressed = !pressed && c.feet.z < -9.0;
        pressed |= i.jump_pressed;
        angle.apply(c, &mut i, 25.0);
        if let State::WallRun { t, .. } = c.state {
            if t > 0.2 && !turned {
                turned = true;
                i.turn_pressed = true;
            } else if turned && !jumped {
                jumped = true;
                i.jump_pressed = true;
            }
        }
        i
    });
    let tu = Tuning::default();
    let full = tu.wallrun_jump_out + tu.wallrun_jump_out_look_add;
    assert!(out_speed.is_some_and(|v| (v - full).abs() < 0.3), "out {out_speed:?}, full {full}");
}

/// Every 180 turns to the right (StandTurn180Right, RunTurn180: their root yaw goes right).
#[test]
fn turn_180_goes_right() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    let start = c.yaw;
    let mut first = None;
    run(&mut c, &w, 1.0, |t, c| {
        if t > 0.02 && first.is_none() {
            first = Some(c.yaw - start);
        }
        Input { turn_pressed: t == 0.0, ..Default::default() }
    });
    assert!(first.is_some_and(|d| d < 0.0), "starts turning right (yaw falls): {first:?}");
    let d = (c.yaw - start + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    assert!(d.abs() > 3.0, "turned round: {d}");
}

fn forward_of(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

/// TdMove_Landing.LandHard ignores move and look input until FallingLandHard ends, so holding
/// W (or moving the mouse) through a hard landing does nothing until you're back up.
#[test]
fn hard_landing_holds_until_the_animation_ends() {
    let w = tower();
    let mut c = ctrl_at(Vec3::new(0.0, 6.0, 0.0));
    let mut landed_at: Option<(f32, Vec3, f32)> = None;
    let mut moved_while_down = 0.0f32;
    let mut free_at = None;
    run(&mut c, &w, 4.0, |t, c| {
        if let State::Stunned { .. } = c.state {
            let (_, at, yaw) = *landed_at.get_or_insert((t, c.feet, c.yaw));
            moved_while_down = moved_while_down.max(horiz_dist(c.feet, at)).max((c.yaw - yaw).abs());
        } else if landed_at.is_some() && free_at.is_none() {
            free_at = Some(t);
        }
        Input { look: Vec2::new(0.05, 0.0), ..fwd() }
    });
    let (t0, _, _) = landed_at.expect("hard landing");
    let held = free_at.expect("got up") - t0;
    assert!((held - Tuning::default().hard_land_stun).abs() < 0.05, "held {held} s");
    assert!(moved_while_down < 1e-4, "moved {moved_while_down}");
}

fn horiz_dist(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// Run (at `speed` m/s) at an obstacle `height` m high and `depth` m deep, 6 m ahead, jump
/// 1 m before it holding W, and report the vault animation the game would pick and where
/// the feet were as it ended.
fn vault_at(height: f32, depth: f32, speed: f32) -> (Option<&'static str>, Vec3) {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-3.0, 0.0, -6.0 - depth), Vec3::new(3.0, height, -6.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    c.vel = Vec3::new(0.0, 0.0, -speed);
    c.run.energy = (speed - c.tuning.loco.max_base).max(0.0);
    let mut seen = None;
    let mut after = None;
    let mut jumped = false;
    run(&mut c, &w, 6.0, |_, c| {
        if let State::Vault(v) = c.state {
            seen.get_or_insert(v.anim());
        } else if seen.is_some() && after.is_none() {
            after = Some(c.feet);
        }
        // Hold the speed until the jump (walking pace: half a stick).
        let push = if speed < 3.0 { 0.35 } else { 1.0 };
        let jump = !jumped && c.feet.z < -5.0 + 0.05;
        jumped |= jump;
        Input { move_axis: Vec2::new(0.0, if jumped { 1.0 } else { push }), jump_pressed: jump, ..Default::default() }
    });
    (seen, after.unwrap_or(c.feet))
}

/// TdMove_SpeedVault.VaultTypes: a waist-high rail at a run is VaultOver, and you come out
/// the far side still running.
#[test]
fn vault_over_a_rail_at_a_run() {
    let (anim, feet) = vault_at(0.9, 0.4, 6.0);
    assert_eq!(anim, Some("VaultOver"));
    assert!(feet.z < -6.4 && feet.y.abs() < 0.05, "{feet:?}");
}

/// A deep block of the same height is VaultOnto: you end up on top.
#[test]
fn vault_onto_a_deep_block() {
    let (anim, feet) = vault_at(0.9, 4.0, 6.0);
    assert_eq!(anim, Some("VaultOnto"));
    assert!((feet.y - 0.9).abs() < 0.05 && feet.z < -6.0, "{feet:?}");
}

/// Jumping at it from a standstill (under 200 uu/s even with the jump's push) you step up onto
/// it: stepuprightleg88.
#[test]
fn standing_jump_onto_a_rail_is_a_step_up() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-3.0, 0.0, -6.4), Vec3::new(3.0, 0.9, -6.0)));
    let mut c = ctrl_at(Vec3::new(0.0, 0.0, -5.2));
    let mut seen = None;
    let mut first = true;
    run(&mut c, &w, 1.5, |_, c| {
        if let State::Vault(v) = c.state {
            seen.get_or_insert(v.anim());
        }
        let i = Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: first, ..Default::default() };
        first = false;
        i
    });
    assert_eq!(seen, Some("stepuprightleg88"));
}

/// Chest height (1.45-1.92 m) needs you still rising (MinSpeedZ 50): VaultOntoHigh.
#[test]
fn vault_onto_high_while_rising() {
    let (anim, feet) = vault_at(1.6, 4.0, 6.0);
    assert_eq!(anim, Some("VaultOntoHigh"));
    assert!((feet.y - 1.6).abs() < 0.05 && feet.z < -6.0, "{feet:?}");
}

/// TdMove_180TurnInAir then TdMove_Landing.LandBackwards: jump at a run, turn round in the air,
/// and you land on your back (TdMove_LayOnGround) and stay there until you jump to get up.
#[test]
fn air_turn_lands_you_on_your_back() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, 2.0, |_, _| fwd());
    let (mut jumped, mut turned) = (false, false);
    let mut ev = run(&mut c, &w, 0.9, |_, c| {
        let mut i = fwd();
        i.jump_pressed = !jumped;
        jumped = true;
        if c.state == State::Air && c.vel.y < 1.0 && !turned {
            turned = true;
            i.turn_pressed = true;
        }
        i
    });
    assert!(has(&ev, Event::Turn180) && has(&ev, Event::LandOnBack), "{ev:?}");
    assert!(!has(&ev, Event::Roll) && !has(&ev, Event::HardLand), "{ev:?}");
    // Holding nothing, you stay down.
    ev = run(&mut c, &w, 2.0, |_, _| Input::default());
    assert!(matches!(c.state, State::LayOnGround { getting_up: None, .. }), "{:?}", c.state);
    assert!(c.horizontal_speed() < 1e-3);
    let _ = ev;
    // Jump gets you up after JumpTurnLandingStand.
    let mut first = true;
    run(&mut c, &w, 1.0, |_, _| {
        let i = Input { jump_pressed: first, ..Default::default() };
        first = false;
        i
    });
    assert!(matches!(c.state, State::LayOnGround { getting_up: Some(_), .. }), "{:?}", c.state);
    run(&mut c, &w, 0.5, |_, _| Input::default());
    assert_eq!(c.state, State::Ground);
}

/// UTdMove_Slide's native tick: the slide follows the body, which turns toward where you look
/// (and with A/D), braking with GroundFriction x 0.1 until it's under 2.5 m/s.
#[test]
fn slide_steers_toward_the_view_and_brakes_out() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    run(&mut c, &w, 3.0, |_, _| fwd());
    let start_speed = c.horizontal_speed();
    let mut pressed = false;
    let mut looked = false;
    let mut slide_time = 0.0;
    let ev = run(&mut c, &w, 3.0, |_, c| {
        let mut i = Input { crouch_held: true, ..Default::default() };
        i.crouch_pressed = !pressed;
        pressed = true;
        if matches!(c.state, State::Slide { .. }) {
            slide_time += DT;
            if !looked {
                looked = true;
                i.look.x = 0.6; // look 34 degrees left
            }
        }
        i
    });
    assert!(has(&ev, Event::Slide), "{ev:?}");
    // Exponential braking at 2 x 0.8 x 0.5 = 0.8/s down to 2.5 m/s.
    let want = (start_speed / 2.5).ln() / 0.8;
    assert!((slide_time - want).abs() < 0.1, "slid {slide_time:.2} s, want {want:.2} (from {start_speed:.2} m/s)");
    // Curved left (toward -X) after looking left.
    assert!(c.feet.x < -0.2, "{:?}", c.feet);
}

/// A wall across the floor at z = -6 with a door in it (x -0.7..0.7), coming at it from +Z.
fn door_world() -> BoxWorld {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-8.0, 0.0, -6.3), Vec3::new(-0.7, 3.0, -6.0)));
    w.add(Aabb::new(Vec3::new(0.7, 0.0, -6.3), Vec3::new(8.0, 3.0, -6.0)));
    w.add(Aabb::new(Vec3::new(-0.7, 2.2, -6.3), Vec3::new(0.7, 3.0, -6.0)));
    w.fixtures.push(Fixture::Door { b: Aabb::new(Vec3::new(-0.7, 0.0, -6.25), Vec3::new(0.7, 2.2, -6.05)), n: Vec3::Z });
    w
}

/// TdMove_Barge: melee running at a door shoulders it open and carries you through.
#[test]
fn barge_through_a_door_at_a_run() {
    let w = door_world();
    let mut c = ctrl_at(Vec3::new(0.0, 0.0, 6.0));
    let mut pressed = false;
    let ev = run(&mut c, &w, 4.0, |_, c| {
        let mut i = fwd();
        i.melee_pressed = !pressed && c.feet.z < -4.5;
        pressed |= i.melee_pressed;
        i
    });
    assert!(has(&ev, Event::Barge { hands: true }) && has(&ev, Event::DoorOpened { door: 0 }), "{ev:?}");
    assert!(c.feet.z < -8.0, "through the door: {:?}", c.feet);
}

/// Standing at it, it's a kick (MeleeKickObject), and then the way is open.
#[test]
fn kick_a_door_open_standing() {
    let w = door_world();
    let mut c = ctrl_at(Vec3::new(0.0, 0.0, -5.4));
    let ev = run(&mut c, &w, 0.8, |t, _| Input { melee_pressed: t == 0.0, ..Default::default() });
    assert!(has(&ev, Event::Barge { hands: false }) && has(&ev, Event::DoorOpened { door: 0 }), "{ev:?}");
    run(&mut c, &w, 2.0, |_, _| fwd());
    assert!(c.feet.z < -7.0, "walked through: {:?}", c.feet);
}

/// Without opening it, the door is a wall.
#[test]
fn a_closed_door_blocks() {
    let w = door_world();
    let mut c = ctrl_at(Vec3::new(0.0, 0.0, -3.0));
    run(&mut c, &w, 2.0, |_, _| fwd());
    assert!(c.feet.z > -6.1, "{:?}", c.feet);
}

/// TdBarbedWireVolume: run into barbed wire and you trip over it (StumbleFwd), carried about
/// 2.6 m on with no control, then you're back on your feet.
#[test]
fn barbed_wire_trips_you() {
    let mut w = floor();
    w.fixtures.push(Fixture::BarbedWire { b: Aabb::new(Vec3::new(-3.0, -0.1, -6.4), Vec3::new(3.0, 0.6, -6.0)) });
    let mut c = ctrl_at(Vec3::ZERO);
    let mut at = None;
    let ev = run(&mut c, &w, 3.0, |_, c| {
        if matches!(c.state, State::Stumble { .. }) {
            at.get_or_insert(c.feet);
        }
        fwd()
    });
    assert!(has(&ev, Event::Stumble { forward: true }), "{ev:?}");
    let at = at.unwrap();
    assert!(c.state == State::Ground, "{:?}", c.state);
    assert!(c.feet.z < at.z - 2.5, "carried over: {:?} from {:?}", c.feet, at);
}

/// TdMove_Landing.LandOnSoftObject: a hard-landing drop onto a mattress is a soft landing.
#[test]
fn soft_landing_on_a_mattress() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 0.5, 2.0)));
    w.fixtures.push(Fixture::SoftPad { b: Aabb::new(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 0.5, 2.0)) });
    let mut c = Controller::new(Tuning::default(), Vec3::new(0.0, 7.0, 0.0), 0.0);
    let mut braced = false;
    let ev = run(&mut c, &w, 2.5, |_, c| {
        braced |= c.soft_brace;
        Input::default()
    });
    assert!(has(&ev, Event::SoftLand) && !has(&ev, Event::HardLand), "{ev:?}");
    assert!(braced, "braced for it in the air");
}

/// TdMove_LayOnGround.GetUpBack: on your back, pull back (S) to roll backwards into a crouch.
#[test]
fn back_roll_off_your_back() {
    let w = floor();
    let mut c = ctrl_at(Vec3::ZERO);
    c.state = State::LayOnGround { t: 2.0, getting_up: None, back_roll: false };
    let ev = run(&mut c, &w, 1.3, |_, c| {
        let back = matches!(c.state, State::LayOnGround { .. });
        Input { move_axis: Vec2::new(0.0, if back { -1.0 } else { 0.0 }), ..Default::default() }
    });
    assert!(has(&ev, Event::BackRoll), "{ev:?}");
    assert_eq!(c.state, State::Ground);
    assert!((c.feet.z - Tuning::default().back_roll_distance).abs() < 0.05, "rolled back: {:?}", c.feet);
}

/// UTdMove_Balance: walking dead straight along the beam with no input and the view on the
/// beam, nothing tips you (the game has no random wobble); at the edge you get
/// TimeToCounter (0.8 s) to bring it back before you fall.
#[test]
fn balance_lean_follows_the_game() {
    let mut w = floor();
    w.fixtures.push(Fixture::Beam { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(0.0, 0.0, -30.0) });
    let mut c = ctrl_at(Vec3::new(0.0, 0.0, -1.0));
    c.state = State::Balance { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, -30.0), lean: 0.0, danger: -1.0, t: 0.0 };
    let ev = run(&mut c, &w, 2.0, |_, _| Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() });
    let State::Balance { lean, .. } = c.state else { panic!("{:?}", c.state) };
    assert!(lean.abs() < 1e-4 && !has(&ev, Event::BalanceFall), "lean {lean}");

    // Push right and hold it: the lean pins at the edge, and 0.8 s later you're off.
    let mut at_edge = None;
    let mut fell = None;
    run(&mut c, &w, 3.0, |t, c| {
        if let State::Balance { lean, .. } = c.state {
            if lean >= 1.0 && at_edge.is_none() {
                at_edge = Some(t);
            }
        } else if fell.is_none() {
            fell = Some(t);
        }
        Input { move_axis: Vec2::new(1.0, 1.0), ..Default::default() }
    });
    let (e, f) = (at_edge.unwrap(), fell.unwrap());
    assert!(((f - e) - 0.8).abs() < 0.05, "edge at {e}, fell at {f}");
}

/// CheckAgainstWall: walk up to a wall and both hands go up on it; at the wall's edge only the
/// hand still in front of it does; back off and they come down.
#[test]
fn hands_go_up_against_a_wall() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-2.0, 0.0, -3.0), Vec3::new(2.0, 3.0, -2.5)));
    let mut c = ctrl_at(Vec3::new(0.0, 0.0, 0.0));
    run(&mut c, &w, 3.0, |_, _| Input { move_axis: Vec2::new(0.0, 0.4), ..Default::default() });
    assert!(c.against_wall.left.is_some() && c.against_wall.right.is_some(), "{:?} at {:?}", c.against_wall, c.feet);
    // Shuffle right until only the left hand is still in front of the wall's end (x = 2).
    c.feet.x = 2.05;
    run(&mut c, &w, DT, |_, _| Input::default());
    assert!(c.against_wall.left.is_some() && c.against_wall.right.is_none(), "{:?}", c.against_wall);
    c.feet = Vec3::new(0.0, 0.0, 0.0);
    run(&mut c, &w, DT, |_, _| Input::default());
    assert!(c.against_wall == AgainstWall::default(), "{:?}", c.against_wall);
}

/// The grid index finds exactly what scanning every box finds, big boxes included.
#[test]
fn indexed_world_matches_a_full_scan() {
    let mut boxes = vec![Aabb::new(Vec3::new(-500.0, -1.0, -500.0), Vec3::new(500.0, 0.0, 500.0))];
    let mut seed = 7u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1 << 24) as f32
    };
    for _ in 0..2000 {
        let c = Vec3::new(rnd() * 200.0 - 100.0, rnd() * 20.0, rnd() * 200.0 - 100.0);
        let s = Vec3::new(rnd() * 6.0 + 0.1, rnd() * 3.0 + 0.1, rnd() * 6.0 + 0.1);
        boxes.push(Aabb::new(c - s, c + s));
    }
    let plain = BoxWorld { boxes: boxes.clone(), ..Default::default() };
    let indexed = BoxWorld::indexed(boxes, vec![], 4.0);
    for _ in 0..300 {
        let c = Vec3::new(rnd() * 220.0 - 110.0, rnd() * 20.0, rnd() * 220.0 - 110.0);
        let r = Aabb::new(c - Vec3::splat(1.5), c + Vec3::splat(1.5));
        let (mut a, mut b) = (vec![], vec![]);
        plain.query(&r, &mut a);
        indexed.query(&r, &mut b);
        let key = |v: &Vec<Aabb>| {
            let mut k: Vec<[i64; 6]> = v.iter().map(|x| [x.min.x, x.min.y, x.min.z, x.max.x, x.max.y, x.max.z].map(|f| (f * 1000.0) as i64)).collect();
            k.sort();
            k
        };
        assert_eq!(key(&a), key(&b));
    }
}

/// TdMove_ZipLine's native tick: 600 units (6 m) from a wall ahead it braces for it
/// (PrepareForForwardImpact), then hits it (PlayForwardImpact).
#[test]
fn zipline_braces_before_the_wall() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-5.0, 0.0, -32.0), Vec3::new(5.0, 10.0, -30.0)));
    let (a, b) = (Vec3::new(0.0, 6.0, 0.0), Vec3::new(0.0, 5.0, -31.0));
    let mut c = ctrl_at(Vec3::ZERO);
    let u = (b - a).normalize();
    c.feet = a + u * 0.5 - Vec3::Y * c.tuning.zip_hang;
    c.state = State::ZipLine { a, b, s: 0.5, speed: 4.0 };
    let mut braced_at = None;
    let mut ev = Vec::new();
    let mut t = 0.0;
    while t < 10.0 && matches!(c.state, State::ZipLine { .. }) {
        c.step(DT, &Input::default(), &w);
        if braced_at.is_none() && c.events.contains(&Event::ZipBrace) {
            braced_at = Some(c.feet.z);
        }
        ev.extend(c.events.iter().copied());
        t += DT;
    }
    let z = braced_at.expect("braced");
    // The body's front is half a width ahead of its centre.
    let gap = (z - c.tuning.half_width) - (-30.0);
    assert!((5.6..=6.1).contains(&gap), "braced {gap} m from the wall");
    assert!(ev.contains(&Event::ZipEnd { hit_wall: true }), "{ev:?}");
}

/// A takedown only reaches someone close (TargetingMaxDistance, 3 m); which one plays follows
/// where they face against the way to them: turned mostly sideways but a little away, from
/// behind; a little towards her, from the front.
#[test]
fn takedowns_reach_close_and_follow_their_facing() {
    let w = floor();
    let at = |dist: f32, facing: Vec3| {
        let mut c = ctrl_at(Vec3::ZERO);
        // yaw 0 faces -Z: the target straight ahead.
        c.targets = vec![Target { id: 1, centre: Vec3::new(0.0, 0.9, -dist), radius: 0.3, half_height: 0.9, eye: 0.6, facing: facing.normalize() }];
        let ev = run(&mut c, &w, 0.1, |t, _| Input { takedown_pressed: t == 0.0, ..Default::default() });
        ev.iter().find_map(|e| if let Event::Takedown { anim, .. } = *e { Some(anim) } else { None })
    };
    assert_eq!(at(5.0, Vec3::Z), None, "5 m off: out of reach");
    assert_eq!(at(1.5, Vec3::new(1.0, 0.0, -0.2)), Some(3), "sideways, a little away: from behind");
    assert!(at(1.5, Vec3::new(1.0, 0.0, 0.2)).is_some_and(|a| a < 3), "sideways, a little towards her: from the front");
}

/// Up a flight of 25 cm steps: her feet jump each step, but her mesh (and the camera on it)
/// eases up after them (ATdPawn's mesh smoothing), never more than 24 uu behind; on the flat
/// at the top it catches up.
#[test]
fn stairs_ease_the_mesh_up() {
    let mut w = floor();
    for i in 0..8 {
        let z0 = -1.0 - i as f32 * 0.3;
        w.add(Aabb::new(Vec3::new(-2.0, 0.0, -20.0), Vec3::new(2.0, 0.25 * (i + 1) as f32, z0)));
    }
    let mut c = ctrl_at(Vec3::ZERO);
    let mut prev_view = c.feet.y + c.mesh_offset;
    let (mut biggest_feet, mut biggest_view, mut most_held) = (0.0f32, 0.0f32, 0.0f32);
    let mut prev_feet = c.feet.y;
    run(&mut c, &w, 3.0, |_, c| {
        let view = c.feet.y + c.mesh_offset;
        biggest_feet = biggest_feet.max(c.feet.y - prev_feet);
        biggest_view = biggest_view.max(view - prev_view);
        most_held = most_held.max(c.mesh_offset.abs());
        prev_feet = c.feet.y;
        prev_view = view;
        Input { move_axis: Vec2::new(0.0, 0.5), ..Default::default() }
    });
    assert!(c.feet.y > 1.9, "climbed to {}", c.feet.y);
    assert!(biggest_feet > 0.2, "the feet step ({biggest_feet})");
    assert!(biggest_view < biggest_feet * 0.6, "the view eases: {biggest_view} a frame against the feet's {biggest_feet}");
    assert!(most_held <= 0.24 + 1e-4, "held back at most 24 uu: {most_held}");
    run(&mut c, &w, 1.0, |_, _| Input::default());
    assert!(c.mesh_offset.abs() < 1e-4, "caught up: {}", c.mesh_offset);
}

/// A 42 cm step: as shipped (auto step-up off) she stops against it; with TdMove_AutoStepUp on
/// she steps up onto it and walks on. A 25 cm one the walk climbs by itself, no step-up.
#[test]
fn auto_step_up_onto_a_knee_high_step() {
    let walk_at = |rise: f32, on: bool| {
        let mut w = floor();
        w.add(Aabb::new(Vec3::new(-2.0, 0.0, -10.0), Vec3::new(2.0, rise, -1.0)));
        let mut c = ctrl_at(Vec3::ZERO);
        c.tuning.auto_step_up = on;
        let ev = run(&mut c, &w, 2.0, |_, _| Input { move_axis: Vec2::new(0.0, 0.5), ..Default::default() });
        (c.feet, ev.contains(&Event::StepUp))
    };
    let (feet, stepped) = walk_at(0.42, false);
    assert!(feet.y < 0.05 && !stepped, "shipped: stopped at the step, {feet}");
    let (feet, stepped) = walk_at(0.42, true);
    assert!(stepped && (feet.y - 0.42).abs() < 0.03 && feet.z < -1.5, "stepped up and on: {feet}");
    let (feet, stepped) = walk_at(0.25, true);
    assert!(!stepped && (feet.y - 0.25).abs() < 0.03, "walked up a low one: {feet}");
}

/// A slope too steep to stand on (49 degrees: normal 0.66) slides her down it on her backside
/// (TdMove_RumpSlide) to the flat at the bottom; a 30 degree one she just walks down.
#[test]
fn rump_slide_down_a_steep_slope() {
    let slide = |deg: f32| {
        let rise = 6.0;
        let run = rise / deg.to_radians().tan();
        // Top at z = 0 (height 6), down to z = -run (height 0), flat beyond.
        let mut tris = vec![];
        tris.extend(MeshWorld::quad(
            Vec3::new(-3.0, rise, 0.0),
            Vec3::new(3.0, rise, 0.0),
            Vec3::new(3.0, 0.0, -run),
            Vec3::new(-3.0, 0.0, -run),
        ));
        tris.extend(MeshWorld::quad(Vec3::new(-3.0, rise, 4.0), Vec3::new(3.0, rise, 4.0), Vec3::new(3.0, rise, 0.0), Vec3::new(-3.0, rise, 0.0)));
        tris.extend(MeshWorld::quad(Vec3::new(-3.0, 0.0, -run), Vec3::new(3.0, 0.0, -run), Vec3::new(3.0, 0.0, -run - 20.0), Vec3::new(-3.0, 0.0, -run - 20.0)));
        let w = MeshWorld::new(tris, vec![]);
        let mut c = Controller::new(Tuning::default(), Vec3::new(0.0, rise, 1.0), 0.0);
        c.state = State::Air;
        let mut ev = vec![];
        let mut slid = false;
        for _ in 0..(6 * 60) {
            c.step(1.0 / 60.0, &Input { move_axis: Vec2::new(0.0, 0.3), ..Default::default() }, &w);
            ev.extend(c.events.iter().copied());
            slid |= matches!(c.state, State::RumpSlide { .. });
        }
        (c.feet, slid, ev.contains(&Event::RumpSlide))
    };
    let (feet, slid, event) = slide(49.0);
    assert!(slid && event, "slid down the steep one");
    assert!(feet.y < 0.2, "reached the bottom: {feet}");
    let (_, slid, _) = slide(30.0);
    assert!(!slid, "walked down the 30 degree one");
}

/// Hanging from a window sill, jump while pushing up: with the next ledge 1.2 m above on the
/// same wall, she reaches up to it (TdMove_GrabTransfer) and hangs there; without one she pulls
/// up as before.
#[test]
fn grab_transfer_up_to_the_ledge_above() {
    let hang_then_jump = |upper: bool| {
        let mut w = floor();
        // A wall to 2 m (face at z = 0, she's on the +z side), a window from 2 to 2.6 m.
        w.add(Aabb::new(Vec3::new(-3.0, 0.0, -1.0), Vec3::new(3.0, 2.0, 0.0)));
        w.add(Aabb::new(Vec3::new(-3.0, 2.0, -1.0), Vec3::new(3.0, 2.6, -0.8)));
        if upper {
            w.add(Aabb::new(Vec3::new(-3.0, 2.6, -1.0), Vec3::new(3.0, 3.2, 0.0)));
        }
        let mut c = ctrl_at(Vec3::ZERO);
        let hands = c.tuning.hang_hands_above_feet;
        c.feet = Vec3::new(0.0, 2.0 - hands, c.tuning.half_width + 0.02);
        c.state = State::LedgeHang { normal: Vec3::Z, ledge_y: 2.0, turned: false };
        c.yaw = 0.0;
        run(&mut c, &w, 1.0, |_, _| Input::default());
        let ev = run(&mut c, &w, 1.5, |t, _| Input { jump_pressed: t < 0.02, move_axis: Vec2::new(0.0, if t < 0.1 { 1.0 } else { 0.0 }), ..Default::default() });
        (c.state, ev)
    };
    let (state, ev) = hang_then_jump(true);
    assert!(ev.contains(&Event::GrabTransfer), "{ev:?}");
    assert!(matches!(state, State::LedgeHang { ledge_y, .. } if (ledge_y - 3.2).abs() < 0.05), "hangs from the upper ledge: {state:?}");
    let (_, ev) = hang_then_jump(false);
    assert!(!ev.contains(&Event::GrabTransfer) && ev.contains(&Event::PullUp), "no ledge above: pulls up: {ev:?}");
}

/// TdMove_MeleeVault: attack pressed mid-vault turns the way down into a kick, 0.3 s in, that
/// lands on someone standing past the rail.
#[test]
fn vault_kick_over_a_rail() {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-5.0, 0.0, -5.2), Vec3::new(5.0, 1.0, -5.0)));
    let mut c = ctrl_at(Vec3::ZERO);
    c.targets = vec![Target { id: 7, centre: Vec3::new(0.0, 0.9, -7.7), radius: 0.3, half_height: 0.9, eye: 0.6, facing: Vec3::Z }];
    let mut pressed = false;
    let ev = run(&mut c, &w, 2.5, |_, c| {
        let melee = !pressed && matches!(c.state, State::Vault(_));
        pressed |= melee;
        Input { jump_pressed: c.feet.z < -4.2 && c.feet.z > -4.9, melee_pressed: melee, ..fwd() }
    });
    assert!(has(&ev, Event::Vault), "events {ev:?}");
    assert!(ev.iter().any(|e| matches!(e, Event::Melee { kind: MeleeKind::VaultKick, .. })), "events {ev:?}");
    assert!(ev.iter().any(|e| matches!(e, Event::MeleeHit { target: 7, kind: MeleeKind::VaultKick, .. })), "events {ev:?}");
    assert!(c.feet.z < -5.4, "past the rail: {}", c.feet.z);
}

/// TdMove_AirBarge: attack in a jump at a door and she goes through it shoulder first,
/// then lands and walks on.
#[test]
fn air_barge_through_a_door() {
    let w = door_world();
    let mut c = ctrl_at(Vec3::new(0.0, 0.0, -2.0));
    let mut jumped = false;
    let mut swung = false;
    let ev = run(&mut c, &w, 3.0, |_, c| {
        let jump = !jumped && c.feet.z < -3.4;
        jumped |= jump;
        let melee = jumped && !swung && c.state == State::Air;
        swung |= melee;
        Input { jump_pressed: jump, melee_pressed: melee, ..fwd() }
    });
    assert!(has(&ev, Event::AirBarge), "{ev:?}");
    assert!(has(&ev, Event::DoorOpened { door: 0 }) && has(&ev, Event::AirBargeImpact), "{ev:?}");
    assert!(has(&ev, Event::AirBargeLand), "{ev:?}");
    assert!(c.feet.z < -6.5 && c.state == State::Ground, "through: {:?} {:?}", c.feet, c.state);
}

/// A rooftop 3 m across with nothing under its edge.
fn rooftop() -> BoxWorld {
    let mut w = BoxWorld::default();
    w.add(Aabb::new(Vec3::new(-5.0, -1.0, -3.0), Vec3::new(5.0, 0.0, 5.0)));
    w
}

/// TdMove_Vertigo: walking up to a long drop she stops at the edge and looks down over it;
/// stepping back afterwards hands back to walking.
#[test]
fn vertigo_at_a_rooftop_edge() {
    let w = rooftop();
    let mut c = ctrl_at(Vec3::ZERO);
    let walk = Input { move_axis: Vec2::new(0.0, 0.4), ..Default::default() };
    let ev = run(&mut c, &w, 3.0, |_, _| walk);
    assert!(has(&ev, Event::Vertigo), "{ev:?}");
    assert!(matches!(c.state, State::Vertigo { .. }), "still looking over while pushing on: {:?}", c.state);
    assert!(c.feet.z > -3.0 && c.feet.y.abs() < 1e-3, "at the edge: {:?}", c.feet);
    assert!(c.pitch < -1.2, "looking down: {}", c.pitch);
    assert!(c.vertigo_zoom || c.state != State::Ground);
    run(&mut c, &w, 0.5, |_, _| Input { move_axis: Vec2::new(0.0, -1.0), ..Default::default() });
    assert_eq!(c.state, State::Ground);
    assert!(!c.vertigo_zoom);
}

/// Running (WAS_Run and up), she goes straight off.
#[test]
fn no_vertigo_at_a_run() {
    let w = rooftop();
    let mut c = ctrl_at(Vec3::ZERO);
    let ev = run(&mut c, &w, 2.5, |_, _| fwd());
    assert!(!has(&ev, Event::Vertigo), "{ev:?}");
    assert!(c.feet.y < -1.0, "off the edge: {:?}", c.feet);
}

/// TdMove_SwingJump: jumping off a swing bar with another one ahead carries her across to it,
/// and she swings on from that one.
#[test]
fn swing_to_swing() {
    let mut w = BoxWorld::default();
    w.add(Aabb::new(Vec3::new(-20.0, -11.0, -20.0), Vec3::new(20.0, -10.0, 20.0)));
    w.fixtures.push(Fixture::SwingPole { a: Vec3::new(-2.0, 3.0, 0.0), b: Vec3::new(2.0, 3.0, 0.0) });
    w.fixtures.push(Fixture::SwingPole { a: Vec3::new(-2.0, 3.0, -3.5), b: Vec3::new(2.0, 3.0, -3.5) });
    let mut c = ctrl_at(Vec3::new(0.0, 0.95, 0.3));
    c.state = State::Air;
    c.vel = Vec3::new(0.0, 0.0, -4.0);
    let mut jumped = false;
    let mut second = None;
    let ev = run(&mut c, &w, 4.0, |_, c| {
        if let State::Swing { at, .. } = c.state {
            if at.z < -3.0 {
                second.get_or_insert(at);
            }
        }
        let jump = !jumped && matches!(c.state, State::Swing { rate, at, .. } if rate > 3.0 && at.z > -1.0);
        jumped |= jump;
        Input { jump_pressed: jump, move_axis: Vec2::new(0.0, if jumped { 0.0 } else { 1.0 }), ..Default::default() }
    });
    assert!(has(&ev, Event::SwingToSwing), "{ev:?}");
    assert!(second.is_some(), "caught the second bar: {ev:?} {:?}", c.feet);
}

/// A wall 4 m high at z = -6 with a ladder (or drainpipe) up it.
fn ladder_wall(pipe: bool) -> BoxWorld {
    let mut w = floor();
    w.add(Aabb::new(Vec3::new(-5.0, 0.0, -9.0), Vec3::new(5.0, 4.0, -6.0)));
    w.fixtures.push(Fixture::Ladder(Ladder { base: Vec3::new(0.0, 0.0, -6.0), top: 4.0, normal: Vec3::Z, pipe, exit: true }));
    w
}

/// TdMove_IntoClimb / TdMove_Climb: walk up to a ladder holding forward, climb it step by step
/// and over the top onto the roof.
#[test]
fn climb_a_ladder_onto_the_roof() {
    for pipe in [false, true] {
        let w = ladder_wall(pipe);
        let mut c = ctrl_at(Vec3::new(0.0, 0.0, -3.0));
        let ev = run(&mut c, &w, 9.0, |_, c| if c.feet.y > 3.9 && c.state == State::Ground { Input::default() } else { fwd() });
        assert!(ev.iter().any(|e| matches!(e, Event::ClimbStart { .. })) || ev.iter().any(|e| matches!(e, Event::ClimbStep)), "pipe {pipe}: {ev:?}");
        assert!(has(&ev, Event::ClimbExit), "pipe {pipe}: {ev:?} {:?} {:?}", c.feet, c.state);
        assert!((c.feet.y - 4.0).abs() < 0.05 && c.feet.z < -6.0 && c.state == State::Ground, "pipe {pipe}: on the roof: {:?} {:?}", c.feet, c.state);
    }
}

/// bCanExitAtTop off: holding up at the top she stays on the ladder, at its last step.
#[test]
fn ladder_without_an_exit_holds_her_at_the_top() {
    for pipe in [false, true] {
        let mut w = ladder_wall(pipe);
        if let Some(Fixture::Ladder(l)) = w.fixtures.last_mut() {
            l.exit = false;
        }
        let mut c = ctrl_at(Vec3::new(0.0, 0.0, -3.0));
        let ev = run(&mut c, &w, 9.0, |_, _| fwd());
        assert!(!has(&ev, Event::ClimbExit), "pipe {pipe}: {ev:?}");
        assert!(matches!(c.state, State::Climb { .. }), "pipe {pipe}: {:?}", c.state);
        assert!(c.feet.y < 4.0, "pipe {pipe}: above the top: {:?}", c.feet);
    }
}

/// Down again: pushing back climbs down, and at the bottom with the floor under her she's off.
#[test]
fn climb_down_a_ladder_and_off() {
    let w = ladder_wall(false);
    let mut c = ctrl_at(Vec3::new(0.0, 0.0, -3.0));
    run(&mut c, &w, 2.0, |_, c| if matches!(c.state, State::Climb { step, .. } if step >= 3) { Input::default() } else { fwd() });
    assert!(matches!(c.state, State::Climb { .. }), "{:?}", c.state);
    let ev = run(&mut c, &w, 4.0, |_, c| if matches!(c.state, State::Climb { .. }) { Input { move_axis: Vec2::new(0.0, -0.5), ..Default::default() } } else { Input::default() });
    assert!(has(&ev, Event::ClimbLetGo), "{ev:?}");
    assert_eq!(c.state, State::Ground);
    assert!(c.feet.y.abs() < 0.05, "{:?}", c.feet);
}

/// Over the top of a ladder from the roof, facing out: LadderEnterTop onto it.
#[test]
fn onto_a_ladder_from_the_top() {
    let w = ladder_wall(false);
    let mut c = ctrl_at(Vec3::new(0.0, 4.0, -7.5));
    c.yaw = std::f32::consts::PI; // facing +Z, out over the edge
    let ev = run(&mut c, &w, 3.0, |_, c| if matches!(c.state, State::Climb { .. }) { Input::default() } else { fwd() });
    assert!(ev.iter().any(|e| matches!(e, Event::ClimbStart { start: ClimbStart::EnterTop, .. })), "{ev:?} {:?}", c.state);
    assert!(matches!(c.state, State::Climb { .. }), "{:?} {:?}", c.state, c.feet);
}

/// The training course's pair of swing bars: jump up to the first, swing, and across to the next.
#[test]
fn training_swing_bars_carry_across() {
    use crate::greybox::{self, ROOF_D_Y};
    let w = greybox::greybox().world();
    let mut c = ctrl_at(Vec3::new(12.2, ROOF_D_Y, -109.0));
    let mut caught = false;
    let mut jumped = false;
    let mut took_off = false;
    let ev = run(&mut c, &w, 6.0, |_, c| {
        let on_first = matches!(c.state, State::Swing { at, .. } if at.z > -116.0);
        caught |= on_first;
        let off = !took_off && c.feet.z < -113.4;
        took_off |= off;
        let jump = off || (!jumped && matches!(c.state, State::Swing { rate, at, .. } if rate > 3.0 && at.z > -116.0));
        jumped |= jump && caught;
        Input { jump_pressed: jump, move_axis: Vec2::new(0.0, if jumped { 0.0 } else { 1.0 }), ..Default::default() }
    });
    assert!(caught, "caught the first bar: {ev:?}");
    assert!(has(&ev, Event::SwingToSwing), "{ev:?}");
    assert!(matches!(c.state, State::Swing { at, .. } if at.z < -118.0) || ev.iter().filter(|e| **e == Event::SwingStart).count() >= 2, "{ev:?} {:?}", c.state);
}

/// The training course's ladder and drainpipe up to the top roof: climbed from roof B2 onto C.
#[test]
fn training_ladder_and_pipe_reach_the_top_roof() {
    use crate::greybox::{self, z, ROOF_B_Y, ROOF_C_Y};
    let w = greybox::greybox().world();
    for x in [4.2, -4.2] {
        let mut c = ctrl_at(Vec3::new(x, ROOF_B_Y, z::CLIMB_WALL + 3.0));
        let ev = run(&mut c, &w, 10.0, |_, c| if c.feet.y > ROOF_C_Y - 0.1 && c.state == State::Ground { Input::default() } else { fwd() });
        assert!(has(&ev, Event::ClimbExit), "x {x}: {ev:?} {:?} {:?}", c.feet, c.state);
        assert!((c.feet.y - ROOF_C_Y).abs() < 0.05 && c.state == State::Ground, "x {x}: {:?} {:?}", c.feet, c.state);
    }
}

/// Stone city stairs: 34.3 cm risers, 48 cm treads. Under ME's
/// 35 cm MaxStepHeight, so the walk climbs them by itself, walking and running.
#[test]
fn walks_up_city_stairs() {
    for push in [0.4, 1.0] {
        let mut w = floor();
        for i in 0..6 {
            let z0 = -2.0 - i as f32 * 0.48;
            w.add(Aabb::new(Vec3::new(-2.0, 0.0, -20.0), Vec3::new(2.0, 0.343 * (i + 1) as f32, z0)));
        }
        let mut c = ctrl_at(Vec3::ZERO);
        let mut top: f32 = 0.0;
        run(&mut c, &w, 4.0, |_, c| {
            top = top.max(c.feet.y);
            Input { move_axis: Vec2::new(0.0, push), ..Default::default() }
        });
        assert!(top > 0.343 * 6.0 - 0.01, "push {push}: got to {top}");
    }
}

/// The same stairs as triangles (what a game engine's static collision is).
#[test]
fn walks_up_city_stairs_as_triangles() {
    for rise in [0.30f32, 0.343, 0.347] {
        let mut tris = MeshWorld::oriented_box(Vec3::new(0.0, -0.5, 0.0), Vec3::new(20.0, 0.5, 20.0), 0.0);
        for i in 0..6 {
            let h = rise * (i + 1) as f32;
            let z0 = -2.0 - i as f32 * 0.48;
            tris.extend(MeshWorld::oriented_box(Vec3::new(0.0, h * 0.5, (z0 - 10.0) * 0.5), Vec3::new(2.0, h * 0.5, (10.0 + z0) * 0.5), 0.0));
        }
        let w = MeshWorld::new(tris, vec![]);
        let mut c = ctrl_at(Vec3::ZERO);
        let mut top: f32 = 0.0;
        for push in [0.4f32, 1.0] {
            c = ctrl_at(Vec3::ZERO);
            for _ in 0..240 {
                c.step(DT, &Input { move_axis: Vec2::new(0.0, push), ..Default::default() }, &w);
                top = top.max(c.feet.y);
                // Stairs aren't a slope to slide down.
                assert!(!matches!(c.state, State::RumpSlide { .. }), "rise {rise} push {push}: rump slide at {:?}", c.feet);
            }
        }
        assert!(top > rise * 6.0 - 0.01, "rise {rise}: got to {top}, {:?}", c.feet);
    }
}

/// Stairs whose collision is only their tops (some games' stairs are): the edge of the next top is met
/// side on, and the walk steps up onto it instead of walking into the step.
#[test]
fn walks_up_stairs_with_only_tops() {
    let mut tris = MeshWorld::oriented_box(Vec3::new(0.0, -0.5, 0.0), Vec3::new(20.0, 0.5, 20.0), 0.0);
    for i in 0..6 {
        let y = 0.343 * (i + 1) as f32;
        let (z0, z1) = (-2.0 - i as f32 * 0.48, -2.0 - (i + 1) as f32 * 0.48);
        tris.extend(MeshWorld::quad(Vec3::new(-2.0, y, z0), Vec3::new(2.0, y, z0), Vec3::new(2.0, y, z1), Vec3::new(-2.0, y, z1)));
    }
    let w = MeshWorld::new(tris, vec![]);
    let mut c = ctrl_at(Vec3::ZERO);
    let mut top: f32 = 0.0;
    for _ in 0..300 {
        c.step(DT, &Input { move_axis: Vec2::new(0.0, 0.4), ..Default::default() }, &w);
        top = top.max(c.feet.y);
    }
    assert!(top > 0.343 * 6.0 - 0.01, "got to {top}, {:?}", c.feet);
}
