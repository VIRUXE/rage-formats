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

/// `RAGE_TEST_YMAP`: the header reads, the generic dump agrees with the
/// fixed reader on the entity count, and the XML names every structure and
/// member (only content hashes — archetypes, dictionaries — may be left).
#[test]
#[ignore]
fn ymap_dumps_with_every_member_named() {
    use rage_formats::{dump_meta, parse_ymap, to_xml, NameTable};
    let Ok(path) = std::env::var("RAGE_TEST_YMAP") else { return };
    let data = std::fs::read(&path).unwrap();
    let ymap = parse_ymap(&data).unwrap();
    println!("name {:#010x} parent {:#010x} flags {:#x} content {:?} streaming {:?}..{:?} entities {} (mlo {})",
        ymap.header.name_hash, ymap.header.parent_hash, ymap.header.flags, ymap.header.content_flag_names(),
        ymap.header.streaming_extents_min, ymap.header.streaming_extents_max, ymap.entities.len(), ymap.mlo_instances.len());
    let dump = dump_meta(&data).unwrap();
    assert!(dump.warnings.is_empty(), "{:?}", dump.warnings);
    let root = dump.root.as_struct().unwrap();
    assert_eq!(root.field("entities").unwrap().items().len(), ymap.entities.len());
    assert_eq!(root.field("name").and_then(|v| v.as_hash()), Some(ymap.header.name_hash));
    let xml = to_xml(&dump.root, &NameTable::core());
    let unnamed_tags: Vec<&str> = xml.lines().filter_map(|l| l.trim().strip_prefix("<hash_")).map(|l| l.split(['>', ' ', '/']).next().unwrap_or("")).collect();
    assert!(unnamed_tags.is_empty(), "members without names: {unnamed_tags:?}");
}

/// `RAGE_TEST_YMF`: a manifest in any container lists its dependencies.
#[test]
#[ignore]
fn manifest_parses() {
    use rage_formats::parse_ymf;
    let Ok(path) = std::env::var("RAGE_TEST_YMF") else { return };
    let data = std::fs::read(&path).unwrap();
    let (format, m) = parse_ymf(&data).unwrap();
    println!("{format:?}: {} groups, {} imap deps, {} imap deps 2, {} ityp deps 2, {} hd txd bindings, {} interiors",
        m.map_data_groups.len(), m.imap_dependencies.len(), m.imap_dependencies_2.len(), m.ityp_dependencies_2.len(), m.hd_txd_bindings.len(), m.interiors.len());
    for d in &m.imap_dependencies_2 {
        println!("  {} -> {}", d.name, d.ityp_deps.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "));
    }
    assert!(!m.is_empty(), "manifest lists nothing");
}

/// `RAGE_TEST_YTYP`: the generic dump of a type file names every member.
#[test]
#[ignore]
fn ytyp_dumps_with_every_member_named() {
    use rage_formats::{dump_meta, parse_ytyp, to_xml, NameTable};
    let Ok(path) = std::env::var("RAGE_TEST_YTYP") else { return };
    let data = std::fs::read(&path).unwrap();
    let ytyp = parse_ytyp(&data).unwrap();
    let dump = dump_meta(&data).unwrap();
    assert!(dump.warnings.is_empty(), "{:?}", dump.warnings);
    let root = dump.root.as_struct().unwrap();
    assert_eq!(root.field("archetypes").unwrap().items().len(), ytyp.archetypes.len());
    let xml = to_xml(&dump.root, &NameTable::core());
    let unnamed_tags: Vec<&str> = xml.lines().filter_map(|l| l.trim().strip_prefix("<hash_")).map(|l| l.split(['>', ' ', '/']).next().unwrap_or("")).collect();
    assert!(unnamed_tags.is_empty(), "members without names: {unnamed_tags:?}");
    println!("{} archetypes, {} MLOs, {} XML lines", ytyp.archetypes.len(), ytyp.mlos.len(), xml.lines().count());
}

/// `RAGE_TEST_YND`: a retail path node cell (or a folder of them) reads,
/// rewrites to the same nodes, links and junctions, and serialises the same
/// bytes twice; the XML round-trips too.
#[test]
#[ignore]
fn path_nodes_round_trip() {
    use rage_formats::{dump_ynd_xml, ynd_from_xml, ynd_to_xml, NameTable};
    let Ok(path) = std::env::var("RAGE_TEST_YND") else { return };
    let path = std::path::Path::new(&path);
    let files: Vec<std::path::PathBuf> = if path.is_dir() {
        let mut v: Vec<_> = std::fs::read_dir(path).unwrap().flatten().map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ynd"))).collect();
        v.sort();
        v
    } else {
        vec![path.to_path_buf()]
    };
    let names = NameTable::core();
    let (mut identical, mut rewritten) = (0usize, 0usize);
    for file in &files {
        let data = std::fs::read(file).unwrap();
        let d = rage_formats::ynd::parse_node_dictionary(&data).unwrap();
        let ynd = rage_formats::parse_ynd(&data).unwrap();
        let ped = ynd.nodes.iter().filter(|n| n.is_ped_node()).count();
        let foreign = ynd.nodes.iter().flat_map(|n| &n.links).filter(|l| Some(l.area_id as u32) != ynd.area_id()).count();
        let junctions = ynd.junctions.len();
        if files.len() == 1 || junctions > 0 || ynd.area_id().is_some_and(|a| a >= 1024) {
            println!("{}: vft {:#010x} area {:?} nodes {} (vehicle {} ped {}, {} ped by flags) links {} ({} to other cells) junctions {} refs {} heightmap bytes {} unk {:?}",
                file.display(), ynd.vft, ynd.area_id(), ynd.nodes.len(), ynd.vehicle_node_count, ynd.ped_node_count, ped,
                d.links.len(), foreign, junctions, ynd.junction_refs.len(), d.junction_heightmap_bytes.len(), ynd.unknowns);
        }
        let unused = d.nodes.iter().filter(|n| n.unused0 != 0 || n.unused1 != 0 || n.unused2 != 0 || n.unused3 != 0 || n.unused4 != 0).count();
        assert_eq!(unused, 0, "{}: nodes with non-zero unused words", file.display());
        if let Some(n) = ynd.nodes.first() {
            println!("  node 0: {:?} street {:#010x} flags {} {} {} {} {} speed {:?} special {} links {}",
                n.position, n.street_name, n.flags0, n.flags1, n.flags2, n.flags3, n.flags4, n.speed(), n.special().name(), n.links.len());
        }
        let bytes = rage_formats::serialize_ynd(&ynd).unwrap();
        let back = rage_formats::parse_ynd(&bytes).unwrap();
        assert_eq!(back, ynd, "{}", file.display());
        assert_eq!(back.to_dictionary().unwrap(), d, "{}: the dictionary is rebuilt as read", file.display());
        let again = rage_formats::serialize_ynd(&back).unwrap();
        assert_eq!(again, bytes, "{}: second serialisation must be byte-identical", file.display());
        if bytes == data { identical += 1 } else { rewritten += 1 }

        // The XML carries no vtable value, so a file built from it gets the default one.
        let xml = ynd_to_xml(&ynd, &names);
        let mut from_xml = ynd_from_xml(&xml).unwrap();
        from_xml.vft = ynd.vft;
        assert_eq!(from_xml, ynd, "{}: XML round trip", file.display());
        assert_eq!(dump_ynd_xml(&bytes, &names).unwrap(), xml);
    }
    println!("{} files: {identical} rewritten byte-identical to retail, {rewritten} laid out differently", files.len());
}
