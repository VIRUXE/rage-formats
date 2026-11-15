pub use super::raster::NativeTexture;
use super::{parse_chunks, write_chunks, Chunk, Document, Format, Game};
use crate::YtdTexture;
use anyhow::{ensure, Context, Result};

#[derive(Debug, Clone)]
pub struct Txd {
    document: Document,
    root_index: usize,
    nodes: Vec<Chunk>,
    entries: Vec<(usize, Vec<Chunk>, usize)>,
    pub native_textures: Vec<NativeTexture>,
}

impl Txd {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let document = Document::parse(data, Format::Txd)?;
        let root_index = document.chunks.iter().position(|c| c.kind == 0x16).unwrap();
        let nodes = parse_chunks(&document.chunks[root_index].payload)?;
        let header = nodes
            .iter()
            .find(|c| c.kind == 1)
            .context("TXD dictionary has no struct")?;
        ensure!(header.payload.len() >= 4, "TXD dictionary struct truncated");
        let count = u16::from_le_bytes(header.payload[..2].try_into().unwrap()) as usize;
        ensure!(count <= 16384, "TXD has too many textures");
        let mut entries = Vec::new();
        let mut native_textures = Vec::new();
        for (node_index, node) in nodes.iter().enumerate().filter(|(_, c)| c.kind == 0x15) {
            let children = parse_chunks(&node.payload)?;
            let struct_index = children
                .iter()
                .position(|c| c.kind == 1)
                .context("native texture has no struct")?;
            let texture = NativeTexture::parse(&children[struct_index].payload)
                .with_context(|| format!("native texture {}", native_textures.len()))?;
            entries.push((node_index, children, struct_index));
            native_textures.push(texture);
        }
        ensure!(
            count == native_textures.len(),
            "TXD declares {count} textures but contains {}",
            native_textures.len()
        );
        let result = Self {
            document,
            root_index,
            nodes,
            entries,
            native_textures,
        };
        result.check_names()?;
        Ok(result)
    }

    fn check_names(&self) -> Result<()> {
        let mut names = std::collections::HashSet::new();
        for texture in &self.native_textures {
            ensure!(
                names.insert(texture.name.to_ascii_lowercase()),
                "duplicate TXD texture name '{}'",
                texture.name
            );
        }
        Ok(())
    }

    pub fn build(&self) -> Result<Vec<u8>> {
        self.check_names()?;
        ensure!(
            self.entries.len() == self.native_textures.len(),
            "TXD entry structure changed without add/remove"
        );
        let mut document = self.document.clone();
        let mut nodes = self.nodes.clone();
        for ((node_index, children, struct_index), texture) in
            self.entries.iter().zip(&self.native_textures)
        {
            let mut children = children.clone();
            children[*struct_index].payload = texture.build()?;
            nodes[*node_index].payload = write_chunks(&children)?;
        }
        let header = nodes.iter_mut().find(|c| c.kind == 1).unwrap();
        header.payload[..2]
            .copy_from_slice(&u16::try_from(self.native_textures.len())?.to_le_bytes());
        document.chunks[self.root_index].payload = write_chunks(&nodes)?;
        document.build()
    }

    pub fn textures(&self) -> Result<Vec<YtdTexture>> {
        self.native_textures
            .iter()
            .map(NativeTexture::to_ytd_texture)
            .collect()
    }
    pub fn top_textures(&self) -> Result<Vec<YtdTexture>> {
        self.native_textures
            .iter()
            .map(|native| {
                let mut top = native.clone();
                top.mips.truncate(1);
                top.to_ytd_texture()
            })
            .collect()
    }
    pub fn decode_mip(&self, texture: usize, level: usize) -> Result<Vec<u8>> {
        self.native_textures
            .get(texture)
            .context("texture index out of bounds")?
            .decode_mip(level)
    }
    pub fn replace(&mut self, texture: YtdTexture) -> Result<()> {
        let index = self
            .native_textures
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(&texture.name))
            .context("replacement texture name does not exist")?;
        let old = &self.native_textures[index];
        let mut new = NativeTexture::from_ytd(
            &texture,
            if old.platform == 9 {
                Game::SanAndreas
            } else {
                Game::ViceCity
            },
        )?;
        new.mask = old.mask.clone();
        new.filter_flags = old.filter_flags;
        new.raster_type = old.raster_type;
        new.preserve_metadata(old);
        self.native_textures[index] = new;
        Ok(())
    }
    pub fn to_ytd(&self) -> Result<Vec<u8>> {
        crate::serialize_ytd(&self.textures()?)
    }

    pub fn add(&mut self, texture: YtdTexture) -> Result<()> {
        ensure!(
            !self
                .native_textures
                .iter()
                .any(|t| t.name.eq_ignore_ascii_case(&texture.name)),
            "texture '{}' already exists",
            texture.name
        );
        let stamp=super::chunk::version_number(self.document.chunks[self.root_index].version);
        let platform=self.native_textures.first().map_or(if stamp>=0x36000 {9}else{8},|t|t.platform);
        let game = if platform==9 {
            Game::SanAndreas
        } else if stamp<0x33000 {
            Game::Iii
        } else {
            Game::ViceCity
        };
        let native = NativeTexture::from_ytd(&texture, game)?;
        self.add_native(native)
    }

    fn add_native(&mut self, native: NativeTexture) -> Result<()> {
        ensure!(self.native_textures.len() < 16384, "too many TXD textures");
        let version = self.document.chunks[self.root_index].version;
        let children = vec![
            Chunk {
                kind: 1,
                version,
                payload: native.build()?,
            },
            Chunk {
                kind: 3,
                version,
                payload: Vec::new(),
            },
        ];
        let index = self
            .nodes
            .iter()
            .rposition(|c| c.kind == 3)
            .unwrap_or(self.nodes.len());
        for (i, _, _) in &mut self.entries {
            if *i >= index {
                *i += 1;
            }
        }
        self.nodes.insert(
            index,
            Chunk {
                kind: 0x15,
                version,
                payload: write_chunks(&children)?,
            },
        );
        self.entries.push((index, children, 0));
        self.native_textures.push(native);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        let index = self
            .native_textures
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name))
            .context("texture does not exist")?;
        self.remove_at(index);
        Ok(())
    }
    fn remove_at(&mut self, index: usize) {
        let (node_index, _, _) = self.entries.remove(index);
        self.nodes.remove(node_index);
        self.native_textures.remove(index);
        for (i, _, _) in &mut self.entries {
            if *i > node_index {
                *i -= 1;
            }
        }
    }

    pub fn editable_textures(&self) -> json::JsonValue {
        json::JsonValue::Array(
            self.native_textures
                .iter()
                .enumerate()
                .map(|(i, t)| t.to_json(i))
                .collect(),
        )
    }

    pub fn apply_texture_edits(&mut self, value: &json::JsonValue) -> Result<()> {
        ensure!(
            value.is_array() && value.len() <= 16384,
            "textures must be an array of at most 16384 entries"
        );
        let original = self.native_textures.clone();
        let mut retained = std::collections::HashSet::new();
        for v in value.members() {
            let index = v["source_index"]
                .as_usize()
                .context("texture source_index must refer to an existing entry")?;
            ensure!(
                index < original.len() && retained.insert(index),
                "invalid or repeated source_index {index}"
            );
        }
        for index in (0..original.len()).rev() {
            if !retained.contains(&index) {
                self.remove_at(index);
            }
        }
        let positions: std::collections::HashMap<_, _> = (0..original.len())
            .filter(|i| retained.contains(i))
            .enumerate()
            .map(|(position, index)| (index, position))
            .collect();
        for v in value.members() {
            let index = v["source_index"].as_usize().unwrap();
            self.native_textures[positions[&index]] = original[index].edited(v)?;
        }
        self.check_names()
    }

    pub fn from_textures(textures: Vec<YtdTexture>, game: Game) -> Result<Self> {
        ensure!(textures.len() <= 16384, "too many TXD textures");
        let version = match game {
            Game::Iii => 0x0401ffff,
            Game::ViceCity => 0x0c02ffff,
            Game::SanAndreas => 0x1803ffff,
        };
        let chunk = |kind, payload| Chunk {
            kind,
            version,
            payload,
        };
        let mut header = Vec::new();
        header.extend_from_slice(&u16::try_from(textures.len())?.to_le_bytes());
        header.extend_from_slice(&(if game == Game::SanAndreas { 2u16 } else { 0 }).to_le_bytes());
        let mut nodes = vec![chunk(1, header)];
        for texture in textures {
            let native = NativeTexture::from_ytd(&texture, game)?;
            nodes.push(chunk(
                0x15,
                write_chunks(&[chunk(1, native.build()?), chunk(3, Vec::new())])?,
            ));
        }
        nodes.push(chunk(3, Vec::new()));
        Self::parse(&write_chunks(&[chunk(0x16, write_chunks(&nodes)?)])?)
    }
}
