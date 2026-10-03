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
        "fallinglandhard2", "HangHardStart2", "HangHardStart3", "gethitfront", "hangturnjump", "jumpstill", "jumpfast", "fallinglandroll",
        "CrouchSlide", "Hang", "HangStrafeLeft", "HangStrafeRight", "HangHeaveUp", "VaultOver", "VaultOnto", "VaultOntoHigh", "RunTurn180", "StandTurn180Right", "SnatchFwd", "SnatchFwd2", "SnatchFwd3", "SnatchBack",
    ];
    let missing: Vec<_> = clips.iter().filter(|c| crate::Driver::length(&arms, c).is_none()).collect();
    assert!(missing.is_empty(), "missing: {missing:?}");
}
