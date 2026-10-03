//! Faith's sounds in the host game: the same cues the app plays (faith_anim::sound::Director),
//! read from the Mirror's Edge install and played on the default sound device.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use faith_anim::sound::{wanted_cues, CueInfo, Director, SoundCmd};
use me_assets::{FaithArms, Packages, SoundBank};
use rodio::{Decoder, MixerDeviceSink, Player};

/// Ogg bytes shared between plays.
#[derive(Clone)]
struct Bytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub(crate) struct Audio {
    pub director: Director,
    waves: HashMap<String, Vec<Bytes>>,
    device: MixerDeviceSink,
    /// Looping sounds playing, by the director's id, with their volume before `gain`.
    loops: HashMap<u64, (Player, f32)>,
    pub gain: f32,
    paused: bool,
}

impl Audio {
    pub fn load(arms: &FaithArms) -> Result<Audio, String> {
        let mut bank = SoundBank::new(Packages::new(&arms.cooked_pc));
        let mut infos = HashMap::new();
        let mut waves = HashMap::new();
        for path in wanted_cues(Some(arms)) {
            let key = path.to_ascii_lowercase();
            if infos.contains_key(&key) {
                continue;
            }
            let Some(cue) = bank.cue(&path) else { continue };
            let w: Vec<Bytes> = cue.waves.iter().map(|w| Bytes(Arc::new(w.ogg.as_ref().clone()))).collect();
            infos.insert(key.clone(), CueInfo { variants: w.len(), volume: cue.volume, pitch: cue.pitch, looping: cue.looping });
            waves.insert(key, w);
        }
        if infos.is_empty() {
            return Err("no Mirror's Edge sound packages".into());
        }
        let mut device = rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| format!("no sound device: {e}"))?;
        device.log_on_drop(false);
        Ok(Audio { director: Director::new(infos), waves, device, loops: HashMap::new(), gain: 0.8, paused: false })
    }

    pub fn cues(&self) -> usize {
        self.waves.len()
    }

    pub fn run(&mut self, cmd: SoundCmd) {
        match cmd {
            SoundCmd::Play { id, cue, variant, volume, speed, looping } => {
                let Some(bytes) = self.waves.get(&cue).and_then(|w| w.get(variant)).cloned() else { return };
                let player = Player::connect_new(self.device.mixer());
                player.set_volume(volume * self.gain);
                player.set_speed(speed);
                if looping {
                    let Ok(source) = Decoder::new_looped(Cursor::new(bytes)) else { return };
                    player.append(source);
                    if self.paused {
                        player.pause();
                    }
                    self.loops.insert(id, (player, volume));
                } else {
                    if self.paused {
                        return;
                    }
                    let Ok(source) = Decoder::new(Cursor::new(bytes)) else { return };
                    player.append(source);
                    player.detach();
                }
            }
            SoundCmd::Stop(id) => {
                if let Some((p, _)) = self.loops.remove(&id) {
                    p.stop();
                }
            }
            SoundCmd::Volume(id, v) => {
                if let Some((p, vol)) = self.loops.get_mut(&id) {
                    *vol = v;
                    p.set_volume(v * self.gain);
                }
            }
        }
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain.max(0.0);
        for (p, v) in self.loops.values() {
            p.set_volume(v * self.gain);
        }
    }

    /// Silence the loops (menus, Faith switched off) or let them play again.
    pub fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        for (p, _) in self.loops.values() {
            if paused {
                p.pause();
            } else {
                p.play();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cue Faith can play decodes with the decoder the plugin plays them through.
    #[test]
    fn every_cue_decodes() {
        let Some(dir) = std::env::var_os("ME_INSTALL") else { return };
        let arms = FaithArms::load(std::path::Path::new(&dir), 4).unwrap();
        let mut bank = SoundBank::new(Packages::new(&arms.cooked_pc));
        let (mut cues, mut waves) = (0, 0);
        for path in wanted_cues(Some(&arms)) {
            let Some(cue) = bank.cue(&path) else { continue };
            cues += 1;
            for w in &cue.waves {
                let bytes = Bytes(Arc::new(w.ogg.as_ref().clone()));
                let d = Decoder::new(Cursor::new(bytes)).unwrap_or_else(|e| panic!("{path}: {e}"));
                assert!(d.count() > 100, "{path}: no samples");
                waves += 1;
            }
        }
        assert!(cues > 60 && waves > cues, "{cues} cues, {waves} waves");
    }
}
