//! Animation checks against Faith's real skeleton and animations. Need ME_INSTALL (skip
//! otherwise).

use glam::Vec2;
use me_assets::FaithArms;

use crate::Rig;

fn arms() -> Option<FaithArms> {
    let dir = std::env::var_os("ME_INSTALL")?;
    Some(FaithArms::load(std::path::Path::new(&dir), 4).expect("load"))
}

/// Standing still, the idle key plays each of Faith's idles in turn, and it keeps playing.
#[test]
fn idles_play_when_standing() {
    let Some(arms) = arms() else { return };
    use faith_move::{greybox, CameraFx, Controller, Input, Tuning};
    let level = greybox::greybox();
    let world = level.world();
    let cp = &level.checkpoints[0];
    let mut c = Controller::new(Tuning::default(), cp.spawn, cp.yaw);
    let mut fx = CameraFx::default();
    let mut rig = Rig::new(&arms, c.yaw);
    let dt = 1.0 / 60.0;
    let mut step = |c: &mut Controller, rig: &mut Rig| {
        let input = Input::default();
        c.step(dt, &input, &world);
        let shot = fx.update(dt, c, &input);
        rig.update(dt, c, &shot, &arms);
    };
    for _ in 0..60 {
        step(&mut c, &mut rig);
    }
    for idle in crate::Driver::IDLES {
        rig.driver.play_idle(&c, &arms);
        let mut seen = vec![];
        for _ in 0..30 {
            step(&mut c, &mut rig);
            seen.push(rig.driver.current().to_string());
        }
        eprintln!("{idle}: {:?} speed {} state {}", &seen[..3], c.horizontal_speed(), c.state.name());
        assert!(seen.iter().all(|s| s.eq_ignore_ascii_case(idle)), "{idle}: {seen:?}");
        for _ in 0..400 {
            step(&mut c, &mut rig);
        }
    }
}

/// As in Mirror's Edge (TdMove_Walking): standing still for 30-40 s plays one of the stand
/// idles. Unlike it, looking round doesn't stop one (moving does).
#[test]
fn idles_play_on_their_own() {
    let Some(arms) = arms() else { return };
    use faith_move::{greybox, CameraFx, Controller, Input, Tuning};
    let level = greybox::greybox();
    let world = level.world();
    let cp = &level.checkpoints[0];
    let mut c = Controller::new(Tuning::default(), cp.spawn, cp.yaw);
    let mut fx = CameraFx::default();
    let mut rig = Rig::new(&arms, c.yaw);
    let dt = 1.0 / 60.0;
    let mut step = |c: &mut Controller, rig: &mut Rig, look: f32| {
        let input = Input { look: Vec2::new(look, 0.0), ..Default::default() };
        c.step(dt, &input, &world);
        let shot = fx.update(dt, c, &input);
        rig.update(dt, c, &shot, &arms);
    };
    let mut started = None;
    for f in 0..(45 * 60) {
        step(&mut c, &mut rig, 0.0);
        if crate::Driver::AUTO_IDLES.iter().any(|i| rig.driver.current().eq_ignore_ascii_case(i)) {
            started = Some(f as f32 * dt);
            break;
        }
    }
    let t = started.expect("an idle played within 45 s");
    assert!((29.9..=40.5).contains(&t), "idle after {t} s");
    for _ in 0..30 {
        step(&mut c, &mut rig, 0.01);
    }
    assert!(crate::Driver::AUTO_IDLES.iter().any(|i| rig.driver.current().eq_ignore_ascii_case(i)), "looking round keeps it: {}", rig.driver.current());
}

/// Walking (half stick) then letting go: how fast she slows, and how far the
/// hands and camera move from frame to frame while the walk gives way to standing. A stop that
/// "jolts" shows as one frame moving far more than the frames around it.
#[test]
fn walking_to_a_stop_is_smooth() {
    let Some(arms) = arms() else { return };
    use faith_move::{greybox, CameraFx, Controller, Input, Tuning};
    let level = greybox::greybox();
    let world = level.world();
    let cp = &level.checkpoints[0];
    let mut c = Controller::new(Tuning::default(), cp.spawn, cp.yaw);
    let mut fx = CameraFx::default();
    let mut rig = Rig::new(&arms, c.yaw);
    let dt = 1.0 / 60.0;
    let hand = arms.bone("RightHand").unwrap();
    let eye = arms.bone("EyeJoint").unwrap();
    let mut last: Option<(glam::Vec3, glam::Vec3)> = None;
    let mut jumps = vec![];
    for f in 0..(4 * 60) {
        let walking = f < 2 * 60;
        let input = Input { move_axis: Vec2::new(0.0, if walking { 0.5 } else { 0.0 }), ..Default::default() };
        c.step(dt, &input, &world);
        let shot = fx.update(dt, &c, &input);
        rig.update(dt, &c, &shot, &arms);
        let g = &rig.driver.globals;
        let now = (g[hand].w_axis.truncate(), g[eye].w_axis.truncate());
        if let Some(l) = last {
            if f >= 2 * 60 - 10 {
                jumps.push(((now.0 - l.0).length(), (now.1 - l.1).length(), c.horizontal_speed(), rig.driver.current().to_string()));
            }
        }
        last = Some(now);
    }
    for (i, j) in jumps.iter().enumerate().take(50) {
        eprintln!("{i:3} hand {:6.2} eye {:6.2} speed {:5.2} {}", j.0, j.1, j.2, j.3);
    }
    // No frame moves the hand more than 2.5x the median of its neighbours (a jolt).
    for w in jumps.windows(7) {
        let mut around: Vec<f32> = w.iter().map(|j| j.0).collect();
        let mid = around[3];
        around.remove(3);
        around.sort_by(f32::total_cmp);
        let med = around[3].max(0.3);
        assert!(mid <= med * 2.5, "a jolt: {mid:.2} against {med:.2} around it");
    }
}

/// Every clip the moves play (as Mirror's Edge's scripts name them, in Faith's set): a missing
/// one would leave her in the rest pose.
#[test]
fn the_clips_the_moves_play_exist() {
    let Some(arms) = arms() else { return };
    let clips = [
        "Stand", "sneakfwd", "sneakbwd", "walkfwd", "walkfwdstiff", "walkbwd", "runfwd", "runfwdstiff", "runbwd", "SprintFwd",
        "SpringBoardLeftLeg", "SpringBoardRightLeg", "swingjumpoff", "swinghardstart", "swing180", "WallrunJumpLeft", "wallrunjumpright",
        "JumpSlow", "JumpTurnFly", "dodgejumpleft", "dodgejumpright", "HangHardStart", "walkbalancefalloffleft", "walkbalancefalloffright",
        "wallrunrightstart", "wallrunleftstart", "WallRunVertical", "wallrunvertical180turn", "jumpcoil", "ziplinestart", "ZipLine",
        "ziplineintohitwall", "ziplinehitwall", "bargeinleft", "bargeoutleft", "meleekickobject", "crouchslidetocrouch", "fallinglandhard",
        "fallinglandhard2", "HangHardStart2", "HangHardStart3", "gethitfront", "hangturnjump", "autostepuprightleg", "crouchslideintoend45", "crouchslideend45", "hangtransferup", "jumpstill", "jumpfast", "fallinglandroll",
        "CrouchSlide", "Hang", "HangStrafeLeft", "HangStrafeRight", "HangHeaveUp", "VaultOver", "VaultOnto", "VaultOntoHigh", "RunTurn180", "StandTurn180Right", "SnatchFwd", "SnatchFwd2", "SnatchFwd3", "SnatchBack",
        "MeleeVaultOver", "AirBargeIdle", "AirBargeImpact", "AirBargeLand", "edgedetection", "swingoff",
        "LadderClimbHangStart", "LadderClimbHangStartLeft", "LadderClimbHangStartRight", "LadderEnterTop", "LadderClimbUpLeftHand",
        "LadderClimbUpRightHand", "LadderClimbUpLeftHandStill", "LadderClimbUpRightHandStill", "LadderClimbDownFast", "LadderExitTopLeftHand",
        "LadderExitTopRightHand", "PipeClimbHangStart", "PipeClimbHangStartHard", "PipeClimbHangStartLeft", "PipeClimbHangStartRight",
        "PipeClimbStart", "PipeClimbUpLeftHand", "PipeClimbUpRightHand", "pipeclimbupfastlefthand", "pipeclimbupfastrighthand",
        "PipeClimbUpLeftHandStill", "PipeClimbUpRightHandStill", "PipeClimbDownFast", "pipeexittoplefthand", "pipeexittoprighthand", "PipeExitBottom",
    ];
    let missing: Vec<_> = clips.iter().filter(|c| crate::Driver::length(&arms, c).is_none()).collect();
    assert!(missing.is_empty(), "missing: {missing:?}");
}

/// Walking over small things on the floor (clutter, kerbs, 5-30 cm) plays about as many step
/// sounds as walking on the flat: no burst when she steps on one.
#[test]
fn stepping_on_small_things_doesnt_spam_steps() {
    let Some(arms) = arms() else { return };
    use crate::sound::{Director, SoundCmd};
    use faith_move::greybox::Surface;
    use faith_move::{Aabb, BoxWorld, CameraFx, Controller, Input, State, Tuning};
    use glam::Vec3;
    let count = |bumps: bool, stick: f32| {
        let mut w = BoxWorld::default();
        w.add(Aabb::new(Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0)));
        if bumps {
            for i in 0..12 {
                let h = [0.05, 0.12, 0.2, 0.3][i % 4];
                let z = -1.5 - i as f32 * 0.9;
                w.add(Aabb::new(Vec3::new(-2.0, 0.0, z - 0.3), Vec3::new(2.0, h, z)));
            }
        }
        let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
        c.state = State::Ground;
        let mut fx = CameraFx::default();
        let mut rig = crate::Rig::new(&arms, c.yaw);
        let cues = crate::sound::wanted_cues(Some(&arms)).into_iter().map(|c| (c.to_ascii_lowercase(), crate::sound::CueInfo { variants: 1, volume: (1.0, 1.0), pitch: (1.0, 1.0), looping: false })).collect();
        let mut d = Director::new(cues);
        let (mut steps, mut lands, mut worst) = (0, 0, 0);
        for _ in 0..(4 * 60) {
            let input = Input { move_axis: glam::Vec2::new(0.0, stick), ..Default::default() };
            c.step(1.0 / 60.0, &input, &w);
            let shot = fx.update(1.0 / 60.0, &c, &input);
            rig.update(1.0 / 60.0, &c, &shot, &arms);
            let mut out = vec![];
            d.update(1.0 / 60.0, &c, rig.driver.notifies(), true, 0.0, &|_| Surface::Concrete, &mut out);
            let mut frame = 0;
            for cmd in out {
                if let SoundCmd::Play { cue, .. } = cmd {
                    if cue.contains("footstep") {
                        steps += 1;
                        frame += 1;
                        if cue.contains("land") {
                            lands += 1;
                        }
                    }
                }
            }
            worst = worst.max(frame);
        }
        (steps, lands, worst)
    };
    for stick in [0.5, 1.0] {
        let flat = count(false, stick);
        let bumpy = count(true, stick);
        eprintln!("stick {stick}: flat {flat:?}, bumpy {bumpy:?} (steps, landings, most in a frame)");
        assert!(bumpy.0 <= flat.0 + 8 && bumpy.2 <= 2, "stick {stick}: flat {flat:?} against bumpy {bumpy:?}");
    }
}

/// A speed hovering on the walk / sneak line (the walking state flipping every frame, as
/// stepping on something can make it) steps no more than steady walking does.
#[test]
fn a_flickering_walk_state_doesnt_spam_steps() {
    let Some(arms) = arms() else { return };
    use faith_move::{Controller, State, Tuning};
    use me_assets::anim::Notify;
    let steps = |flicker: bool| {
        let mut c = Controller::new(Tuning::default(), glam::Vec3::ZERO, 0.0);
        c.state = State::Ground;
        let mut d = crate::Driver::new(&arms);
        let mut n = 0;
        for f in 0..(4 * 60) {
            let speed = if flicker && f % 2 == 0 { 0.45 } else { 0.55 };
            c.vel = glam::Vec3::new(0.0, 0.0, -speed);
            d.update(1.0 / 60.0, &c, &arms);
            n += d.notifies().iter().filter(|x| matches!(x, Notify::Footstep(_))).count();
        }
        n
    };
    let (steady, flicker) = (steps(false), steps(true));
    eprintln!("steady {steady}, flickering {flicker}");
    assert!(flicker <= steady * 2 + 2, "steady {steady} steps, flickering {flicker}");
}


/// Vaulting a block-high wall: after the vault, the camera comes off her eye smoothly (no lurch
/// ahead as the vault clip's tail fades). Prints each frame with --nocapture.
#[test]
fn camera_after_a_vault_stays_with_her() {
    let Some(arms) = arms() else { return };
    use faith_move::{Aabb, BoxWorld, CameraFx, CameraFxSettings, Controller, Input, State, Tuning};
    use glam::{Vec2, Vec3};
    let mut w = BoxWorld::default();
    w.add(Aabb::new(Vec3::new(-50.0, -1.0, -50.0), Vec3::new(50.0, 0.0, 50.0)));
    w.add(Aabb::new(Vec3::new(-4.0, 0.0, -7.0), Vec3::new(4.0, 1.0, -6.0)));
    let mut c = Controller::new(Tuning::default(), Vec3::ZERO, 0.0);
    c.state = State::Ground;
    let mut fx = CameraFx::new(CameraFxSettings::animation_driven());
    let mut rig = Rig::new(&arms, c.yaw);
    let dt = 1.0 / 60.0;
    let mut jumped = false;
    let mut worst: f32 = 0.0;
    let mut after = None;
    for i in 0..150 {
        let jump = !jumped && c.feet.z < -4.6;
        jumped |= jump;
        let input = Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: jump, ..Default::default() };
        c.step(dt, &input, &w);
        let shot = fx.update(dt, &c, &input);
        let r = rig.update(dt, &c, &shot, &arms);
        let ahead = -(r.cam_pos.z - c.view().eye.z);
        if matches!(c.state, State::Vault(_)) {
            after = Some(0);
        } else if let Some(n) = &mut after {
            *n += 1;
            if *n < 40 {
                worst = worst.max(ahead);
            }
        }
        eprintln!("{i} {} ahead {ahead:.2} align {:.2} | {}", c.state.name(), r.body.align, rig.driver.layer_summary());
    }
    assert!(worst < 0.25, "the camera lurched {worst:.2} m ahead of her after the vault");
}
