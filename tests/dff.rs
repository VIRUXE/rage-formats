use rage_formats::{renderware::dff::Dff, Vec3};

fn chunk(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for n in [kind, payload.len() as u32, 0x1803ffff] {
        out.extend_from_slice(&n.to_le_bytes());
    }
    out.extend_from_slice(payload);
    out
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn same(actual: &[u8], expected: &[u8]) {
    assert!(
        actual == expected,
        "DFF bytes differ: first mismatch {:?}, lengths {} vs {}",
        actual.iter().zip(expected).position(|(a, b)| a != b),
        actual.len(),
        expected.len()
    );
}

fn fixture(unknown: bool, skin: bool) -> Vec<u8> {
    let mut frames = words(&[2]);
    for (x, parent) in [(10., u32::MAX), (2., 0)] {
        frames.extend(floats(&[1., 0., 0., 0., 1., 0., 0., 0., 1., x, 0., 0.]));
        frames.extend(words(&[parent, 0]));
    }
    let mut frame_list = chunk(1, &frames);
    for name in ["root", "mesh"] {
        let mut plugins = chunk(0x253f2fe, name.as_bytes());
        if skin && name == "root" {
            plugins.extend(chunk(0x11e, &words(&[0x100, 10, 1, 0, 36, 10, 0, 0])));
        }
        frame_list.extend(chunk(3, &plugins));
    }
    let mut data = words(&[0x1001e, 1, 3, 1]);
    data.extend([255, 255, 255, 255].repeat(3));
    data.extend(floats(&[0., 0., 1., 0., 0., 1.]));
    for n in [1u16, 0, 0, 2] {
        data.extend_from_slice(&n.to_le_bytes());
    }
    data.extend(floats(&[0., 0., 0., 2.]));
    data.extend(words(&[1, 1]));
    data.extend(floats(&[0., 0., 0., 1., 0., 0., 0., 1., 0.]));
    data.extend(floats(&[0., 0., 1.].repeat(3)));
    let mut material = words(&[0]);
    material.extend([255, 128, 64, 200]);
    material.extend(words(&[0, 1]));
    material.extend(floats(&[1., 0., 1.]));
    let mut texture = chunk(1, &words(&[0x1106]));
    texture.extend(chunk(2, b"diffuse\0"));
    texture.extend(chunk(2, &[0; 4]));
    texture.extend(chunk(3, &[]));
    let mut material_nodes = chunk(1, &material);
    material_nodes.extend(chunk(6, &texture));
    material_nodes.extend(chunk(3, &[]));
    let mut materials = chunk(1, &words(&[1, u32::MAX]));
    materials.extend(chunk(7, &material_nodes));
    let mut plugins = chunk(0x50e, &words(&[0, 1, 3, 3, 0, 0, 1, 2]));
    if unknown {
        plugins.extend(chunk(0xdeadc0de, b"opaque geometry"));
    }
    if skin {
        let mut payload = vec![1, 1, 1, 0, 0];
        payload.extend([0, 0, 0, 0].repeat(3));
        payload.extend(floats(&[1., 0., 0., 0.].repeat(3)));
        payload.extend(floats(&[
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ]));
        payload.extend(words(&[0, 0, 0]));
        plugins.extend(chunk(0x116, &payload));
    }
    let mut geom = chunk(1, &data);
    geom.extend(chunk(8, &materials));
    geom.extend(chunk(3, &plugins));
    let mut geoms = chunk(1, &words(&[1]));
    geoms.extend(chunk(0xf, &geom));
    let mut atomic = chunk(1, &words(&[1, 0, 5, 0]));
    atomic.extend(chunk(3, &[]));
    let mut clump = chunk(1, &words(&[1, 0, 0]));
    clump.extend(chunk(0xe, &frame_list));
    clump.extend(chunk(0x1a, &geoms));
    clump.extend(chunk(0x14, &atomic));
    clump.extend(chunk(3, &[]));
    let mut bytes = chunk(0x10, &clump);
    bytes.resize(bytes.len().div_ceil(2048) * 2048, 0);
    bytes
}

#[test]
fn frames_geometry_materials_and_winding_are_preserved() {
    let bytes = fixture(true, false);
    let dff = Dff::parse(&bytes).unwrap();
    assert_eq!(dff.frames.len(), 2);
    assert_eq!(dff.frames[1].name, "mesh");
    assert_eq!(
        dff.frame_world(1)
            .unwrap()
            .transform_point(Vec3::ZERO)
            .xyz(),
        Vec3::new(12., 0., 0.)
    );
    assert_eq!(dff.geometries[0].triangles[0].indices, [0, 1, 2]);
    assert_eq!(dff.geometries[0].materials[0].color, [255, 128, 64, 200]);
    assert_eq!(
        dff.geometries[0].materials[0]
            .texture
            .as_ref()
            .unwrap()
            .name,
        "diffuse"
    );
    same(&dff.build().unwrap(), &bytes);
    let drawable = dff.drawables().unwrap().remove(0).drawable;
    let geometry = &drawable.lods[0].models[0].geometries[0];
    let vertices = geometry
        .vertex_buffer
        .as_ref()
        .unwrap()
        .to_unified_vertices()
        .unwrap();
    assert_eq!(vertices[0].position, Vec3::new(12., 0., 0.));
    assert_eq!(geometry.index_buffer.as_ref().unwrap().indices, [0, 1, 2]);
}

#[test]
fn edits_rebuild_geometry_and_bin_mesh_without_losing_unknown_payloads() {
    let mut dff = Dff::parse(&fixture(true, false)).unwrap();
    dff.geometries[0].morph_targets[0].positions[1] = Vec3::new(2., 0., 0.);
    let bytes = dff.build().unwrap();
    assert!(bytes.windows(15).any(|w| w == b"opaque geometry"));
    assert_eq!(
        Dff::parse(&bytes).unwrap().geometries[0].morph_targets[0].positions[1],
        Vec3::new(2., 0., 0.)
    );
    let triangle = dff.geometries[0].triangles[0].clone();
    dff.geometries[0].triangles.push(triangle);
    assert!(
        dff.build().is_err(),
        "topology changes must reject unmodeled dependent plugins"
    );
    let mut known = Dff::parse(&fixture(false, false)).unwrap();
    let triangle = known.geometries[0].triangles[0].clone();
    known.geometries[0].triangles.push(triangle);
    let rebuilt = Dff::parse(&known.build().unwrap()).unwrap();
    assert_eq!(rebuilt.geometries[0].triangles.len(), 2);
    assert_eq!(
        rebuilt.geometries[0]
            .bin_mesh
            .as_ref()
            .unwrap()
            .indices_count,
        6
    );
}

#[test]
fn skin_and_hanim_survive_roundtrip() {
    let bytes = fixture(false, true);
    let mut dff = Dff::parse(&bytes).unwrap();
    assert_eq!(dff.frames[0].hanim.as_ref().unwrap().nodes[0].id, 10);
    let skin = dff.geometries[0].skin.as_ref().unwrap();
    assert_eq!(skin.weights[0], [1., 0., 0., 0.]);
    assert_eq!(skin.inverse_bind_matrices.len(), 1);
    same(&dff.build().unwrap(), &bytes);
    dff.geometries[0].skin.as_mut().unwrap().weights[0] = [0.5, 0.5, 0., 0.];
    assert_eq!(
        Dff::parse(&dff.build().unwrap()).unwrap().geometries[0]
            .skin
            .as_ref()
            .unwrap()
            .weights[0],
        [0.5, 0.5, 0., 0.]
    );
}

#[test]
fn invalid_frame_links_and_triangle_indices_fail() {
    let mut dff = Dff::parse(&fixture(false, false)).unwrap();
    dff.frames[0].parent = 1;
    assert!(dff.build().is_err());
    let mut dff = Dff::parse(&fixture(false, false)).unwrap();
    dff.geometries[0].triangles[0].indices[0] = 99;
    assert!(dff.build().is_err());
}

#[test]
fn parent_rotation_is_applied_after_child_translation() {
    let mut dff = Dff::parse(&fixture(false, false)).unwrap();
    dff.frames[0].transform = rage_formats::Mat4([
        0., 1., 0., 0., -1., 0., 0., 0., 0., 0., 1., 0., 10., 0., 0., 1.,
    ]);
    assert_eq!(
        dff.frame_world(1)
            .unwrap()
            .transform_point(Vec3::ZERO)
            .xyz(),
        Vec3::new(10., 2., 0.)
    );
    let dff = Dff::parse(&dff.build().unwrap()).unwrap();
    assert_eq!(
        dff.frame_world(1).unwrap().transform_point(Vec3::X).xyz(),
        Vec3::new(10., 3., 0.)
    );
}

fn edit_geometry(
    bytes: &[u8],
    edit: impl FnOnce(&mut Vec<rage_formats::renderware::Chunk>),
) -> Vec<u8> {
    use rage_formats::renderware::{parse_chunks, write_chunks, Document, Format};
    let mut document = Document::parse(bytes, Format::Dff).unwrap();
    let mut root = parse_chunks(&document.chunks[0].payload).unwrap();
    let list = root.iter_mut().find(|c| c.kind == 0x1a).unwrap();
    let mut geometries = parse_chunks(&list.payload).unwrap();
    let geometry = geometries.iter_mut().find(|c| c.kind == 0xf).unwrap();
    let mut nodes = parse_chunks(&geometry.payload).unwrap();
    edit(&mut nodes);
    geometry.payload = write_chunks(&nodes).unwrap();
    list.payload = write_chunks(&geometries).unwrap();
    document.chunks[0].payload = write_chunks(&root).unwrap();
    document.build().unwrap()
}

#[test]
fn shared_materials_can_be_edited_without_aliasing_the_original() {
    use rage_formats::renderware::{parse_chunks, write_chunks};
    let bytes = edit_geometry(&fixture(false, false), |nodes| {
        let list = nodes.iter_mut().find(|c| c.kind == 8).unwrap();
        let mut children = parse_chunks(&list.payload).unwrap();
        children.iter_mut().find(|c| c.kind == 1).unwrap().payload = words(&[2, u32::MAX, 0]);
        list.payload = write_chunks(&children).unwrap();
    });
    let mut dff = Dff::parse(&bytes).unwrap();
    same(&dff.build().unwrap(), &bytes);
    dff.geometries[0].materials[1].color = [0, 255, 0, 255];
    let edited = Dff::parse(&dff.build().unwrap()).unwrap();
    assert_eq!(edited.geometries[0].materials[0].color, [255, 128, 64, 200]);
    assert_eq!(edited.geometries[0].materials[1].color, [0, 255, 0, 255]);
}

#[test]
fn a_second_uv_set_is_rebuilt_and_available_to_renderers() {
    let mut dff = Dff::parse(&fixture(false, false)).unwrap();
    dff.geometries[0].uv_sets.push(vec![[0.2, 0.3]; 3]);
    let dff = Dff::parse(&dff.build().unwrap()).unwrap();
    assert_eq!(dff.geometries[0].uv_sets.len(), 2);
    let entry = dff.drawables().unwrap().remove(0);
    let vertices = entry.drawable.lods[0].models[0].geometries[0]
        .vertex_buffer
        .as_ref()
        .unwrap()
        .to_unified_vertices()
        .unwrap();
    assert_eq!(vertices[0].texcoord1, rage_formats::Vec2::new(0.2, 0.3));
}

#[test]
fn mixed_child_library_stamps_survive_rebuilding() {
    use rage_formats::renderware::{parse_chunks, write_chunks, Document, Format};
    let mut document = Document::parse(&fixture(false, false), Format::Dff).unwrap();
    let mut root = parse_chunks(&document.chunks[0].payload).unwrap();
    let frame_list = root.iter_mut().find(|c| c.kind == 0xe).unwrap();
    let mut frames = parse_chunks(&frame_list.payload).unwrap();
    frames[1].version = 0x1003ffff;
    frame_list.payload = write_chunks(&frames).unwrap();
    document.chunks[0].payload = write_chunks(&root).unwrap();
    let bytes = document.build().unwrap();
    same(&Dff::parse(&bytes).unwrap().build().unwrap(), &bytes);
}

#[test]
fn editable_scene_updates_known_fields_and_checks_plugin_dependencies() {
    use rage_formats::renderware::{Document, Format};
    let source = fixture(false, true);
    let document = Document::parse(&source, Format::Dff).unwrap();
    let mut value = json::parse(&document.to_json().unwrap()).unwrap();
    assert_eq!(value["scene"]["frames"][0]["name"], "root");
    value["scene"]["frames"][1]["name"] = "edited".into();
    value["scene"]["geometries"][0]["morph_targets"][0]["positions"][1][0] = 2.into();
    value["scene"]["geometries"][0]["materials"][0]["texture"]["name"] = "new_diffuse".into();
    let bytes = Document::from_json(&value.dump()).unwrap().build().unwrap();
    let dff = Dff::parse(&bytes).unwrap();
    assert_eq!(dff.frames[1].name, "edited");
    assert_eq!(
        dff.geometries[0].morph_targets[0].positions[1],
        Vec3::new(2., 0., 0.)
    );
    assert_eq!(
        dff.geometries[0].materials[0]
            .texture
            .as_ref()
            .unwrap()
            .name,
        "new_diffuse"
    );
    value["scene"]["frames"][0]["typo"] = 1.into();
    assert!(Document::from_json(&value.dump()).is_err());
    let doc = Document::parse(&fixture(true, false), Format::Dff).unwrap();
    let mut value = json::parse(&doc.to_json().unwrap()).unwrap();
    let face = value["scene"]["geometries"][0]["triangles"][0].clone();
    value["scene"]["geometries"][0]["triangles"]
        .push(face)
        .unwrap();
    assert!(Document::from_json(&value.dump()).is_err());
}

#[test]
fn night_colors_and_native_layout_detection() {
    use rage_formats::renderware::{parse_chunks, write_chunks};
    let bytes = edit_geometry(&fixture(false, false), |nodes| {
        let ext = nodes.iter_mut().find(|c| c.kind == 3).unwrap();
        let mut p = words(&[1]);
        p.extend([5, 6, 7, 8].repeat(3));
        let mut plugins = parse_chunks(&ext.payload).unwrap();
        plugins.extend(parse_chunks(&chunk(0x253f2f9, &p)).unwrap());
        ext.payload = write_chunks(&plugins).unwrap();
    });
    let dff = Dff::parse(&bytes).unwrap();
    same(&dff.build().unwrap(), &bytes);
    assert_eq!(
        dff.geometries[0].night_colors.as_ref().unwrap().colors[0],
        [5, 6, 7, 8]
    );
    let entry = dff.drawables().unwrap().remove(0);
    let vertices = entry.drawable.lods[0].models[0].geometries[0]
        .vertex_buffer
        .as_ref()
        .unwrap()
        .to_unified_vertices()
        .unwrap();
    assert_eq!(vertices[0].color1, [5, 6, 7, 8]);
    let native = edit_geometry(&fixture(false, false), |nodes| {
        nodes.iter_mut().find(|c| c.kind == 1).unwrap().payload[3] = 1;
    });
    assert!(format!("{:#}", Dff::parse(&native).unwrap_err()).contains("native geometry"));
}

#[test]
fn source_uv_sentinels_are_preserved_but_not_rendered_as_valid_coordinates() {
    let bytes = edit_geometry(&fixture(false, false), |nodes| {
        nodes.iter_mut().find(|c| c.kind == 1).unwrap().payload[28..32]
            .copy_from_slice(&u32::MAX.to_le_bytes());
    });
    let dff = Dff::parse(&bytes).unwrap();
    same(&dff.build().unwrap(), &bytes);
    assert!(dff.geometries[0].uv_sets[0][0][0].is_nan());
    assert!(
        dff.drawables().is_err(),
        "active non-finite UVs must be reported"
    );
}

#[test]
fn unused_skin_matrix_words_are_opaque_not_floating_point_values() {
    use rage_formats::renderware::{parse_chunks, write_chunks};
    let bytes = edit_geometry(&fixture(false, true), |nodes| {
        let ext = nodes.iter_mut().find(|c| c.kind == 3).unwrap();
        let mut plugins = parse_chunks(&ext.payload).unwrap();
        let skin = plugins.iter_mut().find(|c| c.kind == 0x116).unwrap();
        skin.payload[125..129].copy_from_slice(&u32::MAX.to_le_bytes());
        ext.payload = write_chunks(&plugins).unwrap();
    });
    let dff = Dff::parse(&bytes).unwrap();
    assert_eq!(
        dff.geometries[0]
            .skin
            .as_ref()
            .unwrap()
            .inverse_bind_matrices[0]
            .0[15],
        1.
    );
    same(&dff.build().unwrap(), &bytes);
}

#[test]
fn unused_vertices_do_not_leak_uv_sentinels_into_render_buffers() {
    let mut dff = Dff::parse(&fixture(false, false)).unwrap();
    let g = &mut dff.geometries[0];
    g.morph_targets[0]
        .positions
        .push(Vec3::new(1000., 1000., 1000.));
    g.morph_targets[0].normals.push(Vec3::Z);
    g.colors.push([255; 4]);
    g.uv_sets[0].push([f32::from_bits(u32::MAX); 2]);
    let dff = Dff::parse(&dff.build().unwrap()).unwrap();
    let entry = dff.drawables().unwrap().remove(0);
    let geometry = &entry.drawable.lods[0].models[0].geometries[0];
    assert_eq!(geometry.vertices_count, 3);
    let vertices = geometry
        .vertex_buffer
        .as_ref()
        .unwrap()
        .to_unified_vertices()
        .unwrap();
    assert!(vertices.iter().all(|v| v.texcoord0.x.is_finite()));
    assert_eq!(entry.drawable.bounds.box_max, Vec3::new(13., 1., 0.));
}

#[test]
fn editable_documents_preserve_signed_zero_and_uv_bit_patterns() {
    use rage_formats::renderware::{Document, Format};
    let bytes = edit_geometry(&fixture(false, true), |nodes| {
        let data = &mut nodes.iter_mut().find(|c| c.kind == 1).unwrap().payload;
        data[28..32].copy_from_slice(&0x80000000u32.to_le_bytes());
        data[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    });
    let doc = Document::parse(&bytes, Format::Dff).unwrap();
    let text = doc.to_json().unwrap();
    let built = Document::from_json(&text).unwrap().build().unwrap();
    same(&built, &bytes);
}

#[test]
fn builders_reject_values_that_cannot_be_written_as_affine_frames_or_bounds() {
    let mut dff = Dff::parse(&fixture(false, false)).unwrap();
    dff.frames[0].transform.0[3] = 0.5;
    assert!(dff.build().is_err());
    let mut dff = Dff::parse(&fixture(false, false)).unwrap();
    dff.geometries[0].morph_targets[0].center.x = f32::NAN;
    assert!(dff.build().is_err());
}

#[test]
fn material_effect_records_have_typed_lossless_views() {
    use rage_formats::renderware::plugins::{
        Effects2d, MaterialEffects, ReflectionMaterial, SpecularMaterial,
    };
    let mut env = words(&[2, 2]);
    env.extend(floats(&[0.5]));
    env.extend(words(&[0, 0, 0]));
    let parsed = MaterialEffects::parse(&env).unwrap();
    assert_eq!(parsed.effect, 2);
    assert_eq!(parsed.build().unwrap(), env);
    let mut reflection = floats(&[1., 2., 3., 4., 0.25]);
    reflection.extend(words(&[0x12345678]));
    let parsed = ReflectionMaterial::parse(&reflection).unwrap();
    assert_eq!(parsed.intensity, 0.25);
    assert_eq!(parsed.build().unwrap(), reflection);
    let mut spec = floats(&[0.75]);
    spec.extend(b"specular\0");
    spec.resize(28, 0);
    let parsed = SpecularMaterial::parse(&spec).unwrap();
    assert_eq!(parsed.texture, "specular");
    assert_eq!(parsed.build().unwrap(), spec);
    let mut fx = words(&[1]);
    fx.extend(floats(&[1., 2., 3.]));
    fx.extend(words(&[0, 4]));
    fx.extend([255, 128, 64, 255]);
    let parsed = Effects2d::parse(&fx).unwrap();
    assert_eq!(parsed.records[0].position, Vec3::new(1., 2., 3.));
    assert_eq!(parsed.build().unwrap(), fx);
}

#[test]
fn editable_float_fields_do_not_lose_a_float32_ulp(){
    use rage_formats::renderware::{Document,Format};
    let bytes=edit_geometry(&fixture(false,false),|nodes|{nodes.iter_mut().find(|c|c.kind==1).unwrap().payload[28..32].copy_from_slice(&0x3f604f2fu32.to_le_bytes());});
    let doc=Document::parse(&bytes,Format::Dff).unwrap();let built=Document::from_json(&doc.to_json().unwrap()).unwrap().build().unwrap();same(&built,&bytes);
}
