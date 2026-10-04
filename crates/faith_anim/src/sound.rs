//! Which of Mirror's Edge's sounds play when, engine independent: the app plays what this
//! decides (A_Material_Footstep, A_Material_Handstep,
//! A_Character_Female_01, A_Character_Effects, A_Bodyfalls).
//!
//! Most sounds are fired by the animations themselves: Faith's first-person animations carry
//! the game's footstep, handstep, cloth and breath cues at the exact frame (e.g. `runfwd` has
//! a run step at 0.02 s and 0.37 s). Landings, rolls, slides and the rush of wind at speed are
//! driven from gameplay.

use std::collections::HashMap;

use faith_move::greybox::Surface;
use faith_move::{Controller, Event as MoveEvent, State as MoveState};
use me_assets::FaithArms;
use me_assets::anim::Notify;

const FOOT: [(i32, &str); 11] = [
    (1, "Sneak"),
    (2, "Walk"),
    (3, "Run"),
    (4, "Sprint"),
    (5, "SprintRelease"),
    (6, "WallRun"),
    (7, "WallrunRelease"),
    (8, "LandSoft"),
    (9, "LandMedium"),
    (10, "LandHard"),
    (11, "Slide"),
];
const HAND: [(i32, &str); 5] = [(21, "Soft"), (22, "Medium"), (23, "Hard"), (24, "SlowRelease"), (25, "FastRelease")];

const SURFACES: [Surface; 11] = [
    Surface::Concrete,
    Surface::Metal,
    Surface::MetalPipe,
    Surface::Airduct,
    Surface::Wood,
    Surface::MetalGantry,
    Surface::MetalLadder,
    Surface::MetalFence,
    Surface::Cardboard,
    Surface::Water,
    Surface::Glass,
];

pub const RUN_WIND: &str = "A_Character_Effects.Movement.RunWind";

/// A blow landing (TdPawn.PlayMeleeImpact): the struck material's TdPhysicalMaterialMelee
/// ImpactSoundFist / ImpactSoundFoot; a person's are TDPhysicalMaterials' _Body and _Head sets.
/// Punches and the wallrun kick (which aims at the eyes) land on the head, the rest on the body.
pub fn impact_cue(kind: faith_move::MeleeKind) -> &'static str {
    use faith_move::MeleeKind::*;
    match kind {
        Punch => "A_Character_Melee.A_Female.Fist_Head",
        Crouch => "A_Character_Melee.A_Female.Fist_Body",
        AirKick | SlideKick | VaultKick => "A_Character_Melee.A_Female.Foot_Body",
        WallRunKick => "A_Character_Melee.A_Female.Foot_Head",
    }
}

/// A door barged or kicked open (the training doors: the game's interactive door hit).
pub const DOOR_HIT: &str = "A_Props_Interactive.Doors.Door_Hit";

/// Footstep (1–11) or handstep (21–25) cue for a surface. The game names
/// these A_Material_Footstep.<Material>._03_Female_FootStepRun and so on.
pub fn step_cue_on(n: i32, surface: Surface) -> Option<String> {
    let n = n.abs();
    if let Some((_, name)) = FOOT.iter().find(|(k, _)| *k == n) {
        // The footstep sets in A_Material_Footstep (no fence or pipe sets: plain metal).
        let group = match surface {
            Surface::Concrete => "Concrete",
            Surface::Metal | Surface::MetalPipe | Surface::MetalFence => "Metal",
            Surface::Airduct => "Metal_Airduct",
            Surface::Wood => "Wood",
            Surface::MetalGantry => "MetalGantry",
            Surface::MetalLadder => "Metal_Ladder",
            Surface::Cardboard => "Cardboard",
            Surface::Water => "Water",
            Surface::Glass => "Glass",
        };
        return Some(format!("A_Material_Footstep.{group}._{n:02}_Female_FootStep{name}"));
    }
    // The handstep sets in A_Material_Handstep (none for wood, cardboard, water or glass:
    // concrete, as the step falls back to).
    let group = match surface {
        Surface::Concrete | Surface::Wood | Surface::Cardboard | Surface::Water | Surface::Glass => "Concrete",
        Surface::Metal | Surface::MetalGantry => "Metal",
        Surface::MetalPipe => "Metal_Pipe_Thin",
        Surface::Airduct => "Metal_Airduct",
        Surface::MetalLadder => "Metal_Ladder",
        Surface::MetalFence => "Metal_Fence",
    };
    HAND.iter().find(|(k, _)| *k == n).map(|(_, name)| format!("A_Material_Handstep.{group}._{n}_Female_HandStep{name}"))
}

fn step_cue(n: i32) -> Option<String> {
    step_cue_on(n, Surface::Concrete)
}

/// TdAnimNotify_CharacterSound trigger type → cue.
pub fn character_cue(t: &str) -> Option<String> {
    let f = "A_Character_Female_01";
    let t = t.strip_prefix("ECS")?;
    Some(match t {
        "Clothing_Run" => format!("{f}.Cloth.Run"),
        "Clothing_Walk" => format!("{f}.Cloth.Walk"),
        "Clothing_Crouch" => format!("{f}.Cloth.Crouch"),
        "Misc_Vault" => "A_Character_Effects.Movement.Vault".into(),
        _ => {
            if let Some(level) = t.strip_prefix("Oral_Strain_") {
                format!("{f}.Oral_Strain.{level}")
            } else if let Some(level) = t.strip_prefix("Oral_Impact_") {
                format!("{f}.Oral_Impact.{level}")
            } else if let Some(rest) = t.strip_prefix("Breath_") {
                // e.g. Breath_Medium_Short → Breath_Medium.Breath_Medium_Short_Out
                let (level, len) = rest.split_once('_')?;
                format!("{f}.Breath_{level}.Breath_{level}_{len}_Out")
            } else {
                return None;
            }
        }
    })
}

/// Every cue Faith's movement can play: steps on every surface, her voice and body, the run
/// wind, and every specific cue the animations name (hand claps, pipe grabs…).
pub fn wanted_cues(arms: Option<&FaithArms>) -> Vec<String> {
    let mut wanted: Vec<String> = vec![];
    for (n, _) in FOOT.iter().chain(HAND.iter()) {
        for surface in SURFACES {
            wanted.extend(step_cue_on(*n, surface));
        }
    }
    for t in [
        "ECSClothing_Run",
        "ECSClothing_Walk",
        "ECSClothing_Crouch",
        "ECSMisc_Vault",
        "ECSOral_Strain_Soft",
        "ECSOral_Strain_Medium",
        "ECSOral_Strain_Hard",
        "ECSOral_Impact_Soft",
        "ECSOral_Impact_Medium",
        "ECSOral_Impact_Hard",
    ] {
        wanted.extend(character_cue(t));
    }
    for level in ["Soft", "Medium", "Hard"] {
        for len in ["Short", "Long"] {
            for dir in ["In", "Out"] {
                wanted.push(format!("A_Character_Female_01.Breath_{level}.Breath_{level}_{len}_{dir}"));
            }
        }
    }
    for c in [
        "A_Character_Female_01.Body.Roll",
        "A_Character_Female_01.Body.RollCloth",
        "A_Character_Female_01.Body.BodySlide",
        "A_Character_Female_01.Body.BodyFall",
        RUN_WIND,
        DOOR_HIT,
    ] {
        wanted.push(c.into());
    }
    for kind in [faith_move::MeleeKind::Punch, faith_move::MeleeKind::Crouch, faith_move::MeleeKind::AirKick, faith_move::MeleeKind::WallRunKick] {
        wanted.push(impact_cue(kind).into());
    }
    if let Some(a) = arms {
        let mut cues: Vec<String> = a
            .anims
            .sequences
            .values()
            .flat_map(|s| s.notifies.iter())
            .filter_map(|n| match &n.notify {
                Notify::Sound(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        cues.sort();
        cues.dedup();
        wanted.extend(cues);
    }
    wanted
}

/// A loaded cue: how many variations, its random volume and pitch ranges, whether it loops.
#[derive(Clone, Copy, Debug)]
pub struct CueInfo {
    pub variants: usize,
    pub volume: (f32, f32),
    pub pitch: (f32, f32),
    pub looping: bool,
}

/// What to do with the sound output. Volumes are before the player's own volume setting.
#[derive(Clone, Debug, PartialEq)]
pub enum SoundCmd {
    /// Play variation `variant` of `cue` (lower case) at `volume`, `speed` times as fast.
    /// Looping sounds play until stopped; `id` names them.
    Play { id: u64, cue: String, variant: usize, volume: f32, speed: f32, looping: bool },
    Stop(u64),
    Volume(u64, f32),
}

/// Decides Faith's sounds from her movement and animation, frame by frame.
pub struct Director {
    cues: HashMap<String, CueInfo>,
    rng: u64,
    next_id: u64,
    last_phase: f32,
    /// Looping cues started by a move (the slide scrape is one): in the game the move's code
    /// stops them when it ends, so they're stopped here when the movement state changes.
    move_loops: Vec<(u64, std::mem::Discriminant<MoveState>)>,
    run_wind: Option<(u64, f32)>,
    /// Seconds since the last landing played: ground that jitters under her (something loose
    /// she's standing on) can't fire one a frame.
    since_land: f32,
}

impl Director {
    /// `cues`: what's loaded (lower-case path → info).
    pub fn new(cues: HashMap<String, CueInfo>) -> Self {
        Director { cues, rng: 0x9E37_79B9_7F4A_7C15, next_id: 1, last_phase: 0.0, move_loops: vec![], run_wind: None, since_land: 1.0 }
    }

    pub fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    pub fn has(&self, cue: &str) -> bool {
        self.cues.contains_key(&cue.to_ascii_lowercase())
    }

    /// A Play for `cue` at `gain` times its own (random) volume, or None if it isn't loaded.
    pub fn play(&mut self, cue: &str, gain: f32) -> Option<SoundCmd> {
        let key = cue.to_ascii_lowercase();
        let c = *self.cues.get(&key)?;
        if c.variants == 0 {
            return None;
        }
        let variant = ((self.rand() * c.variants as f32) as usize).min(c.variants - 1);
        let v = c.volume.0 + (c.volume.1 - c.volume.0) * self.rand();
        let p = c.pitch.0 + (c.pitch.1 - c.pitch.0) * self.rand();
        let id = self.next_id;
        self.next_id += 1;
        Some(SoundCmd::Play { id, cue: key, variant, volume: (v * gain).max(0.0), speed: p.clamp(0.5, 2.0), looping: c.looping })
    }

    /// Play a cue for the current move; a looping cue is stopped when the move ends instead of
    /// playing forever.
    fn play_move(&mut self, cue: &str, gain: f32, state: &MoveState, out: &mut Vec<SoundCmd>) {
        if let Some(cmd) = self.play(cue, gain) {
            if let SoundCmd::Play { id, looping: true, .. } = cmd {
                self.move_loops.push((id, std::mem::discriminant(state)));
            }
            out.push(cmd);
        }
    }

    /// A foot/hand step on a surface, falling back to concrete where the game has no variant
    /// (airducts have no wallrun steps, say).
    fn step(&mut self, n: i32, surface: Surface, gain: f32, state: &MoveState, out: &mut Vec<SoundCmd>) {
        let cue = step_cue_on(n, surface).filter(|c| self.has(c)).or_else(|| step_cue(n));
        if let Some(cue) = cue {
            self.play_move(&cue, gain, state, out);
        }
    }

    /// One frame. `notifies`: the cues the animations hit this frame (with `anim_driven`); without
    /// animation, footsteps come from the head-bob `step_phase`. `surface(hand)` is what the
    /// feet (or hands) touch.
    pub fn update(
        &mut self,
        dt: f32,
        c: &Controller,
        notifies: &[Notify],
        anim_driven: bool,
        step_phase: f32,
        surface: &dyn Fn(bool) -> Surface,
        out: &mut Vec<SoundCmd>,
    ) {
        let speed = c.horizontal_speed();
        self.since_land += dt;

        // ---- the rushing wind loop, started silent
        if self.run_wind.is_none() {
            if let Some(cmd) = self.play(RUN_WIND, 0.0) {
                if let SoundCmd::Play { id, .. } = cmd {
                    self.run_wind = Some((id, 0.0));
                }
                out.push(cmd);
            }
        }

        // ---- stop move loops (slide scrape…) once the move is over
        let now = std::mem::discriminant(&c.state);
        self.move_loops.retain(|(id, kind)| {
            if *kind == now {
                return true;
            }
            out.push(SoundCmd::Stop(*id));
            false
        });

        // ---- cues baked into the animations
        if anim_driven {
            for n in notifies {
                match n {
                    Notify::Footstep(f) => {
                        let gain = if f.abs() >= 21 { 0.8 } else { 1.0 };
                        self.step(*f, surface(f.abs() >= 21), gain, &c.state, out);
                    }
                    Notify::Character(t) => {
                        if let Some(cue) = character_cue(t) {
                            let gain = if t.starts_with("ECSClothing") { 0.5 } else { 0.8 };
                            self.play_move(&cue, gain, &c.state, out);
                        }
                    }
                    Notify::Sound(path) => self.play_move(path, 0.8, &c.state, out),
                }
            }
        } else if matches!(c.state, MoveState::Ground) && speed > 0.5 {
            // No animation: footsteps from the head-bob step phase.
            let crossed = |a: f32, b: f32, at: f32| if b >= a { a < at && at <= b } else { at > a || at <= b };
            if crossed(self.last_phase, step_phase, 0.0001) || crossed(self.last_phase, step_phase, std::f32::consts::PI) {
                let n = if speed > c.tuning.run_speed + 0.5 { 4 } else if speed > 2.5 { 3 } else { 2 };
                self.step(n, surface(false), 1.0, &c.state, out);
            }
        }
        self.last_phase = step_phase;

        // ---- gameplay events
        for e in c.events.clone() {
            match e {
                MoveEvent::Land { .. } if self.since_land < 0.15 => {}
                MoveEvent::Land { impact, .. } => {
                    self.since_land = 0.0;
                    // Landing animations don't carry their own impact sound.
                    let n = if impact > 9.0 { 10 } else if impact > 6.0 { 9 } else { 8 };
                    self.step(n, surface(false), 1.0, &c.state, out);
                    if impact > 8.0 {
                        self.play_move("A_Character_Female_01.Oral_Impact.Soft", 0.7, &c.state, out);
                    }
                }
                MoveEvent::HardLand => {
                    for cue in ["A_Character_Female_01.Body.BodyFall", "A_Character_Female_01.Oral_Impact.Hard"] {
                        self.play_move(cue, 1.0, &c.state, out);
                    }
                    self.step(10, surface(false), 1.0, &c.state, out);
                }
                MoveEvent::Roll => {
                    for cue in ["A_Character_Female_01.Body.Roll", "A_Character_Female_01.Body.RollCloth"] {
                        self.play_move(cue, 1.0, &c.state, out);
                    }
                    self.step(9, surface(false), 1.0, &c.state, out);
                }
                // TdMove_RumpSlide: the slide's scrape, as a slide.
                MoveEvent::Slide | MoveEvent::RumpSlide => {
                    self.play_move("A_Character_Female_01.Body.BodySlide", 0.9, &c.state, out);
                    self.step(11, surface(false), 1.0, &c.state, out);
                }
                MoveEvent::Vault | MoveEvent::Mantle if !anim_driven => {
                    self.play_move("A_Character_Effects.Movement.Vault", 0.8, &c.state, out);
                }
                MoveEvent::LedgeGrab if !anim_driven => self.step(23, surface(true), 1.0, &c.state, out),
                // With the game's animations playing, these moves' sounds come from the
                // animations' own notifies (handsteps on the bar, the swing release, melee
                // strains...). Without them, a rough stand-in.
                MoveEvent::SpringBoard if !anim_driven => self.step(4, surface(false), 1.0, &c.state, out),
                MoveEvent::SwingStart | MoveEvent::ZipStart if !anim_driven => self.step(23, Surface::MetalPipe, 1.0, &c.state, out),
                MoveEvent::SwingJump | MoveEvent::ZipEnd { .. } if !anim_driven => self.step(25, Surface::MetalPipe, 0.9, &c.state, out),
                MoveEvent::BalanceFall | MoveEvent::Melee { .. } if !anim_driven => {
                    self.play_move("A_Character_Female_01.Oral_Strain.Medium", 0.7, &c.state, out);
                }
                MoveEvent::MeleeHit { kind, .. } => self.play_move(impact_cue(kind), 1.0, &c.state, out),
                MoveEvent::DoorOpened { .. } => self.play_move(DOOR_HIT, 1.0, &c.state, out),
                MoveEvent::Jump | MoveEvent::WallJump | MoveEvent::WallKick | MoveEvent::Dodge { .. } => {
                    if self.rand() < 0.5 {
                        self.play_move("A_Character_Female_01.Oral_Strain.Soft", 0.6, &c.state, out);
                    }
                }
                _ => {}
            }
        }

        // Breathing comes only from the animations' ECSBreath notifies, as in the game: its
        // native breathing log (TdPawn.AddBreathingLog) is never called, and there is no breath
        // timer.

        // ---- rushing wind at speed / falling
        let rush = ((speed - 4.5) / 3.5).clamp(0.0, 1.0).max(((-c.vel.y - 6.0) / 10.0).clamp(0.0, 1.0));
        if let Some((id, vol)) = &mut self.run_wind {
            let target = rush * 0.7;
            *vol += (target - *vol) * (1.0 - (-6.0 * dt).exp());
            out.push(SoundCmd::Volume(*id, *vol));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faith_move::{greybox, Input, Tuning};
    use glam::Vec2;

    fn cues() -> HashMap<String, CueInfo> {
        let info = |looping| CueInfo { variants: 2, volume: (0.8, 1.0), pitch: (0.95, 1.05), looping };
        let mut m = HashMap::new();
        for c in wanted_cues(None) {
            m.insert(c.to_ascii_lowercase(), info(c == RUN_WIND));
        }
        m
    }

    /// Running without animation steps on concrete, the wind loop starts silent and rises with
    /// speed, and a jump's landing plays a landing step.
    #[test]
    fn steps_wind_and_landing() {
        let level = greybox::greybox();
        let world = level.world();
        let cp = &level.checkpoints[0];
        let mut c = Controller::new(Tuning::default(), cp.spawn, cp.yaw);
        let mut d = Director::new(cues());
        let mut fx = faith_move::CameraFx::default();
        let mut all = vec![];
        let dt = 1.0 / 60.0;
        for i in 0..240 {
            let input = Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: i == 150, ..Default::default() };
            c.step(dt, &input, &world);
            let shot = fx.update(dt, &c, &input);
            let mut out = vec![];
            d.update(dt, &c, &[], false, shot.step_phase, &|_| Surface::Concrete, &mut out);
            all.extend(out);
        }
        let played: Vec<&str> = all.iter().filter_map(|c| if let SoundCmd::Play { cue, .. } = c { Some(cue.as_str()) } else { None }).collect();
        assert_eq!(played[0], RUN_WIND.to_ascii_lowercase(), "the wind loop first");
        assert!(played.iter().filter(|c| c.contains("footstep") && (c.contains("run") || c.contains("sprint"))).count() >= 4, "{played:?}");
        assert!(played.iter().any(|c| c.contains("landsoft") || c.contains("landmedium") || c.contains("landhard")), "{played:?}");
        let wind = all.iter().filter_map(|c| if let SoundCmd::Volume(_, v) = c { Some(*v) } else { None }).fold(0.0f32, f32::max);
        assert!(wind > 0.05, "wind rose to {wind}");
    }

    /// A blow that lands plays the game's impact (a punch the fist on the head, a slide kick the
    /// foot on the body), and a door knocked open its hit.
    #[test]
    fn hits_and_doors_sound() {
        let level = greybox::greybox();
        let cp = &level.checkpoints[0];
        let mut c = Controller::new(Tuning::default(), cp.spawn, cp.yaw);
        let mut d = Director::new(cues());
        let mut played = vec![];
        for kind in [faith_move::MeleeKind::Punch, faith_move::MeleeKind::SlideKick] {
            c.events = vec![MoveEvent::MeleeHit { target: 1, damage: 30.0, momentum: glam::Vec3::ZERO, kind }];
            let mut out = vec![];
            d.update(1.0 / 60.0, &c, &[], true, 0.0, &|_| Surface::Concrete, &mut out);
            played.extend(out.into_iter().filter_map(|c| if let SoundCmd::Play { cue, .. } = c { Some(cue) } else { None }));
        }
        c.events = vec![MoveEvent::DoorOpened { door: 0 }];
        let mut out = vec![];
        d.update(1.0 / 60.0, &c, &[], true, 0.0, &|_| Surface::Concrete, &mut out);
        played.extend(out.into_iter().filter_map(|c| if let SoundCmd::Play { cue, .. } = c { Some(cue) } else { None }));
        for want in ["a_character_melee.a_female.fist_head", "a_character_melee.a_female.foot_body", "a_props_interactive.doors.door_hit"] {
            assert!(played.iter().any(|p| p == want), "{want} in {played:?}");
        }
    }
}
