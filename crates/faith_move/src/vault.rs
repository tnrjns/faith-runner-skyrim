//! Vaults and step-ups: TdMove_SpeedVault / TdMove_VaultOver.
//!
//! The game picks one of six `VaultTypes` by how high the hand-plant is, whether you go onto
//! the obstacle or over it, your speed, your vertical speed and how soon you'd reach it. Each
//! type has its own animation and three timed phases: up to the hand-plant, over the ledge,
//! down to where you land. You come out at the speed it took to cover that last stretch.
//! It's checked every frame of a jump or fall while you hold forward (the Jump and Falling
//! moves' bCheckForVaultOver, with MoveActionHint "up"), so a vault is "jump at it with W held".

use glam::Vec3;

use crate::tuning::Tuning;
use crate::world::{column_top, tops_below, trace_wall, Body, World};

/// Unreal units to metres.
const fn uu(v: f32) -> f32 {
    v / 100.0
}

/// Faith's collision cylinder: CollisionRadius 30, CollisionHeight 90 (half height), so
/// MoveLedgeLocation.Z - Location.Z + CollisionHeight is the ledge height above the feet.
const RADIUS: f32 = uu(30.0);
const HALF_HEIGHT: f32 = uu(90.0);

/// One entry of TdMove_SpeedVault.VaultTypes (defaults from TdGame.u; heights and speeds in
/// Unreal units, -1 = no limit).
pub struct VaultType {
    pub anim: &'static str,
    pub onto: bool,
    pub min_height: f32,
    pub max_height: f32,
    pub min_speed_z: f32,
    pub max_speed_z: f32,
    pub max_momentum: f32,
    pub max_distance_time: f32,
    pub clamp_speed_min: f32,
    pub clamp_speed_max: f32,
    pub speed_addition: f32,
    pub time_up: f32,
    pub time_over: f32,
    pub time_down: f32,
    /// HandplantOffset / LedgeOffset: (along the move, up) from the ledge, for the body centre.
    pub handplant: (f32, f32),
    pub ledge: (f32, f32),
}

pub const VAULT_TYPES: [VaultType; 6] = [
    VaultType { anim: "autostepuprightleg", onto: true, min_height: 0.0, max_height: 48.0, min_speed_z: -600.0, max_speed_z: 0.0,
        max_momentum: -1.0, max_distance_time: 0.2, clamp_speed_min: 100.0, clamp_speed_max: 300.0, speed_addition: 0.0,
        time_up: 0.0, time_over: 0.3, time_down: 0.2, handplant: (0.0, 5.0), ledge: (0.0, 90.0) },
    VaultType { anim: "stepuprightleg88", onto: true, min_height: 48.0, max_height: 148.0, min_speed_z: 0.0, max_speed_z: 700.0,
        max_momentum: 200.0, max_distance_time: 0.4, clamp_speed_min: 200.0, clamp_speed_max: 700.0, speed_addition: 0.0,
        time_up: 0.0, time_over: 0.4, time_down: 0.25, handplant: (0.0, 0.0), ledge: (-20.0, 60.0) },
    VaultType { anim: "VaultOnto", onto: true, min_height: 64.0, max_height: 148.0, min_speed_z: 0.0, max_speed_z: 10000.0,
        max_momentum: -1.0, max_distance_time: 0.4, clamp_speed_min: 400.0, clamp_speed_max: 720.0, speed_addition: 80.0,
        time_up: 0.0, time_over: 0.35, time_down: 0.3, handplant: (0.0, 5.0), ledge: (0.0, 25.0) },
    VaultType { anim: "VaultOver", onto: false, min_height: 64.0, max_height: 148.0, min_speed_z: 0.0, max_speed_z: 10000.0,
        max_momentum: -1.0, max_distance_time: 0.4, clamp_speed_min: 400.0, clamp_speed_max: 720.0, speed_addition: 80.0,
        time_up: 0.0, time_over: 0.35, time_down: 0.3, handplant: (0.0, 5.0), ledge: (0.0, 25.0) },
    VaultType { anim: "VaultOverHigh", onto: false, min_height: 145.0, max_height: 192.0, min_speed_z: 50.0, max_speed_z: 10000.0,
        max_momentum: -1.0, max_distance_time: 0.4, clamp_speed_min: 200.0, clamp_speed_max: 400.0, speed_addition: 0.0,
        time_up: 0.28, time_over: 0.3, time_down: 0.45, handplant: (-40.0, -69.0), ledge: (0.0, 5.0) },
    VaultType { anim: "VaultOntoHigh", onto: true, min_height: 145.0, max_height: 192.0, min_speed_z: 50.0, max_speed_z: 10000.0,
        max_momentum: -1.0, max_distance_time: 0.4, clamp_speed_min: 200.0, clamp_speed_max: 400.0, speed_addition: 0.0,
        time_up: 0.27, time_over: 0.3, time_down: 0.6, handplant: (-65.0, -15.0), ledge: (0.0, 35.0) },
];

/// TdMove_SpeedVault.MaxTimeToLedge and VaultClearObjectHeight.
const MAX_TIME_TO_LEDGE: f32 = 0.4;
const CLEAR_OBJECT_HEIGHT: f32 = uu(35.0);

/// A vault in progress. Positions are feet positions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vault {
    /// Index into [`VAULT_TYPES`].
    pub kind: usize,
    /// Seconds since the vault started (the animation plays from 0 at rate 1).
    pub t: f32,
    pub from: Vec3,
    pub hand: Vec3,
    pub over: Vec3,
    pub end: Vec3,
    pub exit_vel: Vec3,
    /// Landing past the obstacle with nothing under you (bEndMoveFalling).
    pub falling: bool,
    /// Attack pressed during it (TdMove_SpeedVault.HandleMoveAction: bEndMoveInMelee; every
    /// vault type has bMeleePossible): the way down becomes TdMove_MeleeVault's kick.
    pub kick: bool,
    /// At the end before the kick's hit detection came on: she stays there, still, until it's
    /// over (TdMove_MeleeVault.ReachedPreciseLocation only restores the speed with bHitDetection).
    pub held: bool,
}

impl Vault {
    pub fn kind(&self) -> &'static VaultType {
        &VAULT_TYPES[self.kind]
    }

    pub fn anim(&self) -> &'static str {
        self.kind().anim
    }

    pub fn onto(&self) -> bool {
        self.kind().onto
    }

    pub fn duration(&self) -> f32 {
        let k = self.kind();
        k.time_up + k.time_over + k.time_down
    }

    /// Feet position `t` seconds in (UpdateVaultMovement's SetPreciseLocation legs).
    pub fn position(&self, t: f32) -> Vec3 {
        let k = self.kind();
        let (up, over, down) = (k.time_up, k.time_over, k.time_down);
        if t < up {
            self.from.lerp(self.hand, t / up)
        } else if t < up + over {
            let start = if up > 0.0 { self.hand } else { self.from };
            start.lerp(self.over, (t - up) / over)
        } else {
            self.over.lerp(self.end, ((t - up - over) / down).min(1.0))
        }
    }
}

fn horiz(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Sweep a box from `start` along `dir` for `dist` and return how far it gets before it runs
/// into something. Like the game's extent traces, anything it starts inside doesn't count; nor
/// does floor it only skims (the bottom 10 cm of the box is left out).
fn sweep(world: &dyn World, start: Vec3, dir: Vec3, dist: f32, half: Vec3) -> Option<f32> {
    let half = Vec3::new(half.x, (half.y - 0.05).max(0.01), half.z);
    let start = start + Vec3::Y * 0.05;
    world.sweep(half, start, dir * dist).map(|h| h.t * dist)
}

/// TdMove_SpeedVault.CanDoMove (with CheckCollision, FindValidOntoEndLocation,
/// FindValidOverEndLocation, FindVaultFloor and UpdateActiveVaultType) on the box world.
/// `view` is the full view direction (pitch included), `fwd` the facing on the ground plane.
pub fn plan(world: &dyn World, tu: &Tuning, body: Body, feet: Vec3, vel: Vec3, fwd: Vec3, view: Vec3) -> Option<Vault> {
    let speed = horiz(vel).length();
    // The ledge in front of you, close enough to reach in MaxTimeToLedge (at least 300 uu/s).
    let reach = MAX_TIME_TO_LEDGE * speed.max(uu(300.0));
    let mut hit = None;
    let mut y = 0.05;
    while y < uu(192.0) {
        if let Some(h) = trace_wall(world, body, feet, fwd, reach, y) {
            hit = Some((h, y));
            break;
        }
        y += 0.1;
    }
    let (hit, y) = hit?;
    let n = hit.n;
    // Facing it: vector(Controller.Rotation) . MoveNormal <= -0.2.
    if view.dot(n) > -0.2 {
        return None;
    }
    let inside = hit.at - n * 0.02;
    let top = column_top(world, inside, feet.y + y, feet.y + uu(192.0) + 0.05);
    let height = (top - feet.y) * 100.0;
    if !(0.0..=192.0).contains(&height) {
        return None;
    }
    let ledge = Vec3::new(hit.at.x, top, hit.at.z);
    let to_ledge = horiz(ledge - feet);
    let dir = to_ledge.normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    let time_to_handplant = to_ledge.length() / speed.max(uu(300.0));
    if time_to_handplant > MAX_TIME_TO_LEDGE {
        return None;
    }
    let stand = Body { half_width: tu.half_width, height: tu.stand_height };
    let mut end = ledge + dir * uu(48.0).max(speed * 0.3);

    // FindValidOntoEndLocation: room on top, at least 32 (low) or 64 units deep.
    let ext = Vec3::new(RADIUS * 1.4, HALF_HEIGHT * 0.5, RADIUS * 1.4);
    let onto_y = top + CLEAR_OBJECT_HEIGHT + ext.y;
    let span = horiz(end - ledge).length();
    let mut onto_end = Vec3::new(end.x, top, end.z);
    if let Some(s) = sweep(world, Vec3::new(ledge.x, onto_y, ledge.z), dir, span, ext) {
        let width = s + ext.x;
        if width < if height <= 48.0 { uu(32.0) } else { uu(64.0) } {
            return None;
        }
        onto_end = Vec3::new(ledge.x, top, ledge.z) + dir * (width - RADIUS);
    }
    // CheckCollision.
    end = Vec3::new(onto_end.x, end.y, onto_end.z);
    let onto_dist = horiz(onto_end - ledge).length().min(uu(160.0));
    let onto_end = Vec3::new(ledge.x, top, ledge.z) + dir * onto_dist;
    let max_ledge_width = (speed * 0.2).clamp(uu(60.0), uu(180.0));
    // FindValidOverEndLocation: from the far end back toward the ledge, just under its top.
    let over_ext = Vec3::new(RADIUS, uu(48.0), RADIUS);
    let over_y = top - over_ext.y;
    let back = horiz(end - ledge).length();
    let far = Vec3::new(ledge.x, over_y, ledge.z) + dir * back;
    let onto = match sweep(world, far, -dir, back, over_ext) {
        // Hit the obstacle's far side: over if that's within MaxLedgeWidth of the ledge.
        Some(d) => back - d >= max_ledge_width,
        None => true,
    };
    let mut onto = onto;
    let mut end = if onto { onto_end } else { Vec3::new(end.x, top, end.z) };
    // FindVaultFloor: a floor under the end, from 32 above the ledge down to 32 (onto) or
    // 240 (over) below it. Landing no more than 64 below the ledge counts as onto.
    let drop = if onto { uu(32.0) } else { uu(240.0) };
    let floor = tops_below(world, end, tu.half_width, top + uu(32.0), top - drop).first().copied();
    let mut falling = false;
    match floor {
        Some(f) => {
            onto = f > top - uu(64.0);
            end.y = f;
        }
        None => falling = !onto,
    }
    if !world.is_free(&stand.aabb(end + Vec3::Y * 0.01)) {
        return None;
    }
    // The way there is clear too: crouched, across the top from the lip and down to the end.
    // (Where the end is inside something solid, as in a rock, its overlap alone can't tell:
    // collision made of triangles has no inside, only surfaces.)
    let crouch_half = Vec3::new(tu.half_width * 0.9, tu.crouch_height * 0.5, tu.half_width * 0.9);
    let lip = Vec3::new(ledge.x, top + 0.05, ledge.z) + dir * 0.05 + Vec3::Y * crouch_half.y;
    let above = Vec3::new(end.x, lip.y, end.z);
    let across = above - lip;
    let down = Vec3::new(end.x, end.y + 0.02, end.z) + Vec3::Y * crouch_half.y - above;
    if world.sweep(crouch_half, lip, across).is_some() || (down.y < -0.01 && world.sweep(crouch_half, above, down).is_some()) {
        return None;
    }

    // UpdateActiveVaultType: the first type that fits.
    let momentum = speed * 100.0;
    let vz = vel.y * 100.0;
    let kind = VAULT_TYPES.iter().enumerate().position(|(i, t)| {
        (t.min_height..=t.max_height).contains(&height)
            && (t.onto == onto || i == 0)
            && (t.max_momentum < 0.0 || momentum <= t.max_momentum)
            && vz >= t.min_speed_z
            && vz <= t.max_speed_z
            && time_to_handplant <= t.max_distance_time
    })?;
    let k = &VAULT_TYPES[kind];
    // The step-up (type 0) takes either; going onto, it ends on top.
    if !onto && k.onto {
        end = onto_end;
    }
    let onto = k.onto;
    let clamped = uu((momentum + k.speed_addition).clamp(k.clamp_speed_min, k.clamp_speed_max));
    if onto {
        let d = horiz(end - ledge).length().min(uu(48.0).max(clamped * k.time_down));
        end = Vec3::new(ledge.x, end.y, ledge.z) + dir * d;
        if !falling && !world.is_free(&stand.aabb(end + Vec3::Y * 0.01)) {
            return None;
        }
    }

    let at = |(along, up): (f32, f32)| Vec3::new(ledge.x, top + uu(up) - HALF_HEIGHT, ledge.z) + dir * uu(along);
    let over = at(k.ledge);
    let exit_speed = horiz(end - over).length() / k.time_down;
    Some(Vault {
        kind,
        t: 0.0,
        from: feet,
        hand: at(k.handplant),
        over,
        end,
        exit_vel: dir * exit_speed,
        falling: falling && !onto,
        kick: false,
        held: false,
    })
}
