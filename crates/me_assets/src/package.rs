//! Unreal Engine 3 package reader, as used by Mirror's Edge (file version 536,
//! licensee 43). Enough to read name/import/export tables and object bytes,
//! including LZO-compressed packages.

use std::path::Path;

use crate::{Error, Result};

pub const TAG: u32 = 0x9E2A_83C1;

/// Little-endian cursor over a byte slice.
#[derive(Clone)]
pub struct Cursor<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::Truncated)?;
        let s = self.data.get(self.pos..end).ok_or(Error::Truncated)?;
        self.pos = end;
        Ok(s)
    }
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.take(n)
    }
    /// Array count, sanity-checked against the remaining bytes.
    pub fn count(&mut self, min_elem_size: usize) -> Result<usize> {
        let n = self.i32()?;
        if n < 0 || (n as usize).saturating_mul(min_elem_size) > self.data.len().saturating_sub(self.pos) {
            return Err(Error::Format(format!("bad array count {n} at {}", self.pos - 4)));
        }
        Ok(n as usize)
    }
    pub fn fstring(&mut self) -> Result<String> {
        let n = self.i32()?;
        if n == 0 {
            return Ok(String::new());
        }
        if n > 0 {
            let b = self.take(n as usize)?;
            Ok(b[..b.len() - 1].iter().map(|&c| c as char).collect())
        } else {
            let b = self.take((-n) as usize * 2)?;
            let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            Ok(String::from_utf16_lossy(&w[..w.len() - 1]))
        }
    }
}

#[derive(Clone, Debug)]
pub struct Import {
    pub class: String,
    pub outer: i32,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Export {
    pub class: i32,
    pub outer: i32,
    pub name: String,
    /// The template this object was made from (object index, 0 = the class default).
    pub archetype: i32,
    pub size: usize,
    pub offset: usize,
}

pub struct Package {
    pub data: Vec<u8>,
    pub names: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

impl Package {
    pub fn open(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).map_err(|e| Error::Io(path.display().to_string(), e))?;
        Self::parse(raw)
    }

    pub fn parse(raw: Vec<u8>) -> Result<Self> {
        let mut c = Cursor::new(&raw, 0);
        if c.u32()? != TAG {
            return Err(Error::Format("not an Unreal package".into()));
        }
        let version = c.u16()?;
        let _licensee = c.u16()?;
        if version != 536 {
            return Err(Error::Format(format!("package version {version}, expected 536 (Mirror's Edge)")));
        }
        let _header_size = c.i32()?;
        let _folder = c.fstring()?;
        let _flags = c.u32()?;
        let name_count = c.i32()? as usize;
        let name_off = c.i32()? as usize;
        let export_count = c.i32()? as usize;
        let export_off = c.i32()? as usize;
        let import_count = c.i32()? as usize;
        let import_off = c.i32()? as usize;
        let _depends = c.i32()?;
        c.skip(16)?; // guid
        let gens = c.count(12)?;
        c.skip(gens * 12)?;
        let _engine = c.i32()?;
        let _cooker = c.i32()?;
        let compression = c.u32()?;
        let nchunks = c.count(16)?;
        let mut chunks = Vec::with_capacity(nchunks);
        for _ in 0..nchunks {
            chunks.push((c.i32()? as usize, c.i32()? as usize, c.i32()? as usize, c.i32()? as usize));
        }
        let summary_end = c.pos;

        let data = if compression != 0 && !chunks.is_empty() {
            if compression != 2 {
                return Err(Error::Format(format!("unsupported package compression {compression}")));
            }
            let total = chunks.iter().map(|(uo, us, _, _)| uo + us).max().unwrap_or(0);
            let mut out = vec![0u8; total.max(summary_end)];
            out[..summary_end].copy_from_slice(&raw[..summary_end]);
            for &(uo, _us, co, _cs) in &chunks {
                let bytes = decompress_chunked(&raw, co)?;
                out[uo..uo + bytes.len()].copy_from_slice(&bytes);
            }
            out
        } else {
            raw
        };

        let mut pkg = Package { data, names: vec![], imports: vec![], exports: vec![] };
        let mut c = Cursor::new(&pkg.data, name_off);
        for _ in 0..name_count {
            pkg.names.push(c.fstring()?);
            c.u64()?;
        }
        let mut c = Cursor::new(&pkg.data, import_off);
        for _ in 0..import_count {
            let _pkg = read_name(&pkg.names, &mut c)?;
            let class = read_name(&pkg.names, &mut c)?;
            let outer = c.i32()?;
            let name = read_name(&pkg.names, &mut c)?;
            pkg.imports.push(Import { class, outer, name });
        }
        let mut c = Cursor::new(&pkg.data, export_off);
        for _ in 0..export_count {
            let class = c.i32()?;
            let _super = c.i32()?;
            let outer = c.i32()?;
            let name = read_name(&pkg.names, &mut c)?;
            let archetype = c.i32()?;
            let _flags = c.u64()?;
            let size = c.i32()? as usize;
            let offset = c.i32()? as usize;
            let comps = c.count(12)?;
            c.skip(comps * 12)?; // component map (version < 543)
            let _eflags = c.i32()?;
            let net = c.count(4)?;
            c.skip(net * 4 + 16 + 4)?; // net object counts, guid, package flags
            pkg.exports.push(Export { class, outer, name, archetype, size, offset });
        }
        Ok(pkg)
    }

    pub fn name(&self, index: i32, number: i32) -> String {
        let base = self.names.get(index as usize).cloned().unwrap_or_else(|| format!("<name {index}>"));
        if number == 0 { base } else { format!("{base}_{}", number - 1) }
    }

    pub fn object_name(&self, index: i32) -> &str {
        if index > 0 {
            self.exports.get(index as usize - 1).map_or("?", |e| e.name.as_str())
        } else if index < 0 {
            self.imports.get((-index) as usize - 1).map_or("?", |i| i.name.as_str())
        } else {
            "None"
        }
    }

    /// Full dotted path of an object (e.g. "A_Material_Footstep.Concrete._03_Female_FootStepRun").
    pub fn path(&self, index: i32) -> String {
        let mut parts = vec![];
        let mut i = index;
        let mut guard = 0;
        while i != 0 && guard < 64 {
            guard += 1;
            if i > 0 {
                let Some(e) = self.exports.get(i as usize - 1) else { break };
                parts.push(e.name.clone());
                i = e.outer;
            } else {
                let Some(m) = self.imports.get((-i) as usize - 1) else { break };
                parts.push(m.name.clone());
                i = m.outer;
            }
        }
        parts.reverse();
        parts.join(".")
    }

    /// Export whose full path (without the package name) matches.
    pub fn find_path(&self, class: &str, path: &str) -> Option<usize> {
        self.exports
            .iter()
            .enumerate()
            .find(|(i, e)| self.class_name(e) == class && self.path(*i as i32 + 1).eq_ignore_ascii_case(path))
            .map(|(i, _)| i)
    }

    pub fn class_name(&self, e: &Export) -> &str {
        if e.class == 0 { "Class" } else { self.object_name(e.class) }
    }

    /// First export with this name and class.
    pub fn find(&self, class: &str, name: &str) -> Option<&Export> {
        self.exports.iter().find(|e| e.name == name && self.class_name(e) == class)
    }

    pub fn of_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a Export> + 'a {
        self.exports.iter().filter(move |e| self.class_name(e) == class)
    }

    pub fn bytes(&self, e: &Export) -> Result<&[u8]> {
        self.data.get(e.offset..e.offset + e.size).ok_or(Error::Truncated)
    }
}

pub fn read_name(names: &[String], c: &mut Cursor) -> Result<String> {
    let i = c.i32()?;
    let n = c.i32()?;
    let base = names.get(i as usize).cloned().ok_or_else(|| Error::Format(format!("bad name index {i}")))?;
    Ok(if n == 0 { base } else { format!("{base}_{}", n - 1) })
}

/// A UE3 compressed-chunk block (tag, block size, sizes, per-block table, LZO data).
pub fn decompress_chunked(data: &[u8], at: usize) -> Result<Vec<u8>> {
    let mut c = Cursor::new(data, at);
    if c.u32()? != TAG {
        return Err(Error::Format("bad compressed chunk tag".into()));
    }
    let _block = c.i32()?;
    let _csum = c.i32()?;
    let usum = c.i32()? as usize;
    let mut blocks = vec![];
    let mut left = usum as i64;
    while left > 0 {
        let cs = c.i32()? as usize;
        let us = c.i32()? as usize;
        blocks.push((cs, us));
        left -= us as i64;
    }
    let mut out = Vec::with_capacity(usum);
    for (cs, us) in blocks {
        let src = c.bytes(cs)?;
        let dec = lzokay_native::decompress_all(src, Some(us)).map_err(|e| Error::Format(format!("LZO: {e:?}")))?;
        out.extend_from_slice(&dec);
    }
    Ok(out)
}
