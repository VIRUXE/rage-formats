use rage_formats::renderware::{parse_chunks, write_chunks, Document, Format};

fn chunk(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&kind.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0x1803ffffu32.to_le_bytes());
    bytes.extend_from_slice(payload); bytes
}

#[test]
fn unknown_chunk_roundtrip() {
    let bytes = chunk(0x12345678, &[7, 0, 9]);
    let chunks = parse_chunks(&bytes).unwrap();
    assert_eq!(chunks[0].kind, 0x12345678);
    assert_eq!(chunks[0].version, 0x1803ffff);
    assert_eq!(chunks[0].payload, [7, 0, 9]);
    assert_eq!(write_chunks(&chunks).unwrap(), bytes);
}

#[test]
fn rejects_truncated_and_overflowing_chunk() {
    for bytes in [vec![0; 11], vec![0x10,0,0,0,0xff,0xff,0xff,0xff,0,0,0,0], chunk(0x1234, &[1])[..12].to_vec()] {
        let result = std::panic::catch_unwind(|| parse_chunks(&bytes));
        assert!(result.is_ok(), "parser panicked");
        let error = result.unwrap().unwrap_err().to_string();
        assert!(error.contains("offset"), "{error}");
    }
}

#[test]
fn rejects_excessive_nesting() {
    let mut bytes = chunk(3, &[]);
    for _ in 0..70 { bytes = chunk(0x10, &bytes); }
    let error = Document::parse(&bytes, Format::Dff).unwrap_err().to_string();
    assert!(error.contains("nesting"), "{error}");
}

#[test]
fn document_schema_is_checked() {
    let bytes = chunk(0x16, &chunk(0x12345678, &[1, 2, 3]));
    let mut padded = bytes.clone(); padded.resize(2048, 0);
    let doc = Document::parse(&padded, Format::Txd).unwrap();
    let text = doc.to_json().unwrap();
    let doc = Document::from_json(&text).unwrap();
    assert_eq!(doc.build().unwrap(), padded);
    assert!(Document::from_json(&text.replace("rage-renderware/1", "rage-renderware/2")).is_err());
    assert!(Document::from_json(&text.replace("\"payload\":", "\"wrong\":")).is_err());
    assert!(Document::parse(&bytes, Format::Dff).is_err());
}

#[test]
fn chunk_edits_update_lengths_and_reject_bad_hex() {
    let doc = Document::parse(&chunk(0x10, &chunk(0x12345678, &[7])), Format::Dff).unwrap();
    let mut value = json::parse(&doc.to_json().unwrap()).unwrap();
    value["chunks"][0]["payload"] = "7856341202000000ffff03180708".into();
    let built = Document::from_json(&value.dump()).unwrap().build().unwrap();
    assert_eq!(built, chunk(0x10, &chunk(0x12345678, &[7,8])));
    value["chunks"][0]["payload"] = "0z".into();
    assert!(Document::from_json(&value.dump()).is_err());
}
