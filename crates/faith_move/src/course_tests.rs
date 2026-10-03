//! Full-course runs: start at each checkpoint, drive scripted input, and make
//! sure the section is actually beatable with the current tuning.

use glam::{Vec2, Vec3};

use crate::greybox::{self, z, ROOF_B_Y, ROOF_C_Y, ROOF_D_Y};
use crate::*;

const DT: f32 = 1.0 / 60.0;

pub(crate) struct Sim {
    pub(crate) c: Controller,
    pub(crate) w: BoxWorld,
    pub(crate) level: greybox::Level,
    pub(crate) events: Vec<Event>,
    pub(crate) t: f32,
    pub(crate) deaths: usize,
}

impl Sim {
    fn at_checkpoint(i: usize) -> Self {
        Self::on(greybox::greybox(), i)
    }

    pub(crate) fn on(level: greybox::Level, i: usize) -> Self {
        let cp = &level.checkpoints[i];
        let mut c = Controller::new(Tuning::default(), cp.spawn, cp.yaw);
        c.state = State::Ground;
        Sim { c, w: level.world(), level, events: vec![], t: 0.0, deaths: 0 }
    }

    pub(crate) fn run(&mut self, secs: f32, mut f: impl FnMut(&Controller, f32) -> Input) {
        let end = self.t + secs;
        while self.t < end {
            let i = f(&self.c, self.t);
            self.c.step(DT, &i, &self.w);
            for e in &self.c.events {
                if *e == Event::Death {
                    self.deaths += 1;
                }
            }
            self.events.extend(self.c.events.iter().copied());
            self.t += DT;
        }
    }

    pub(crate) fn has(&self, e: Event) -> bool {
        self.events.iter().any(|x| std::mem::discriminant(x) == std::mem::discriminant(&e))
    }

    pub(crate) fn checkpoint(&self) -> Option<&'static str> {
        self.level.checkpoint_at(self.c.feet).map(|i| self.level.checkpoints[i].name)
    }
}

/// Run forward while strafing toward `x`.
pub(crate) fn toward_x(c: &Controller, x: f32) -> Input {
    let s = ((x - c.feet.x) * 1.2).clamp(-0.6, 0.6);
    Input { move_axis: Vec2::new(s, 1.0), ..Default::default() }
}

/// Angling into a wall to wallrun it, the way you do in ME: once airborne, turn the view
/// `deg` toward the wall (positive = right), until the first wallrun starts. The game's
/// FindWallForward only finds a wall you're facing into.
#[derive(Default)]
pub(crate) struct AngleIn {
    base: Option<f32>,
    done: bool,
}

impl AngleIn {
    pub(crate) fn apply(&mut self, c: &Controller, i: &mut Input, deg: f32) {
        self.done |= matches!(c.state, State::WallRun { .. });
        if self.done || c.state != State::Air {
            return;
        }
        let base = *self.base.get_or_insert(c.yaw);
        i.look.x = (base - deg.to_radians() - c.yaw).clamp(-0.1, 0.1);
    }
}

pub(crate) fn once(flag: &mut bool, cond: bool) -> bool {
    let fire = cond && !*flag;
    *flag |= fire;
    fire
}

#[test]
fn section_a_rail_pipe_crate_gap() {
    let mut s = Sim::at_checkpoint(0);
    let (mut v, mut sl, mut m, mut g) = (false, false, false, false);
    s.run(9.0, |c, _| {
        let mut i = toward_x(c, 0.0);
        i.jump_pressed |= once(&mut v, c.feet.z < z::RAIL + 0.85);
        i.crouch_pressed |= once(&mut sl, c.feet.z < z::PIPE + 2.5);
        i.crouch_held = c.feet.z < z::PIPE + 2.5 && c.feet.z > z::PIPE - 0.6;
        i.jump_pressed |= once(&mut m, c.feet.z < z::CRATE + 0.8);
        i.jump_pressed |= once(&mut g, c.feet.z < z::ROOF_A_END + 0.5 && c.state == State::Ground);
        if c.feet.z < z::ROOF_B1_START - 2.0 {
            i = Input::default();
        }
        i
    });
    assert!(s.has(Event::Vault), "no vault: {:?}", s.events);
    assert!(s.has(Event::Slide), "no slide: {:?}", s.events);
    assert!(s.has(Event::Mantle) || s.has(Event::Vault), "no crate move: {:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("Roof B1"), "ended at {:?} {:?}", s.c.feet, s.events);
}

#[test]
fn section_b_wallrun_over_pit() {
    let mut s = Sim::at_checkpoint(1);
    let mut j = false;
    let mut angle = AngleIn::default();
    s.run(6.0, |c, _| {
        let mut i = toward_x(c, greybox::WALLRUN_X - 0.5);
        i.jump_pressed = once(&mut j, c.feet.z < z::ROOF_B1_END + 1.3);
        angle.apply(c, &mut i, 25.0);
        if c.feet.z < z::ROOF_B2_START - 3.0 && c.state == State::Ground {
            i = Input::default();
        }
        i
    });
    assert!(s.has(Event::WallRunStart), "{:?}", s.events);
    assert_eq!(s.deaths, 0, "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("Roof B2"), "ended at {:?}", s.c.feet);
    assert!((s.c.feet.y - ROOF_B_Y).abs() < 0.05);
}

#[test]
fn section_b2_climb_to_roof_c() {
    let mut s = Sim::at_checkpoint(2);
    let mut j = false;
    s.run(6.0, |c, _| {
        if c.feet.y > ROOF_C_Y - 0.1 && c.state == State::Ground {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::CLIMB_WALL + 1.2);
        i
    });
    assert!(s.has(Event::WallClimbStart), "{:?}", s.events);
    assert!(s.has(Event::PullUp) || s.has(Event::Mantle), "{:?}", s.events);
    assert!((s.c.feet.y - ROOF_C_Y).abs() < 0.05, "y = {}", s.c.feet.y);
}

#[test]
fn section_c_kick_to_catwalk() {
    let mut s = Sim::at_checkpoint(3);
    let (mut j, mut turn, mut kick) = (false, false, false);
    s.run(8.0, |c, _| {
        match c.state {
            State::WallClimb { t, .. } => {
                return Input { turn_pressed: once(&mut turn, t > 0.15), ..Default::default() };
            }
            State::WallClimbTurned { t, .. } => {
                return Input { jump_pressed: once(&mut kick, t > 0.12), ..Default::default() };
            }
            State::LedgeHang { .. } | State::Traverse(_) => {
                return Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() };
            }
            _ => {}
        }
        if kick {
            // Flying back toward the catwalk: hold forward (we're facing it now).
            if c.feet.y > 7.3 && c.state == State::Ground {
                return Input::default();
            }
            return Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() };
        }
        let mut i = toward_x(c, -3.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::KICK_WALL + 1.2);
        i
    });
    assert!(s.has(Event::WallKick), "{:?}", s.events);
    // Up onto it either way: catching the edge and pulling up, or (kicking from higher up
    // the wall) clearing the edge and mantling straight on.
    assert!(s.has(Event::LedgeGrab) || s.has(Event::Mantle), "{:?}", s.events);
    assert!((s.c.feet.y - 7.4).abs() < 0.05, "on catwalk? feet = {:?} events {:?}", s.c.feet, s.events);
}

#[test]
fn section_c_drop_needs_roll() {
    // Without rolling: hard landing.
    let mut s = Sim::at_checkpoint(3);
    s.run(6.0, |c, _| toward_x(c, 3.0));
    assert!(s.has(Event::HardLand), "{:?}", s.events);

    // With a roll: keep going.
    let mut s = Sim::at_checkpoint(3);
    s.run(6.0, |c, _| {
        let mut i = toward_x(c, 3.0);
        i.crouch_pressed = c.state == State::Air && c.feet.y < ROOF_D_Y + 1.0 && c.vel.y < 0.0;
        i
    });
    assert!(s.has(Event::Roll), "{:?}", s.events);
    assert!(!s.has(Event::HardLand), "{:?}", s.events);
    assert_eq!(s.checkpoint(), Some("Roof D"));
}

#[test]
fn section_d_stairs_to_finish() {
    let mut s = Sim::at_checkpoint(4);
    s.run(3.5, |c, _| {
        if c.feet.z < -121.0 {
            return Input::default();
        }
        toward_x(c, 7.0)
    });
    assert!(s.c.feet.y > ROOF_D_Y + 1.7, "top of stairs? {:?}", s.c.feet);
}

#[test]
fn checkpoints_are_on_solid_ground() {
    let level = greybox::greybox();
    let w = level.world();
    for cp in &level.checkpoints {
        let mut c = Controller::new(Tuning::default(), cp.spawn + Vec3::Y * 0.01, cp.yaw);
        for _ in 0..30 {
            c.step(DT, &Input::default(), &w);
        }
        assert_eq!(c.state, State::Ground, "{} spawn not grounded: {:?}", cp.name, c.feet);
        assert_eq!(level.checkpoint_at(c.feet).map(|i| level.checkpoints[i].name), Some(cp.name));
    }
}

#[test]
fn hangs_full_height_on_climb_wall() {
    // Decorations must never make the player hang crouched.
    let mut s = Sim::at_checkpoint(2);
    let mut j = false;
    let mut hung = None;
    s.run(4.0, |c, _| {
        if let State::LedgeHang { .. } = c.state {
            hung.get_or_insert((c.crouched, c.height()));
            return Input::default();
        }
        if matches!(c.state, State::WallClimb { .. }) {
            return Input::default();
        }
        let mut i = toward_x(c, 0.0);
        i.jump_pressed = once(&mut j, c.feet.z < z::CLIMB_WALL + 1.2);
        i
    });
    let (crouched, h) = hung.expect("never grabbed the ledge");
    assert!(!crouched, "hung crouched (height {h})");
}
