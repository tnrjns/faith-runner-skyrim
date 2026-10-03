//! UE3 tagged-property lists (the default-value / serialized-property block at
//! the start of every object).

use crate::package::{read_name, Cursor, Export, Package};
use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct Tag {
    pub name: String,
    pub kind: String,
    pub size: usize,
    pub index: i32,
    /// Absolute offset of the value bytes in the package data.
    pub at: usize,
    pub bool_value: bool,
}

pub struct Props {
    pub tags: Vec<Tag>,
    /// Absolute offset just past the terminating "None".
    pub end: usize,
}

impl Props {
    pub fn read(pkg: &Package, e: &Export) -> Result<Self> {
        Self::read_list(pkg, e.offset + 4, e.offset + e.size, &e.name)
    }

    /// Properties of an object that may carry a header before them: placed actors have a
    /// state frame before the net index, components their template's class and name. Finds
    /// where the property list starts and reads it.
    pub fn read_object(pkg: &Package, e: &Export) -> Result<Self> {
        const KINDS: [&str; 11] = [
            "ObjectProperty", "StructProperty", "FloatProperty", "IntProperty", "BoolProperty", "NameProperty",
            "ByteProperty", "ArrayProperty", "StrProperty", "ComponentProperty", "ClassProperty",
        ];
        let end = e.offset + e.size;
        for skip in (4..80).step_by(4) {
            let at = e.offset + skip;
            if at + 8 > end {
                break;
            }
            let mut c = Cursor::new(&pkg.data, at);
            let Ok(first) = read_name(&pkg.names, &mut c) else { continue };
            if first != "None" {
                let Ok(kind) = read_name(&pkg.names, &mut c) else { continue };
                if !KINDS.contains(&kind.as_str()) {
                    continue;
                }
            }
            if let Ok(p) = Self::read_list(pkg, at, end, &e.name) {
                return Ok(p);
            }
        }
        Err(Error::Format(format!("no property list found in {}", e.name)))
    }

    /// A Vector struct property.
    pub fn vec3(&self, pkg: &Package, name: &str) -> Option<[f32; 3]> {
        let t = self.get(name)?;
        let mut c = Cursor::new(&pkg.data, t.at);
        c.vec3().ok()
    }

    /// A Rotator struct property (pitch, yaw, roll in 65536ths of a turn).
    pub fn rotator(&self, pkg: &Package, name: &str) -> Option<[i32; 3]> {
        let t = self.get(name)?;
        let mut c = Cursor::new(&pkg.data, t.at);
        Some([c.i32().ok()?, c.i32().ok()?, c.i32().ok()?])
    }

    /// A property list starting at `start` (e.g. one element of an array of
    /// structs), ending at its "None" terminator.
    pub fn read_list(pkg: &Package, start: usize, limit: usize, what: &str) -> Result<Self> {
        let mut c = Cursor::new(&pkg.data, start);
        let mut tags = vec![];
        loop {
            if c.pos >= limit {
                return Err(Error::Format(format!("unterminated properties in {what}")));
            }
            let name = read_name(&pkg.names, &mut c)?;
            if name == "None" {
                break;
            }
            let kind = read_name(&pkg.names, &mut c)?;
            let size = c.i32()? as usize;
            let index = c.i32()?;
            if kind == "StructProperty" {
                read_name(&pkg.names, &mut c)?;
            }
            if kind == "BoolProperty" {
                let v = c.i32()? != 0;
                tags.push(Tag { name, kind, size: 0, index, at: c.pos, bool_value: v });
                continue;
            }
            let at = c.pos;
            c.skip(size)?;
            tags.push(Tag { name, kind, size, index, at, bool_value: false });
        }
        Ok(Props { tags, end: c.pos })
    }

    pub fn get(&self, name: &str) -> Option<&Tag> {
        self.tags.iter().find(|t| t.name == name)
    }

    pub fn f32(&self, pkg: &Package, name: &str) -> Option<f32> {
        let t = self.get(name)?;
        Cursor::new(&pkg.data, t.at).f32().ok()
    }

    pub fn i32(&self, pkg: &Package, name: &str) -> Option<i32> {
        let t = self.get(name)?;
        Cursor::new(&pkg.data, t.at).i32().ok()
    }

    /// NameProperty, or a ByteProperty holding an enum name.
    pub fn name(&self, pkg: &Package, name: &str) -> Option<String> {
        let t = self.get(name)?;
        if t.size == 8 {
            read_name(&pkg.names, &mut Cursor::new(&pkg.data, t.at)).ok()
        } else {
            None
        }
    }

    /// Elements of an array of structs, each read as its own property list.
    pub fn struct_array(&self, pkg: &Package, name: &str) -> Vec<Props> {
        let Some(t) = self.get(name) else { return vec![] };
        let mut c = Cursor::new(&pkg.data, t.at);
        let Ok(n) = c.count(8) else { return vec![] };
        let limit = t.at + t.size;
        let mut out = Vec::with_capacity(n);
        let mut pos = c.pos;
        for _ in 0..n {
            match Props::read_list(pkg, pos, limit, name) {
                Ok(p) => {
                    pos = p.end;
                    out.push(p);
                }
                Err(_) => break,
            }
        }
        out
    }

    /// Array of object references.
    pub fn object_array(&self, pkg: &Package, name: &str) -> Vec<i32> {
        let Some(t) = self.get(name) else { return vec![] };
        let mut c = Cursor::new(&pkg.data, t.at);
        let Ok(n) = c.count(4) else { return vec![] };
        (0..n).filter_map(|_| c.i32().ok()).collect()
    }

    pub fn object(&self, pkg: &Package, name: &str) -> Option<i32> {
        let t = self.get(name)?;
        Cursor::new(&pkg.data, t.at).i32().ok()
    }
}
