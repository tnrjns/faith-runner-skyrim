//! Texture2D: pick an inline mip, LZO-decompress it, decode DXT to RGBA8.

use crate::package::{decompress_chunked, Cursor, Package};
use crate::props::Props;
use crate::{Error, Result};

const BULK_SEPARATE_FILE: i32 = 0x01;
const BULK_LZO: i32 = 0x10;

#[derive(Clone, Debug)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Read `name`, using the largest inline mip no bigger than `max_size`.
pub fn read_texture(pkg: &Package, name: &str, max_size: u32) -> Result<Rgba> {
    let e = pkg.find("Texture2D", name).ok_or_else(|| Error::Missing(format!("Texture2D {name}")))?;
    read_texture_export(pkg, e, max_size)
}

/// Any texture export (Texture2D, LightMapTexture2D, ...), largest inline mip <= `max_size`.
pub fn read_texture_export(pkg: &Package, e: &crate::package::Export, max_size: u32) -> Result<Rgba> {
    let name = &e.name;
    let props = Props::read(pkg, e)?;
    let format = props.name(pkg, "Format").unwrap_or_else(|| "PF_A8R8G8B8".into());
    let mut c = Cursor::new(&pkg.data, props.end);
    skip_bulk(&mut c)?; // source art
    let nmips = c.count(24)?;
    let mut best: Option<(Vec<u8>, u32, u32)> = None;
    for _ in 0..nmips {
        let flags = c.i32()?;
        let count = c.i32()? as usize;
        let on_disk = c.i32()?;
        let offset = c.i32()? as usize;
        let inline = flags & BULK_SEPARATE_FILE == 0 && on_disk > 0;
        let data_at = c.pos;
        if inline {
            c.skip(on_disk as usize)?;
        }
        let w = c.i32()? as u32;
        let h = c.i32()? as u32;
        if best.is_some() || !inline || w > max_size || h > max_size {
            continue;
        }
        let bytes = if flags & BULK_LZO != 0 {
            decompress_chunked(&pkg.data, data_at)?
        } else {
            pkg.data[data_at..data_at + count].to_vec()
        };
        let _ = offset;
        best = Some((bytes, w, h));
    }
    let (bytes, w, h) = best.ok_or_else(|| Error::Missing(format!("{name}: no inline mip <= {max_size}")))?;
    let pixels = match format.as_str() {
        "PF_DXT1" => decode_bc(&bytes, w, h, false)?,
        "PF_DXT5" => decode_bc(&bytes, w, h, true)?,
        "PF_A8R8G8B8" => bytes.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect(),
        f => return Err(Error::Format(format!("{name}: unsupported texture format {f}"))),
    };
    Ok(Rgba { width: w, height: h, pixels })
}

fn skip_bulk(c: &mut Cursor) -> Result<()> {
    let flags = c.i32()?;
    let _count = c.i32()?;
    let on_disk = c.i32()?;
    let _offset = c.i32()?;
    if flags & BULK_SEPARATE_FILE == 0 && on_disk > 0 {
        c.skip(on_disk as usize)?;
    }
    Ok(())
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [((r * 255 + 15) / 31) as u8, ((g * 255 + 31) / 63) as u8, ((b * 255 + 15) / 31) as u8]
}

/// Decode BC1 (DXT1) or BC3 (DXT5) to RGBA8.
pub fn decode_bc(data: &[u8], w: u32, h: u32, alpha: bool) -> Result<Vec<u8>> {
    let bw = w.div_ceil(4) as usize;
    let bh = h.div_ceil(4) as usize;
    let block = if alpha { 16 } else { 8 };
    if data.len() < bw * bh * block {
        return Err(Error::Truncated);
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    for by in 0..bh {
        for bx in 0..bw {
            let b = &data[(by * bw + bx) * block..][..block];
            let (abytes, cbytes) = if alpha { b.split_at(8) } else { (&b[..0], b) };
            let c0 = u16::from_le_bytes([cbytes[0], cbytes[1]]);
            let c1 = u16::from_le_bytes([cbytes[2], cbytes[3]]);
            let (p0, p1) = (rgb565(c0), rgb565(c1));
            let mut pal = [[0u8; 4]; 4];
            pal[0] = [p0[0], p0[1], p0[2], 255];
            pal[1] = [p1[0], p1[1], p1[2], 255];
            if c0 > c1 || alpha {
                for k in 0..3 {
                    pal[2][k] = ((2 * p0[k] as u32 + p1[k] as u32) / 3) as u8;
                    pal[3][k] = ((p0[k] as u32 + 2 * p1[k] as u32) / 3) as u8;
                }
                pal[2][3] = 255;
                pal[3][3] = 255;
            } else {
                for k in 0..3 {
                    pal[2][k] = ((p0[k] as u32 + p1[k] as u32) / 2) as u8;
                }
                pal[2][3] = 255;
                pal[3] = [0, 0, 0, 0];
            }
            let bits = u32::from_le_bytes([cbytes[4], cbytes[5], cbytes[6], cbytes[7]]);
            // DXT5 alpha
            let mut alphas = [255u8; 16];
            if alpha {
                let a0 = abytes[0] as u32;
                let a1 = abytes[1] as u32;
                let mut ap = [0u8; 8];
                ap[0] = a0 as u8;
                ap[1] = a1 as u8;
                if a0 > a1 {
                    for k in 1..7 {
                        ap[k + 1] = (((7 - k as u32) * a0 + k as u32 * a1) / 7) as u8;
                    }
                } else {
                    for k in 1..5 {
                        ap[k + 1] = (((5 - k as u32) * a0 + k as u32 * a1) / 5) as u8;
                    }
                    ap[6] = 0;
                    ap[7] = 255;
                }
                let mut ab = 0u64;
                for k in 0..6 {
                    ab |= (abytes[2 + k] as u64) << (8 * k);
                }
                for (k, a) in alphas.iter_mut().enumerate() {
                    *a = ap[((ab >> (3 * k)) & 7) as usize];
                }
            }
            for py in 0..4 {
                for px in 0..4 {
                    let x = bx * 4 + px;
                    let y = by * 4 + py;
                    if x >= w as usize || y >= h as usize {
                        continue;
                    }
                    let k = py * 4 + px;
                    let mut col = pal[((bits >> (2 * k)) & 3) as usize];
                    if alpha {
                        col[3] = alphas[k];
                    }
                    let o = (y * w as usize + x) * 4;
                    out[o..o + 4].copy_from_slice(&col);
                }
            }
        }
    }
    Ok(out)
}
