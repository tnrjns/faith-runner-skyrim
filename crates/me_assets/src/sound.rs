//! SoundCue / SoundNodeWave reader.
//!
//! A cue is a small tree: mix group → relative position → attenuation →
//! modulator (random volume/pitch) → random → waves. For first-person
//! playback all that matters is the set of waves to pick from, the
//! modulator's ranges, the cue's volume multiplier, and whether it loops.
//! Wave audio on PC is Ogg Vorbis, stored inline as `CompressedPCData`.

use std::collections::HashMap;
use std::rc::Rc;

use crate::package::{decompress_chunked, Cursor, Package};
use crate::props::Props;
use crate::{Packages, Result};

#[derive(Clone, Debug)]
pub struct Wave {
    pub name: String,
    /// Ogg Vorbis bytes.
    pub ogg: Rc<Vec<u8>>,
    pub duration: f32,
}

#[derive(Clone, Debug)]
pub struct Cue {
    pub path: String,
    pub waves: Vec<Wave>,
    pub volume: (f32, f32),
    pub pitch: (f32, f32),
    pub looping: bool,
}

/// Loads cues by full path ("A_Material_Footstep.Concrete._03_Female_FootStepRun")
/// from whichever packages are installed, caching results (including misses).
pub struct SoundBank {
    pkgs: Packages,
    cues: HashMap<String, Option<Rc<Cue>>>,
}

impl SoundBank {
    pub fn new(pkgs: Packages) -> Self {
        SoundBank { pkgs, cues: HashMap::new() }
    }

    pub fn cue(&mut self, path: &str) -> Option<Rc<Cue>> {
        let key = path.to_ascii_lowercase();
        if let Some(c) = self.cues.get(&key) {
            return c.clone();
        }
        let c = self.load(path).ok().flatten().map(Rc::new);
        self.cues.insert(key, c.clone());
        c
    }

    fn load(&mut self, path: &str) -> Result<Option<Cue>> {
        let Some((pkg_name, rest)) = path.split_once('.') else { return Ok(None) };
        if !self.pkgs.has(pkg_name) {
            return Ok(None);
        }
        let pkg = self.pkgs.get(pkg_name)?;
        let Some(idx) = pkg.find_path("SoundCue", rest) else { return Ok(None) };
        let props = Props::read(&pkg, &pkg.exports[idx])?;
        let mult = props.f32(&pkg, "VolumeMultiplier").unwrap_or(1.0);
        let pmult = props.f32(&pkg, "PitchMultiplier").unwrap_or(1.0);
        let mut cue = Cue { path: path.to_string(), waves: vec![], volume: (mult, mult), pitch: (pmult, pmult), looping: false };
        if let Some(first) = props.object(&pkg, "FirstNode") {
            walk(&pkg, first, &mut cue, 0)?;
        }
        Ok(if cue.waves.is_empty() { None } else { Some(cue) })
    }
}

fn walk(pkg: &Rc<Package>, object: i32, cue: &mut Cue, depth: u32) -> Result<()> {
    if object <= 0 || depth > 16 {
        return Ok(());
    }
    let e = &pkg.exports[object as usize - 1];
    let props = Props::read(pkg, e)?;
    match pkg.class_name(e) {
        "SoundNodeWave" => {
            if let Some(ogg) = wave_ogg(pkg, props.end, e.offset + e.size)? {
                let duration = props.f32(pkg, "Duration").unwrap_or(0.0);
                cue.waves.push(Wave { name: e.name.clone(), ogg: Rc::new(ogg), duration });
            }
            return Ok(());
        }
        "SoundNodeModulator" => {
            if let Some((lo, hi)) = uniform(pkg, &props, "VolumeModulation") {
                cue.volume = (cue.volume.0 * lo, cue.volume.1 * hi);
            }
            if let Some((lo, hi)) = uniform(pkg, &props, "PitchModulation") {
                cue.pitch = (cue.pitch.0 * lo, cue.pitch.1 * hi);
            }
        }
        "SoundNodeLooping" => cue.looping = true,
        _ => {}
    }
    for child in props.object_array(pkg, "ChildNodes") {
        walk(pkg, child, cue, depth + 1)?;
    }
    Ok(())
}

/// Min/max of a RawDistributionFloat struct whose Distribution is a
/// DistributionFloatUniform (or Constant) subobject.
fn uniform(pkg: &Package, props: &Props, name: &str) -> Option<(f32, f32)> {
    let t = props.get(name)?;
    let inner = Props::read_list(pkg, t.at, t.at + t.size, name).ok()?;
    let d = inner.object(pkg, "Distribution").filter(|&o| o > 0)?;
    let de = &pkg.exports[d as usize - 1];
    let dp = Props::read(pkg, de).ok()?;
    match pkg.class_name(de) {
        "DistributionFloatUniform" => Some((dp.f32(pkg, "Min").unwrap_or(0.0), dp.f32(pkg, "Max").unwrap_or(0.0))),
        "DistributionFloatConstant" => dp.f32(pkg, "Constant").map(|c| (c, c)),
        _ => None,
    }
}

/// The CompressedPCData bulk block (the second one after RawData).
fn wave_ogg(pkg: &Package, start: usize, end: usize) -> Result<Option<Vec<u8>>> {
    let mut c = Cursor::new(&pkg.data, start);
    for block in 0..2 {
        if c.pos + 16 > end {
            return Ok(None);
        }
        let flags = c.i32()?;
        let count = c.i32()? as usize;
        let disk = c.i32()?;
        let _offset = c.i32()?;
        let inline = flags & 1 == 0 && disk > 0;
        let at = c.pos;
        if inline {
            c.skip(disk as usize)?;
        }
        if block == 1 {
            if !inline || count == 0 {
                return Ok(None);
            }
            let bytes = if flags & 0x10 != 0 {
                decompress_chunked(&pkg.data, at)?
            } else {
                pkg.data[at..at + count].to_vec()
            };
            return Ok(bytes.starts_with(b"OggS").then_some(bytes));
        }
    }
    Ok(None)
}
