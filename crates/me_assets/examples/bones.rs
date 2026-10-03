fn main() {
    let install = std::env::var("ME_INSTALL").unwrap();
    let arms = me_assets::FaithArms::load(std::path::Path::new(&install), 4).unwrap();
    let rest = me_assets::pose::Pose::rest(&arms.mesh);
    let mut g = vec![];
    me_assets::pose::globals(&arms.mesh, &rest, &mut g);
    for (i, b) in arms.mesh.bones.iter().enumerate() {
        let p = g[i].w_axis;
        println!("{i} {} parent {} at ({:.1},{:.1},{:.1})", b.name, b.parent, p.x, p.y, p.z);
    }
}
