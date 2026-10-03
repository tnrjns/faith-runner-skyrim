//! AnimSet / AnimSequence reader for Mirror's Edge.
//!
//! Each sequence stores `CompressedTrackOffsets` (per track: translation
//! offset/count, rotation offset/count) into a `CompressedByteStream`:
//! - translations: 3 × f32 per key;
//! - one rotation key: 3 × f32 (W derived);
//! - several rotation keys: a 24-byte per-track header, then UE3 Fixed48NoW
//!   keys (3 × u16, x = (u − 32767) / 32767, W derived).
//!
//! Animation rotations use the opposite handedness convention from the mesh's
//! rest pose, so they're conjugated here: everything this module returns is in
//! the mesh's convention and can be mixed freely with `Bone::rotation`. The root
//! track (track 0) is the exception: UE3 stores the root bone's rotation the
//! other way round from its children, so it is taken as is. (Conjugated, every
//! clip's root yaw ran backwards: StandTurn180Right turned left.)

use std::collections::HashMap;

use crate::package::{read_name, Cursor, Package};
use crate::props::Props;
use crate::{Error, Result};

#[derive(Clone, Debug, Default)]
pub struct Track {
    pub translations: Vec<[f32; 3]>,
    pub rotations: Vec<[f32; 4]>,
}

/// A sound cue baked into an animation at a point in time.
#[derive(Clone, Debug, PartialEq)]
pub enum Notify {
    /// AnimNotify_Footstep: |n| is the sound type (1 sneak … 11 slide for
    /// feet, 21 soft … 25 fast release for hands), the sign the side.
    Footstep(i32),
    /// TdAnimNotify_CharacterSound: e.g. "ECSClothing_Run", "ECSOral_Strain_Soft".
    Character(String),
    /// AnimNotify_Sound: a specific cue, by full path.
    Sound(String),
}

#[derive(Clone, Debug)]
pub struct NotifyEvent {
    pub time: f32,
    pub notify: Notify,
}

#[derive(Clone, Debug)]
pub struct Sequence {
    pub name: String,
    /// Seconds.
    pub length: f32,
    pub frames: u32,
    pub rate: f32,
    /// One per bone, in `AnimSet::bones` order.
    pub tracks: Vec<Track>,
    /// Sound cues, sorted by time.
    pub notifies: Vec<NotifyEvent>,
}

#[derive(Clone, Debug)]
pub struct AnimSet {
    pub bones: Vec<String>,
    pub sequences: HashMap<String, Sequence>,
}

fn w_from_xyz(x: f32, y: f32, z: f32) -> f32 {
    (1.0 - x * x - y * y - z * z).max(0.0).sqrt()
}

impl AnimSet {
    /// The package's (first) AnimSet, with every sequence in the package.
    pub fn read(pkg: &Package) -> Result<Self> {
        let set = pkg
            .exports
            .iter()
            .find(|e| matches!(pkg.class_name(e), "AnimSet" | "TdAnimSet"))
            .ok_or_else(|| Error::Missing("AnimSet".into()))?;
        Self::read_set(pkg, set, false)
    }

    /// The AnimSet called `name`, with only its own sequences (a package can hold several sets,
    /// each with its own track order).
    pub fn read_named(pkg: &Package, name: &str) -> Result<Self> {
        let set = pkg
            .exports
            .iter()
            .find(|e| matches!(pkg.class_name(e), "AnimSet" | "TdAnimSet") && e.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| Error::Missing(format!("AnimSet {name}")))?;
        Self::read_set(pkg, set, true)
    }

    fn read_set(pkg: &Package, set: &crate::package::Export, own: bool) -> Result<Self> {
        let props = Props::read(pkg, set)?;
        let own_sequences: Vec<&crate::package::Export> = if own {
            props.object_array(pkg, "Sequences").into_iter().filter(|&o| o > 0).filter_map(|o| pkg.exports.get(o as usize - 1)).collect()
        } else {
            pkg.of_class("AnimSequence").collect()
        };
        let t = props.get("TrackBoneNames").ok_or_else(|| Error::Missing("TrackBoneNames".into()))?;
        let mut c = Cursor::new(&pkg.data, t.at);
        let n = c.count(8)?;
        let bones = (0..n).map(|_| read_name(&pkg.names, &mut c)).collect::<Result<Vec<_>>>()?;

        let mut sequences = HashMap::new();
        for e in own_sequences {
            let props = Props::read(pkg, e)?;
            let name = props.name(pkg, "SequenceName").unwrap_or_else(|| e.name.clone());
            let length = props.f32(pkg, "SequenceLength").unwrap_or(0.0);
            let frames = props.i32(pkg, "NumFrames").unwrap_or(1).max(1) as u32;
            let rate = props.f32(pkg, "RateScale").unwrap_or(1.0);
            // Faith's own clips are ACF_Fixed48NoW; the AI's are ACF_Float96NoW too. Anything else
            // is left out (not the whole set).
            let float96 = match props.name(pkg, "RotationCompressionFormat").as_deref() {
                None | Some("ACF_Fixed48NoW") => false,
                Some("ACF_Float96NoW") => true,
                Some(_) => continue,
            };
            let t = props
                .get("CompressedTrackOffsets")
                .ok_or_else(|| Error::Missing(format!("{name}: track offsets")))?;
            let mut c = Cursor::new(&pkg.data, t.at);
            let n = c.count(4)?;
            let offsets = (0..n).map(|_| c.i32()).collect::<Result<Vec<_>>>()?;
            let mut c = Cursor::new(&pkg.data, props.end);
            let len = c.count(1)?;
            let stream = c.bytes(len)?;

            let mut tracks = Vec::with_capacity(offsets.len() / 4);
            for (ti, o) in offsets.chunks_exact(4).enumerate() {
                let (to, tk, ro, rk) = (o[0] as usize, o[1] as usize, o[2] as usize, o[3] as usize);
                let s = if ti == 0 { -1.0 } else { 1.0 };
                let mut tr = Track::default();
                let mut c = Cursor::new(stream, to);
                for _ in 0..tk {
                    tr.translations.push(c.vec3()?);
                }
                let mut c = Cursor::new(stream, ro);
                if rk == 1 {
                    let [x, y, z] = c.vec3()?;
                    tr.rotations.push([-x * s, -y * s, -z * s, w_from_xyz(x, y, z)]);
                } else if rk > 1 && float96 {
                    c.skip(24)?;
                    for _ in 0..rk {
                        let [x, y, z] = c.vec3()?;
                        tr.rotations.push([-x * s, -y * s, -z * s, w_from_xyz(x, y, z)]);
                    }
                } else if rk > 1 {
                    c.skip(24)?;
                    for _ in 0..rk {
                        let x = (c.u16()? as f32 - 32767.0) / 32767.0;
                        let y = (c.u16()? as f32 - 32767.0) / 32767.0;
                        let z = (c.u16()? as f32 - 32767.0) / 32767.0;
                        tr.rotations.push([-x * s, -y * s, -z * s, w_from_xyz(x, y, z)]);
                    }
                }
                tracks.push(tr);
            }
            let mut notifies = vec![];
            for ev in props.struct_array(pkg, "Notifies") {
                let time = ev.f32(pkg, "Time").unwrap_or(0.0);
                let Some(obj) = ev.object(pkg, "Notify").filter(|&o| o > 0) else { continue };
                let ne = &pkg.exports[obj as usize - 1];
                let Ok(np) = Props::read(pkg, ne) else { continue };
                let notify = match pkg.class_name(ne) {
                    "AnimNotify_Footstep" => Notify::Footstep(np.i32(pkg, "FootDown").unwrap_or(0)),
                    "TdAnimNotify_CharacterSound" => match np.name(pkg, "TriggerType") {
                        Some(t) => Notify::Character(t),
                        None => continue,
                    },
                    "AnimNotify_Sound" => match np.object(pkg, "SoundCue").filter(|&o| o != 0) {
                        Some(o) => Notify::Sound(pkg.path(o)),
                        None => continue,
                    },
                    _ => continue,
                };
                notifies.push(NotifyEvent { time, notify });
            }
            notifies.sort_by(|a, b| a.time.total_cmp(&b.time));
            sequences.insert(name.clone(), Sequence { name, length, frames, rate, tracks, notifies });
        }
        Ok(AnimSet { bones, sequences })
    }

    /// Another set's sequences (those `keep` picks, by name), its tracks put in this set's bone
    /// order; a bone it has no track for keeps the rest pose. Ones already here stay.
    pub fn add_from(&mut self, other: &AnimSet, keep: impl Fn(&str) -> bool) {
        let from: Vec<Option<usize>> = self.bones.iter().map(|b| other.bones.iter().position(|o| o.eq_ignore_ascii_case(b))).collect();
        for (name, seq) in &other.sequences {
            if !keep(name) || self.sequences.contains_key(name) {
                continue;
            }
            let tracks = from.iter().map(|i| i.and_then(|i| seq.tracks.get(i).cloned()).unwrap_or_default()).collect();
            self.sequences.insert(name.clone(), Sequence { tracks, ..seq.clone() });
        }
    }
}
