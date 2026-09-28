use rage_formats::blocks::drawable::{Drawable, DrawableModelsBlock};
use rage_formats::blocks::ydr::{build_ydr_from_xml, dump_ydr_xml, read_ydr};
use rage_formats::NameTable;

const XML: &str = include_str!("fixtures/one_triangle.ydr.xml");

#[test]
fn a_triangle_builds_and_the_old_parser_agrees() {
    let ydr = build_ydr_from_xml(XML, None).unwrap();
    let d = rage_formats::parse_ydr(&ydr).unwrap();
    assert_eq!(d.name, "tri_prop");
    let lod = d.best_lod().unwrap();
    assert_eq!(d.triangle_count(lod), 1);
    assert_eq!(d.geometry_count(), 1);
    assert_eq!(d.shader_count(), 1);
}

#[test]
fn dump_of_a_build_is_the_input() {
    let ydr = build_ydr_from_xml(XML, None).unwrap();
    let dumped = dump_ydr_xml(&ydr, &NameTable::core(), None).unwrap();
    assert_eq!(dumped, XML);
}

#[test]
fn render_masks_and_lod_pointers_for_absent_lists() {
    let xml = XML.replace("DrawableModelsHigh", "DrawableModelsMedium");
    let ydr = build_ydr_from_xml(&xml, None).unwrap();
    let (g, root) = read_ydr(&ydr).unwrap();
    let d = g.get::<Drawable>(root);
    assert_eq!(d.render_mask_flags[0] >> 8 & 0xFF, 0, "no high models: high mask 0");
    assert_eq!(d.render_mask_flags[1] >> 8 & 0xFF, 255);
    let models = g.get::<DrawableModelsBlock>(d.models.unwrap());
    assert!(models.high.is_none() && models.med.as_ref().unwrap().len() == 1);
    assert_eq!(dump_ydr_xml(&ydr, &NameTable::core(), None).unwrap(), xml);
}

/// Two geometries in a model (the outer box, the 16-byte padding of the shader mapping), bone
/// ids past four (the extra 8 bytes after the geometry) and a second LOD list.
#[test]
fn several_geometries_bone_ids_and_lods_round_trip() {
    let start = XML.find("        <Item>\n          <ShaderIndex").unwrap();
    let end = XML.find("      </Geometries>").unwrap();
    let geom = &XML[start..end];
    let second = geom
        .replace("<BoundingBoxMax x=\"1\" y=\"1\" z=\"1\" w=\"0\" />", "<BoundingBoxMax x=\"3\" y=\"1\" z=\"2\" w=\"0\" />\n          <BoneIDs>1, 2, 3, 4, 5</BoneIDs>")
        .replace("<ShaderIndex value=\"0\" />", "<ShaderIndex value=\"1\" />");
    let mut xml = XML.replace(geom, &format!("{geom}{second}"));
    let first_model = xml.find("    <Item>\n      <RenderMask").unwrap();
    let last_model = xml.find("  </DrawableModelsHigh>").unwrap();
    let models = xml[first_model..last_model].to_owned();
    xml = xml.replace("</DrawableModelsHigh>\n", &format!("</DrawableModelsHigh>\n  <DrawableModelsLow>\n{models}  </DrawableModelsLow>\n"));

    let ydr = build_ydr_from_xml(&xml, None).unwrap();
    let old = rage_formats::parse_ydr(&ydr).unwrap();
    assert_eq!(old.geometry_count(), 4);
    let (g, root) = read_ydr(&ydr).unwrap();
    let d = g.get::<Drawable>(root);
    let block = g.get::<DrawableModelsBlock>(d.models.unwrap());
    let model = g.get::<rage_formats::blocks::drawable::DrawableModel>(block.high.as_ref().unwrap()[0]);
    assert_eq!(model.shader_mapping(&g), vec![0, 1]);
    let bounds = model.bounds_data(&g);
    assert_eq!(bounds.len(), 3);
    assert_eq!(bounds[0].1.x, 3.0, "the outer box comes first");
    assert_eq!((d.render_mask_flags[2] >> 8) & 0xFF, 255);
    assert_eq!(dump_ydr_xml(&ydr, &NameTable::core(), None).unwrap(), xml);
    // a graph read back writes the same file as the graph built from the XML
    let (mut g2, root2) = read_ydr(&ydr).unwrap();
    assert_eq!(rage_formats::blocks::ydr::write_ydr(&mut g2, root2).unwrap(), ydr);
}

#[test]
fn a_drawable_carries_its_bound() {
    let xml = XML.replace("</DrawableModelsHigh>\n", &format!("</DrawableModelsHigh>\n{}", include_str!("fixtures/box_bounds_fragment.xml")));
    assert_ne!(xml, XML);
    let ydr = build_ydr_from_xml(&xml, None).unwrap();
    assert_eq!(dump_ydr_xml(&ydr, &NameTable::core(), None).unwrap(), xml);
    let (mut g, root) = read_ydr(&ydr).unwrap();
    let d = g.get::<Drawable>(root);
    let bound = d.bound.expect("the bound pointer is read");
    assert!(matches!(g.get::<rage_formats::blocks::bounds::BoundBlock>(bound), rage_formats::blocks::bounds::BoundBlock::Box(_)));
    assert!(g.get::<rage_formats::blocks::bounds::BoundBlock>(bound).common().pages.is_none(), "only the drawable root owns pages");
    assert_eq!(rage_formats::blocks::ydr::write_ydr(&mut g, root).unwrap(), ydr);
    assert_eq!(rage_formats::parse_ydr(&ydr).unwrap().triangle_count(rage_formats::parse_ydr(&ydr).unwrap().best_lod().unwrap()), 1);
}

#[test]
fn a_drawable_with_a_none_bound_has_none() {
    let xml = XML.replace("</DrawableModelsHigh>\n", "</DrawableModelsHigh>\n  <Bounds type=\"None\" />\n");
    let ydr = build_ydr_from_xml(&xml, None).unwrap();
    let (g, root) = read_ydr(&ydr).unwrap();
    assert!(g.get::<Drawable>(root).bound.is_none());
    assert_eq!(dump_ydr_xml(&ydr, &NameTable::core(), None).unwrap(), XML);
}
