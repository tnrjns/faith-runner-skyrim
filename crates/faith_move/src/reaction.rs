//! Mirror's Edge's Reaction Time (TdPlayerController.UpdateReactionTime / AttemptReactionTime):
//! running fills a meter; full, a key slows the game down.
//!
//! - **Filling** (not active, below 100): ReactionTimeEnergyBuildRate (medium difficulty,
//!   0.005) x the frame x her speed (uu/s). It starts at ReactionTimeSpawnLevel (99.9).
//! - **Starting** (AttemptReactionTime): only full, and only at normal speed.
//! - **While on**: it drains 100 / ReactionTimeDrain (8 s) a real second; the game's speed is
//!   eased from 1 down to ReactionTimeMaxEffect (0.25) while the meter is above
//!   ReactionTimeFadeIn (90), held there, then eased back up below ReactionTimeFadeOut (20); at
//!   0 it's over (SetGameSpeed(1)).

const BUILD_RATE: f32 = 0.005;
const SPAWN_LEVEL: f32 = 99.9;
const DRAIN_TIME: f32 = 8.0;
const MAX_EFFECT: f32 = 0.25;
const FADE_IN: f32 = 90.0;
const FADE_OUT: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReactionTime {
    /// The meter, 0..100.
    pub energy: f32,
    pub active: bool,
    /// The game's speed (WorldInfo.TimeDilation): 1 normally.
    pub game_speed: f32,
}

impl Default for ReactionTime {
    fn default() -> Self {
        ReactionTime { energy: SPAWN_LEVEL, active: false, game_speed: 1.0 }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl ReactionTime {
    /// AttemptReactionTime: true if it started.
    pub fn attempt(&mut self) -> bool {
        if self.active || self.energy < 100.0 || self.game_speed < 1.0 {
            return false;
        }
        self.active = true;
        true
    }

    /// One frame: `dt` game seconds (already slowed by `game_speed`), `speed` her speed in m/s.
    /// Returns the game speed to use from now.
    pub fn update(&mut self, dt: f32, speed: f32) -> f32 {
        if self.active {
            let real = dt / self.game_speed.max(1e-3);
            self.energy -= 100.0 / DRAIN_TIME * real;
            if self.energy <= 0.0 {
                self.active = false;
                self.energy = 0.0;
                self.game_speed = 1.0;
            } else if self.energy > FADE_IN {
                self.game_speed = lerp(MAX_EFFECT, 1.0, (self.energy - FADE_IN) / (100.0 - FADE_IN));
            } else if self.energy < FADE_OUT {
                self.game_speed = lerp(1.0, MAX_EFFECT, self.energy / FADE_OUT);
            } else {
                self.game_speed = MAX_EFFECT;
            }
        } else if self.energy < 100.0 {
            self.energy = (self.energy + BUILD_RATE * dt * speed * 100.0).min(100.0);
        }
        self.game_speed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_running_then_slows_the_game_for_eight_seconds() {
        let mut r = ReactionTime { energy: 0.0, ..Default::default() };
        assert!(!r.attempt());
        // Running at 6 m/s fills it in 100 / (0.005 x 600) = 33 s.
        let mut t: f32 = 0.0;
        while r.energy < 100.0 {
            r.update(1.0 / 60.0, 6.0);
            t += 1.0 / 60.0;
        }
        assert!((t - 33.3).abs() < 0.2, "{t}");
        assert!(r.attempt());
        let mut real: f32 = 0.0;
        let mut slowest: f32 = 1.0;
        while r.active {
            let dt = 1.0 / 60.0 * r.game_speed;
            slowest = slowest.min(r.update(dt, 6.0));
            real += 1.0 / 60.0;
        }
        assert!((real - 8.0).abs() < 0.1, "{real}");
        assert!((slowest - 0.25).abs() < 1e-3);
        assert_eq!(r.game_speed, 1.0);
    }
}
