use rage_formats::{
    decompress_texture,
    renderware::{txd::Txd, Game},
};

fn chunk(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for n in [kind, payload.len() as u32, 0x1803ffff] {
        out.extend_from_slice(&n.to_le_bytes());
    }
    out.extend_from_slice(payload);
    out
}

fn fixture(
    platform: u32,
    raster: u32,
    d3d: u32,
    depth: u8,
    width: u16,
    height: u16,
    palette: &[u8],
    mips: &[Vec<u8>],
) -> Vec<u8> {
    let mut header = vec![0; 88];
    header[..4].copy_from_slice(&platform.to_le_bytes());
    header[4..8].copy_from_slice(&0x1101u32.to_le_bytes());
    header[8..12].copy_from_slice(b"test");
    header[72..76].copy_from_slice(&raster.to_le_bytes());
    header[76..80].copy_from_slice(&d3d.to_le_bytes());
    header[80..82].copy_from_slice(&width.to_le_bytes());
    header[82..84].copy_from_slice(&height.to_le_bytes());
    header[84] = depth;
    header[85] = mips.len() as u8;
    header[86] = 4;
    if platform == 8 {
        header[87] = if d3d == 0x31545844 {
            1
        } else if d3d == 0x33545844 {
            3
        } else if d3d == 0x35545844 {
            5
        } else {
            0
        };
    } else {
        header[87] = if matches!(d3d, 0x31545844 | 0x33545844 | 0x35545844) {
            8
        } else {
            1
        };
    }
    header.extend_from_slice(palette);
    for mip in mips {
        header.extend_from_slice(&(mip.len() as u32).to_le_bytes());
        header.extend_from_slice(mip);
    }
    let mut native = chunk(1, &header);
    native.extend(chunk(3, &chunk(0x12345678, b"opaque")));
    let mut dict = chunk(1, &[1, 0, 2, 0]);
    dict.extend(chunk(0x15, &native));
    dict.extend(chunk(3, &[]));
    chunk(0x16, &dict)
}

#[test]
fn packed_pc_rasters_decode_to_known_colors() {
    for (raster, d3d, depth, input, want) in [
        (0x200, 23, 16, vec![0, 0xf8], vec![255, 0, 0, 255]),
        (0x100, 25, 16, vec![0xe0, 0x83], vec![0, 255, 0, 255]),
        (0x300, 26, 16, vec![0, 0x8f], vec![255, 0, 0, 136]),
        (0xa00, 24, 16, vec![0, 0x7c], vec![255, 0, 0, 255]),
        (0x600, 22, 32, vec![3, 2, 1, 0], vec![1, 2, 3, 255]),
        (0x600, 20, 24, vec![3, 2, 1], vec![1, 2, 3, 255]),
        (0x500, 21, 32, vec![3, 2, 1, 4], vec![1, 2, 3, 4]),
        (0x400, 50, 8, vec![127], vec![127, 127, 127, 255]),
    ] {
        for platform in [8, 9] {
            let bytes = fixture(
                platform,
                raster,
                if platform == 8 { 1 } else { d3d },
                depth,
                1,
                1,
                &[],
                &[input.clone()],
            );
            let txd = Txd::parse(&bytes).unwrap();
            assert_eq!(
                decompress_texture(&txd.textures().unwrap()[0]).unwrap(),
                want,
                "raster {raster:x}, platform {platform}"
            );
            assert_eq!(txd.build().unwrap(), bytes);
        }
    }
}

#[test]
fn palettes_use_rgba_entries_and_low_nibble_first() {
    for (raster, depth, palette_size, indices) in [
        (0x4500, 4, 128, vec![0x10]),
        (0x2500, 8, 1024, vec![0, 1]),
        (0x4500, 8, 128, vec![0, 1]),
    ] {
        let mut palette = vec![0; palette_size];
        palette[..8].copy_from_slice(&[255, 0, 0, 255, 0, 255, 0, 128]);
        let bytes = fixture(8, raster, 1, depth, 2, 1, &palette, &[indices]);
        let txd = Txd::parse(&bytes).unwrap();
        assert_eq!(
            txd.decode_mip(0, 0).unwrap(),
            [255, 0, 0, 255, 0, 255, 0, 128]
        );
    }
}

#[test]
fn dxt_formats_and_mip_tails_decode() {
    for (format, mut prefix) in [
        (0x31545844, vec![]),
        (0x33545844, vec![255; 8]),
        (0x35545844, vec![255, 0, 0, 0, 0, 0, 0, 0]),
    ] {
        prefix.extend_from_slice(&[0, 0xf8, 0, 0, 0, 0, 0, 0]);
        let bytes = fixture(
            9,
            0x8500,
            format,
            16,
            4,
            4,
            &[],
            &[prefix.clone(), prefix.clone(), prefix],
        );
        let txd = Txd::parse(&bytes).unwrap();
        for (level, size) in [(0, 16), (1, 4), (2, 1)] {
            assert_eq!(
                txd.decode_mip(0, level).unwrap(),
                [255, 0, 0, 255].repeat(size)
            );
        }
        let target = txd.to_ytd().unwrap();
        let read = rage_formats::parse_ytd(&target).unwrap();
        assert_eq!(read[0].levels, 3);
        assert_eq!(
            decompress_texture(&read[0]).unwrap(),
            [255, 0, 0, 255].repeat(16)
        );
    }
}

#[test]
fn replace_preserves_unknown_chunks_and_other_encoded_entries() {
    let bytes = fixture(9, 0x500, 21, 32, 1, 1, &[], &[vec![3, 2, 1, 4]]);
    let mut txd = Txd::parse(&bytes).unwrap();
    let mut replacement = txd.textures().unwrap().remove(0);
    replacement.pixel_data = vec![9, 8, 7, 6];
    replacement.format = rage_formats::TextureFormat::A8B8G8R8;
    txd.replace(replacement).unwrap();
    let built = txd.build().unwrap();
    assert!(built.windows(6).any(|w| w == b"opaque"));
    assert_eq!(
        Txd::parse(&built).unwrap().decode_mip(0, 0).unwrap(),
        [9, 8, 7, 6]
    );
    let tex = Txd::parse(&bytes).unwrap().textures().unwrap().remove(0);
    assert!(Txd::from_textures(vec![tex.clone(), tex], Game::SanAndreas).is_err());
}

#[test]
fn bad_platform_dimensions_and_mip_sizes_fail() {
    for bytes in [
        fixture(5, 0x500, 21, 32, 1, 1, &[], &[vec![0; 4]]),
        fixture(9, 0x500, 21, 32, 0, 1, &[], &[vec![0; 4]]),
        fixture(9, 0x500, 21, 32, 4, 4, &[], &[vec![0; 1]]),
    ] {
        assert!(Txd::parse(&bytes).is_err());
    }
}

#[test]
fn editable_texture_fields_rename_remove_and_reject_typos() {
    use rage_formats::renderware::{Document, Format};
    let source = fixture(9, 0x500, 21, 32, 1, 1, &[], &[vec![3, 2, 1, 4]]);
    let doc = Document::parse(&source, Format::Txd).unwrap();
    let mut json = json::parse(&doc.to_json().unwrap()).unwrap();
    json["textures"][0]["name"] = "renamed".into();
    json["textures"][0]["mips"][0] = "06070809".into();
    let built = Document::from_json(&json.dump()).unwrap().build().unwrap();
    let txd = Txd::parse(&built).unwrap();
    assert_eq!(txd.native_textures[0].name, "renamed");
    assert_eq!(txd.decode_mip(0, 0).unwrap(), [8, 7, 6, 9]);
    json["textures"][0]["typo_field"] = 123.into();
    assert!(
        Document::from_json(&json.dump()).is_err(),
        "unknown editable fields must not be ignored"
    );
    json["textures"] = json::array![];
    let built = Document::from_json(&json.dump()).unwrap().build().unwrap();
    assert_eq!(Txd::parse(&built).unwrap().native_textures.len(), 0);
}

#[test]
fn rectangular_dds_preserves_all_source_mips() {
    let block = vec![0, 0xf8, 0, 0, 0, 0, 0, 0];
    let bytes = fixture(
        9,
        0x8200,
        0x31545844,
        16,
        4,
        2,
        &[],
        &[block.clone(), block.clone(), block],
    );
    let txd = Txd::parse(&bytes).unwrap();
    let dds = txd.native_textures[0].to_dds().unwrap();
    assert_eq!(u32::from_le_bytes(dds[28..32].try_into().unwrap()), 3);
    assert_eq!(&dds[128..], [0, 0xf8, 0, 0, 0, 0, 0, 0].repeat(3));
    assert!(
        txd.to_ytd().is_err(),
        "incompatible YTD tail layout must not silently drop data"
    );
}

#[test]
fn txd_insert_delete_preserves_an_unmodified_entry() {
    let original = fixture(9, 0x500, 21, 32, 1, 1, &[], &[vec![3, 2, 1, 4]]);
    let mut txd = Txd::parse(&original).unwrap();
    let mut texture = txd.textures().unwrap().remove(0);
    texture.name = "new".into();
    texture.name_hash = 0;
    txd.add(texture).unwrap();
    assert_eq!(
        Txd::parse(&txd.build().unwrap())
            .unwrap()
            .native_textures
            .len(),
        2
    );
    txd.remove("new").unwrap();
    assert_eq!(txd.build().unwrap(), original);
}

#[test]
fn d3d9_alpha_luminance_and_palette_formats() {
    for (format, depth, pixel, want) in [
        (28, 8, vec![64], vec![255, 255, 255, 64]),
        (51, 16, vec![127, 64], vec![127, 127, 127, 64]),
    ] {
        let bytes = fixture(9, 0x400, format, depth, 1, 1, &[], &[pixel]);
        let txd = Txd::parse(&bytes).unwrap();
        assert_eq!(txd.decode_mip(0, 0).unwrap(), want);
    }
    let mut palette = vec![0; 1024];
    palette[..4].copy_from_slice(&[1, 2, 3, 4]);
    let bytes = fixture(9, 0x2500, 41, 8, 1, 1, &palette, &[vec![0]]);
    assert_eq!(
        Txd::parse(&bytes).unwrap().decode_mip(0, 0).unwrap(),
        [1, 2, 3, 4]
    );
}

#[test]
fn new_dictionaries_use_source_game_stream_versions() {
    let original = fixture(9, 0x500, 21, 32, 1, 1, &[], &[vec![3, 2, 1, 4]]);
    let tex = Txd::parse(&original).unwrap().textures().unwrap().remove(0);
    for (game, version, platform) in [
        (Game::Iii, 0x0401ffff, 8),
        (Game::ViceCity, 0x0c02ffff, 8),
        (Game::SanAndreas, 0x1803ffff, 9),
    ] {
        let txd = Txd::from_textures(vec![tex.clone()], game).unwrap();
        let bytes = txd.build().unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            version
        );
        assert_eq!(txd.native_textures[0].platform, platform);
    }
}

#[test]
fn opaque_dxt1_does_not_turn_black_pixels_transparent() {
    let bytes = fixture(
        9,
        0x200,
        0x31545844,
        16,
        4,
        4,
        &[],
        &[vec![0, 0, 0, 0, 255, 255, 255, 255]],
    );
    let txd = Txd::parse(&bytes).unwrap();
    assert_eq!(txd.decode_mip(0, 0).unwrap(), [0, 0, 0, 255].repeat(16));
    assert_eq!(
        rage_formats::decompress_texture(&txd.textures().unwrap()[0]).unwrap(),
        [0, 0, 0, 255].repeat(16)
    );
    let dds = txd.native_textures[0].to_dds().unwrap();
    let exported = rage_formats::parse_dds(&dds).unwrap();
    assert_eq!(
        rage_formats::decompress_texture(&exported).unwrap(),
        [0, 0, 0, 255].repeat(16)
    );
}

#[test]
fn retail_empty_mip_tail_records_are_preserved_but_not_fabricated() {
    let block = vec![0, 0xf8, 0, 0, 0, 0, 0, 0];
    let bytes = fixture(
        9,
        0x8200,
        0x31545844,
        16,
        4,
        4,
        &[],
        &[block, vec![], vec![]],
    );
    let txd = Txd::parse(&bytes).unwrap();
    assert_eq!(txd.build().unwrap(), bytes);
    assert!(txd.decode_mip(0, 1).is_err());
    let target = rage_formats::parse_ytd(&txd.to_ytd().unwrap()).unwrap();
    assert_eq!(target[0].levels, 1);
    let mut invalid = bytes.clone();
    // An absent top mip is not a usable texture.
    let all_empty = fixture(
        9,
        0x8200,
        0x31545844,
        16,
        4,
        4,
        &[],
        &[vec![], vec![], vec![]],
    );
    assert!(Txd::parse(&all_empty).is_err());
    invalid.truncate(15);
    assert!(Txd::parse(&invalid).is_err());
}

#[test]
fn dxt1_alpha_mode_preserves_transparent_texels() {
    let mut bytes = fixture(
        9,
        0x100,
        0x31545844,
        16,
        4,
        4,
        &[],
        &[vec![0, 0, 0, 0, 255, 255, 255, 255]],
    );
    // Dictionary root(12), dict struct(16), native(12), native struct(12), flags(+87).
    bytes[12 + 16 + 12 + 12 + 87] = 9;
    let txd = Txd::parse(&bytes).unwrap();
    assert_eq!(txd.decode_mip(0, 0).unwrap(), [0, 0, 0, 0].repeat(16));
    assert_eq!(
        rage_formats::decompress_texture(&txd.textures().unwrap()[0]).unwrap(),
        [0, 0, 0, 0].repeat(16)
    );
}

#[test]
fn adding_to_empty_dictionary_retains_its_game_platform() {
    let original=fixture(9,0x500,21,32,1,1,&[],&[vec![3,2,1,4]]);
    let tex=Txd::parse(&original).unwrap().textures().unwrap().remove(0);
    for (game,platform) in [(Game::Iii,8),(Game::ViceCity,8),(Game::SanAndreas,9)] {
        let mut txd=Txd::from_textures(vec![],game).unwrap();
        txd.add(tex.clone()).unwrap();
        assert_eq!(Txd::parse(&txd.build().unwrap()).unwrap().native_textures[0].platform,platform);
    }
}
