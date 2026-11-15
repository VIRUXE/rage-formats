use anyhow::{ensure, Context, Result};

const MAX_CHUNKS: usize = 100_000;
pub const MAX_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub kind: u32,
    pub version: u32,
    pub payload: Vec<u8>,
}

pub fn parse_chunks(data: &[u8]) -> Result<Vec<Chunk>> {
    let (chunks, padding) = parse_file_chunks(data, false)?;
    ensure!(padding.is_empty(), "unexpected padding");
    Ok(chunks)
}

/// Root streams extracted from IMG can have sector padding. Only the caller
/// that owns that container boundary may opt into retaining it as padding.
pub(crate) fn parse_file_chunks(data: &[u8], allow_padding: bool) -> Result<(Vec<Chunk>, Vec<u8>)> {
    ensure!(data.len() <= MAX_BYTES, "RenderWare input exceeds {MAX_BYTES} bytes at offset 0");
    let mut chunks = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        if allow_padding && data[offset..].iter().all(|b| *b == 0) {
            return Ok((chunks, data[offset..].to_vec()));
        }
        ensure!(chunks.len() < MAX_CHUNKS, "too many RenderWare chunks at offset {offset}");
        let header = data.get(offset..offset + 12).with_context(|| format!("truncated chunk header at offset {offset}"))?;
        let kind = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let version = u32::from_le_bytes(header[8..12].try_into().unwrap());
        let end = offset.checked_add(12).and_then(|n| n.checked_add(length))
            .with_context(|| format!("chunk length overflow at offset {offset}"))?;
        let payload = data.get(offset + 12..end)
            .with_context(|| format!("truncated chunk 0x{kind:X} at offset {offset}: declared {length} bytes"))?;
        chunks.push(Chunk { kind, version, payload: payload.to_vec() });
        offset = end;
    }
    Ok((chunks, Vec::new()))
}

pub fn write_chunks(chunks: &[Chunk]) -> Result<Vec<u8>> {
    ensure!(chunks.len() <= MAX_CHUNKS, "too many RenderWare chunks");
    let length = chunks.iter().try_fold(0usize, |n, c| n.checked_add(12)?.checked_add(c.payload.len()))
        .context("RenderWare output length overflow")?;
    ensure!(length <= MAX_BYTES, "RenderWare output exceeds {MAX_BYTES} bytes");
    let mut out = Vec::with_capacity(length);
    for chunk in chunks {
        let length = u32::try_from(chunk.payload.len()).context("chunk payload exceeds u32")?;
        out.extend_from_slice(&chunk.kind.to_le_bytes());
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&chunk.version.to_le_bytes());
        out.extend_from_slice(&chunk.payload);
    }
    Ok(out)
}

/// Only known container IDs have recursively chunked payloads. Unknown plugin
/// data is opaque, not guessed to be a tree from the bytes it starts with.
pub(crate) fn validate_tree(chunks: &[Chunk], depth: usize) -> Result<()> {
    ensure!(depth <= 64, "RenderWare nesting exceeds 64 levels");
    let mut count = chunks.len();
    for chunk in chunks {
        if container(chunk.kind) {
            validate_payload(&chunk.payload, depth + 1, &mut count)
                .map_err(|error| anyhow::anyhow!("inside chunk 0x{:X}: {error:#}", chunk.kind))?;
        }
    }
    Ok(())
}

fn container(kind: u32) -> bool {
    matches!(kind, 3 | 6 | 7 | 8 | 0xe | 0xf | 0x10 | 0x14 | 0x15 | 0x16 | 0x1a)
}

fn validate_payload(data: &[u8], depth: usize, count: &mut usize) -> Result<()> {
    ensure!(depth <= 64, "RenderWare nesting exceeds 64 levels");
    let mut offset = 0;
    while offset < data.len() {
        *count += 1;
        ensure!(*count <= MAX_CHUNKS, "too many nested RenderWare chunks");
        let header = data.get(offset..offset + 12).with_context(|| format!("truncated nested chunk header at offset {offset}"))?;
        let kind = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let end = offset.checked_add(12).and_then(|n| n.checked_add(length)).context("nested chunk length overflow")?;
        let payload = data.get(offset + 12..end).with_context(|| format!("truncated nested chunk at offset {offset}"))?;
        if container(kind) { validate_payload(payload, depth + 1, count)?; }
        offset = end;
    }
    Ok(())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes { write!(out, "{b:02x}").unwrap(); }
    out
}

pub(crate) fn unhex(text: &str) -> Result<Vec<u8>> {
    ensure!(text.len() % 2 == 0 && text.len() <= MAX_BYTES * 2, "invalid hex payload length");
    fn digit(b: u8) -> Option<u8> {
        match b { b'0'..=b'9' => Some(b - b'0'), b'a'..=b'f' => Some(b - b'a' + 10), b'A'..=b'F' => Some(b - b'A' + 10), _ => None }
    }
    text.as_bytes().chunks_exact(2).map(|p| {
        Ok(digit(p[0]).context("invalid hex payload")? * 16 + digit(p[1]).context("invalid hex payload")?)
    }).collect()
}
