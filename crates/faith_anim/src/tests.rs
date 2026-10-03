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

/// As in Mirror's Edge (TdMove_Walking): standing still with the view untouched for 30-40 s
/// plays one of the stand idles; turning the view stops it.
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
    assert!(!crate::Driver::AUTO_IDLES.iter().any(|i| rig.driver.current().eq_ignore_ascii_case(i)), "looking round stops it: {}", rig.driver.current());
}
