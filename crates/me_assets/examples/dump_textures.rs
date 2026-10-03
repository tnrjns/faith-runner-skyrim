//! Writes each arm material's decoded diffuse texture as raw RGBA, for checking.
fn main() {
    let dir = std::env::args().nth(1).expect("install dir");
    let out = std::env::args().nth(2).unwrap_or_else(|| ".".into());
    let a = me_assets::FaithArms::load(std::path::Path::new(&dir), 1024).unwrap();
    for s in &a.materials {
        if let Some(t) = &s.diffuse {
            let path = format!("{out}/{}_{}x{}.rgba", s.material, t.width, t.height);
            std::fs::write(&path, &t.pixels).unwrap();
            println!("{path}");
        }
    }
}
