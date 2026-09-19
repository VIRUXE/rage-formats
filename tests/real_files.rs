//! Checks against real game files, run only when the paths are given:
//! `RAGE_TEST_YNV=<navmesh cell> RAGE_TEST_YBN=<collision> cargo test --test real_files -- --ignored --nocapture`

use rage_formats::{parse_ybn, parse_ynv, serialize_ynv};

#[test]
#[ignore]
fn vanilla_cell_round_trips() {
    let Ok(path) = std::env::var("RAGE_TEST_YNV") else { return };
    let data = std::fs::read(&path).unwrap();
    let ynv = parse_ynv(&data).unwrap();
    let interior = ynv.polys.iter().filter(|p| p.is_interior()).count();
    let linked = ynv.polys.iter().flat_map(|p| &p.edges).filter(|e| !e.a.is_none()).count();
    let foreign = ynv.polys.iter().flat_map(|p| &p.edges).filter(|e| !e.a.is_none() && e.a.area_id != ynv.area_id).count();
    println!(
        "area {} vft 0x{:08X} flags {} unk2 0x{:08X}\nbb {:?} .. {:?} size {:?}\npolys {} (interior {}) edges linked {} (to other cells {}) portals {} points {}",
        ynv.area_id, ynv.vft, ynv.content_flags, ynv.version_unk2, ynv.bb_min, ynv.bb_max, ynv.bb_size,
        ynv.polys.len(), interior, linked, foreign, ynv.portals.len(), ynv.points.len()
    );
    let sizes: std::collections::BTreeMap<usize, usize> = ynv.polys.iter().fold(Default::default(), |mut m, p| { *m.entry(p.vertices.len()).or_default() += 1; m });
    println!("vertex counts: {sizes:?}");
    let sample = &ynv.polys[0];
    println!("poly 0: flags0 {:#06x} flags1 {:#010x} flags2 {:#010x} centroid {:?}", sample.flags0, sample.flags1, sample.flags2, sample.centroid());

    for (i, p) in ynv.polys.iter().enumerate().step_by(200).take(12) {
        let (lo, hi) = p.bounds();
        println!("poly {i}: unkx {:3} unky {:3} centroid ({:.2}, {:.2}) lo ({:.2}, {:.2}) hi ({:.2}, {:.2}) n {}",
            p.flags2 & 0xFF, (p.flags2 >> 8) & 0xFF, p.centroid().x, p.centroid().y, lo.x, lo.y, hi.x, hi.y, p.vertices.len());
    }
    // Cafe footprint: what vanilla polys sit inside it, and how are polys wound?
    let inside: Vec<usize> = ynv.polys.iter().enumerate()
        .filter(|(_, p)| { let c = p.centroid(); c.x > -601.0 && c.x < -551.0 && c.y > -1079.0 && c.y < -1050.0 && c.z > 19.0 && c.z < 28.0 })
        .map(|(i, _)| i).collect();
    println!("vanilla polys inside cafe footprint: {} {:?}", inside.len(), &inside[..inside.len().min(20)]);
    let mut cw = 0; let mut ccw = 0;
    for p in &ynv.polys {
        let n = p.vertices.len();
        let area: f32 = (0..n).map(|i| { let a = p.vertices[i]; let b = p.vertices[(i + 1) % n]; a.x * b.y - b.x * a.y }).sum();
        if area > 0.0 { ccw += 1 } else { cw += 1 }
    }
    println!("winding: ccw {ccw} cw {cw}");
    for &i in inside.iter().take(3) { println!("  inside poly {i}: {:?} flags1 {:#x}", ynv.polys[i].vertices, ynv.polys[i].flags1); }
    let bytes = serialize_ynv(&ynv).unwrap();
    println!("rewritten: {} bytes (original {})", bytes.len(), data.len());
    let back = parse_ynv(&bytes).unwrap();
    assert_eq!(back.area_id, ynv.area_id);
    assert_eq!(back.polys.len(), ynv.polys.len());
    assert_eq!(back.portals, ynv.portals);
    assert_eq!(back.points.len(), ynv.points.len());
    for (i, (a, b)) in ynv.polys.iter().zip(&back.polys).enumerate() {
        assert_eq!(a.edges, b.edges, "poly {i} edges");
        assert_eq!((a.flags0, a.flags1, a.flags2, &a.portal_links), (b.flags0, b.flags1, b.flags2, &b.portal_links), "poly {i} flags");
        assert_eq!(a.vertices.len(), b.vertices.len(), "poly {i} vertex count");
        for (v, w) in a.vertices.iter().zip(&b.vertices) {
            assert!((*v - *w).length() < 0.01, "poly {i}: {v:?} vs {w:?}");
        }
    }
    let again = serialize_ynv(&back).unwrap();
    assert_eq!(again, bytes, "second serialisation must be byte-identical");
}

#[test]
#[ignore]
fn collision_parses() {
    let Ok(path) = std::env::var("RAGE_TEST_YBN") else { return };
    let data = std::fs::read(&path).unwrap();
    let ybn = parse_ybn(&data).unwrap();
    println!("root {:?} box {:?} .. {:?} children {}", ybn.root.kind, ybn.root.box_min, ybn.root.box_max, ybn.root.children.len());
    for (i, (child, xf)) in ybn.root.children.iter().enumerate() {
        let g = child.geometry.as_ref();
        println!("  child {i}: {:?} box {:?}..{:?} identity {} verts {} tris {} prims {} mats {}",
            child.kind, child.box_min, child.box_max, xf.is_identity(),
            g.map_or(0, |g| g.vertices.len()), g.map_or(0, |g| g.triangles.len()), g.map_or(0, |g| g.primitive_count), g.map_or(0, |g| g.materials.len()));
    }
    let tris = ybn.triangles();
    let up = tris.iter().filter(|t| t.normal().z > 0.7).count();
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for t in &tris { for v in t.vertices { lo = lo.min(v.z); hi = hi.max(v.z); } }
    println!("{} triangles, {} facing up, z {lo:.2}..{hi:.2}", tris.len(), up);
    let mut mats: std::collections::BTreeMap<u8, usize> = Default::default();
    for t in &tris { *mats.entry(t.material).or_default() += 1; }
    println!("materials: {mats:?}");
    assert!(!tris.is_empty());
}

#[test]
#[ignore]
fn ymap_entities_parse() {
    let Ok(path) = std::env::var("RAGE_TEST_YMAP") else { return };
    let data = std::fs::read(&path).unwrap();
    let entities = rage_formats::parse_ymap_entities(&data).unwrap();
    for e in &entities {
        println!("{e:?}");
        println!("  local origin -> world {:?}; local (10,0,0) -> {:?}", e.to_world(rage_formats::Vec3::new(0.0, 0.0, 0.0)), e.to_world(rage_formats::Vec3::new(10.0, 0.0, 0.0)));
    }
    assert!(!entities.is_empty());
}

#[test]
#[ignore]
fn ytyp_mlo_parses() {
    let Ok(dir) = std::env::var("RAGE_TEST_MLO_DIR") else { return };
    let dir = std::path::Path::new(&dir);
    let mut names = std::collections::HashMap::new();
    for entry in std::fs::read_dir(dir.join("ydr")).unwrap().flatten() {
        let stem = entry.path().file_stem().unwrap().to_string_lossy().to_lowercase();
        names.insert(rage_formats::rage_joaat(&stem), stem);
    }
    let mut all = rage_formats::Ytyp::default();
    for entry in std::fs::read_dir(dir.join("ytyp")).unwrap().flatten() {
        let y = rage_formats::parse_ytyp(&std::fs::read(entry.path()).unwrap()).unwrap();
        println!("{}: {} archetypes, {} mlos", entry.path().display(), y.archetypes.len(), y.mlos.len());
        all.archetypes.extend(y.archetypes);
        all.mlos.extend(y.mlos);
    }
    let boxes: std::collections::HashMap<u32, &rage_formats::Archetype> = all.archetypes.iter().map(|a| (a.name_hash, a)).collect();
    for mlo in &all.mlos {
        println!("MLO {} ({} entities, {} rooms)", names.get(&mlo.name_hash).cloned().unwrap_or_default(), mlo.entities.len(), mlo.rooms.len());
        for r in &mlo.rooms { println!("  room {:?}..{:?} flags {:#x}", r.bb_min, r.bb_max, r.flags); }
        for e in &mlo.entities {
            let name = names.get(&e.archetype_hash).cloned().unwrap_or_else(|| format!("{:#010x}", e.archetype_hash));
            match boxes.get(&e.archetype_hash) {
                Some(a) => println!("  {name:40} at ({:.2}, {:.2}, {:.2}) rot {:?} box {:.2}x{:.2}x{:.2} (z {:.2}..{:.2})",
                    e.position.x, e.position.y, e.position.z, e.rotation,
                    a.bb_max.x - a.bb_min.x, a.bb_max.y - a.bb_min.y, a.bb_max.z - a.bb_min.z, a.bb_min.z, a.bb_max.z),
                None => println!("  {name:40} at ({:.2}, {:.2}, {:.2}) box unknown", e.position.x, e.position.y, e.position.z),
            }
        }
    }
    assert!(!all.mlos.is_empty());
}
