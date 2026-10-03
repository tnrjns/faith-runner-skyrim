//! Writes an uncompressed copy of a UE3 package (header compression flag cleared), so the
//! Python tools in `tools/me-extract` can read it without `python-lzo`.
//!
//! `cargo run -p me_assets --example decompress -- <in.u> <out.u>`
use me_assets::package::Cursor;

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("input package");
    let output = args.next().expect("output path");
    let raw = std::fs::read(&input).expect("read input");
    let pkg = me_assets::Package::parse(raw.clone()).expect("parse package");
    let mut data = pkg.data;

    // Walk the summary to the compression flag and zero it (the chunk table is then ignored).
    let mut c = Cursor::new(&raw, 8);
    c.i32().unwrap(); // header size
    c.fstring().unwrap(); // folder
    c.skip(4 + 4 * 7 + 16).unwrap(); // flags, name/export/import tables, depends, guid
    let gens = c.count(12).unwrap();
    c.skip(gens * 12 + 8).unwrap(); // generations, engine + cooker versions
    data[c.pos..c.pos + 4].copy_from_slice(&0u32.to_le_bytes());

    std::fs::write(&output, &data).expect("write output");
    println!("{output}: {} bytes, {} exports", data.len(), pkg.exports.len());
}
