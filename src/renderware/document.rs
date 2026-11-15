use anyhow::{bail, ensure, Context, Result};
use super::chunk::{hex, unhex, parse_file_chunks, validate_tree, MAX_BYTES};
use super::{Chunk, write_chunks};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format { Txd, Dff, Ipl, Col }

impl Format {
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "txd" => Some(Self::Txd), "dff" => Some(Self::Dff),
            "ipl" => Some(Self::Ipl), "col" => Some(Self::Col), _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self { Self::Txd => "txd", Self::Dff => "dff", Self::Ipl => "ipl", Self::Col => "col" }
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    pub format: Format,
    pub chunks: Vec<Chunk>,
    pub padding: Vec<u8>,
}

impl Document {
    pub fn parse(data: &[u8], format: Format) -> Result<Self> {
        ensure!(matches!(format, Format::Txd | Format::Dff), "{} semantic codec is not available yet", format.name());
        let (chunks, padding) = parse_file_chunks(data, true)?;
        ensure!(!chunks.is_empty(), "{} file has no root chunk", format.name());
        let root = if format == Format::Txd { 0x16 } else { 0x10 };
        ensure!(chunks.iter().any(|c| c.kind == root), "{} file has no root 0x{root:X} chunk", format.name());
        validate_tree(&chunks, 0)?;
        Ok(Self { format, chunks, padding })
    }

    pub fn to_json(&self) -> Result<String> {
        let chunks: Vec<_> = self.chunks.iter().map(|c| json::object! {
            kind: c.kind, version: c.version, payload: hex(&c.payload)
        }).collect();
        Ok(json::object! {
            schema: "rage-renderware/1", format: self.format.name(),
            chunks: json::JsonValue::Array(chunks), padding: hex(&self.padding)
        }.pretty(2))
    }

    pub fn from_json(text: &str) -> Result<Self> {
        ensure!(text.len() <= MAX_BYTES * 2 + 1024 * 1024, "editable document is too large");
        let value = json::parse(text).context("invalid RenderWare JSON")?;
        ensure!(value["schema"] == "rage-renderware/1", "unsupported RenderWare document schema");
        let format = value["format"].as_str().and_then(Format::from_extension).context("unknown document format")?;
        ensure!(value["chunks"].is_array(), "document chunks must be an array");
        ensure!(value["chunks"].len() <= 100_000, "too many document chunks");
        let mut chunks = Vec::new();
        for (i, c) in value["chunks"].members().enumerate() {
            chunks.push(Chunk {
                kind: c["kind"].as_u32().with_context(|| format!("chunk {i} needs a u32 kind"))?,
                version: c["version"].as_u32().with_context(|| format!("chunk {i} needs a u32 version"))?,
                payload: unhex(c["payload"].as_str().with_context(|| format!("chunk {i} needs a hex payload"))?)?,
            });
        }
        let padding = unhex(value["padding"].as_str().context("document needs hex padding")?)?;
        ensure!(padding.iter().all(|b| *b == 0), "only zero sector padding is permitted");
        for (key, _) in value.entries() {
            if !matches!(key, "schema" | "format" | "chunks" | "padding") { bail!("unrecognized document field '{key}'"); }
        }
        let doc = Self { format, chunks, padding };
        // Validate edited chunk lengths/tree as rigorously as binary inputs.
        let bytes = doc.build()?;
        Self::parse(&bytes, format)
    }

    pub fn build(&self) -> Result<Vec<u8>> {
        validate_tree(&self.chunks, 0)?;
        let mut out = write_chunks(&self.chunks)?;
        ensure!(out.len().checked_add(self.padding.len()).is_some_and(|n| n <= MAX_BYTES), "output exceeds RenderWare size limit");
        out.extend_from_slice(&self.padding);
        Ok(out)
    }
}
