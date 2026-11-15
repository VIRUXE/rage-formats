use super::plugins::{BinMesh, NightColors, Skin};
use super::reader::{put_f32, put_u32, put_vec3, Reader};
use super::{parse_chunks, write_chunks, Chunk};
use crate::Vec3;
use anyhow::{ensure, Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Triangle {
    pub indices: [u32; 3],
    pub material: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MorphTarget {
    pub center: Vec3,
    pub radius: f32,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    positions_flag: u32,
    normals_flag: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextureRef {
    pub name: String,
    pub mask: String,
    pub filter_flags: u32,
    nodes: Vec<Chunk>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub flags: u32,
    pub color: [u8; 4],
    pub texture: Option<TextureRef>,
    pub surface: [f32; 3],
    pub plugins: Vec<Chunk>,
    nodes: Vec<Chunk>,
}

#[derive(Debug, Clone)]
pub struct Geometry {
    pub flags: u32,
    pub colors: Vec<[u8; 4]>,
    pub uv_sets: Vec<Vec<[f32; 2]>>,
    pub triangles: Vec<Triangle>,
    pub morph_targets: Vec<MorphTarget>,
    pub materials: Vec<Material>,
    pub plugins: Vec<Chunk>,
    pub skin: Option<Skin>,
    pub bin_mesh: Option<BinMesh>,
    pub night_colors: Option<NightColors>,
    nodes: Vec<Chunk>,
    material_nodes: Vec<Chunk>,
    material_refs: Vec<i32>,
    original_vertex_count: usize,
    original_triangles: Vec<Triangle>,
    original_uv_count: usize,
    original_morphs: Vec<MorphTarget>,
    legacy_surface: Vec<u8>,
    trailing: Vec<u8>,
}

pub(crate) fn text(data: &[u8]) -> Result<String> {
    let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
    String::from_utf8(data[..end].to_vec()).context("DFF string is not UTF-8")
}
pub(crate) fn struct_index(nodes: &[Chunk]) -> Result<usize> {
    nodes
        .iter()
        .position(|c| c.kind == 1)
        .context("chunk has no struct")
}
pub(crate) fn extensions(nodes: &[Chunk]) -> Result<Vec<Chunk>> {
    nodes
        .iter()
        .find(|c| c.kind == 3)
        .map(|c| parse_chunks(&c.payload))
        .unwrap_or_else(|| Ok(Vec::new()))
}
pub(crate) fn set_extensions(
    nodes: &mut Vec<Chunk>,
    plugins: &[Chunk],
    version: u32,
) -> Result<()> {
    let payload = write_chunks(plugins)?;
    if let Some(ext) = nodes.iter_mut().find(|c| c.kind == 3) {
        ext.payload = payload;
    } else {
        nodes.push(Chunk {
            kind: 3,
            version,
            payload,
        });
    }
    Ok(())
}

pub(crate) fn replace_kind(nodes: &mut Vec<Chunk>, kind: u32, new: Vec<Chunk>) {
    let mut new = new.into_iter();
    let mut out = Vec::new();
    for node in nodes.drain(..) {
        if node.kind == kind {
            if let Some(mut next) = new.next() {
                next.version = node.version;
                out.push(next);
            }
        } else {
            out.push(node);
        }
    }
    let index = out.iter().rposition(|c| c.kind == 3).unwrap_or(out.len());
    out.splice(index..index, new);
    *nodes = out;
}

impl TextureRef {
    pub fn new(name: String, mask: String, filter_flags: u32, version: u32) -> Self {
        let mut name_bytes = name.as_bytes().to_vec();
        name_bytes.push(0);
        name_bytes.resize(name_bytes.len().div_ceil(4) * 4, 0);
        let mut mask_bytes = mask.as_bytes().to_vec();
        mask_bytes.push(0);
        mask_bytes.resize(mask_bytes.len().div_ceil(4) * 4, 0);
        Self {
            name,
            mask,
            filter_flags,
            nodes: vec![
                Chunk {
                    kind: 1,
                    version,
                    payload: filter_flags.to_le_bytes().to_vec(),
                },
                Chunk {
                    kind: 2,
                    version,
                    payload: name_bytes,
                },
                Chunk {
                    kind: 2,
                    version,
                    payload: mask_bytes,
                },
                Chunk {
                    kind: 3,
                    version,
                    payload: Vec::new(),
                },
            ],
        }
    }
    pub(crate) fn parse(node: &Chunk) -> Result<Self> {
        let nodes = parse_chunks(&node.payload)?;
        let index = struct_index(&nodes)?;
        let mut reader = Reader::new(&nodes[index].payload);
        let filter_flags = reader.u32()?;
        let strings: Vec<_> = nodes.iter().filter(|c| c.kind == 2).collect();
        ensure!(!strings.is_empty(), "texture reference has no name");
        Ok(Self {
            name: text(&strings[0].payload)?,
            mask: if let Some(mask) = strings.get(1) {
                text(&mask.payload)?
            } else {
                String::new()
            },
            filter_flags,
            nodes,
        })
    }
    pub(crate) fn build(&self, version: u32) -> Result<Chunk> {
        let mut nodes = self.nodes.clone();
        let index = struct_index(&nodes)?;
        nodes[index].payload[..4].copy_from_slice(&self.filter_flags.to_le_bytes());
        let mut strings = nodes.iter_mut().filter(|c| c.kind == 2);
        for name in [&self.name, &self.mask] {
            ensure!(!name.contains('\0'), "DFF texture reference contains NUL");
            if let Some(node) = strings.next() {
                if text(&node.payload)? != *name {
                    let mut bytes = name.as_bytes().to_vec();
                    bytes.push(0);
                    bytes.resize(bytes.len().div_ceil(4) * 4, 0);
                    node.payload = bytes;
                }
            } else {
                ensure!(name.is_empty(), "texture reference lacks a mask field");
            }
        }
        Ok(Chunk {
            kind: 6,
            version,
            payload: write_chunks(&nodes)?,
        })
    }
}

impl Material {
    pub fn effects(&self) -> Result<Option<super::plugins::MaterialEffects>> {
        self.plugins
            .iter()
            .find(|p| p.kind == 0x120)
            .map(|p| super::plugins::MaterialEffects::parse(&p.payload))
            .transpose()
    }
    pub fn reflection(&self) -> Result<Option<super::plugins::ReflectionMaterial>> {
        self.plugins
            .iter()
            .find(|p| p.kind == 0x253f2fc)
            .map(|p| super::plugins::ReflectionMaterial::parse(&p.payload))
            .transpose()
    }
    pub fn specular(&self) -> Result<Option<super::plugins::SpecularMaterial>> {
        self.plugins
            .iter()
            .find(|p| p.kind == 0x253f2f6)
            .map(|p| super::plugins::SpecularMaterial::parse(&p.payload))
            .transpose()
    }
    pub fn new(version: u32) -> Self {
        let mut data = vec![0; 28];
        data[4..8].fill(255);
        for at in [16, 20, 24] {
            data[at..at + 4].copy_from_slice(&1f32.to_le_bytes());
        }
        Self {
            flags: 0,
            color: [255; 4],
            texture: None,
            surface: [1.; 3],
            plugins: Vec::new(),
            nodes: vec![
                Chunk {
                    kind: 1,
                    version,
                    payload: data,
                },
                Chunk {
                    kind: 3,
                    version,
                    payload: Vec::new(),
                },
            ],
        }
    }
    fn parse(node: &Chunk) -> Result<Self> {
        let nodes = parse_chunks(&node.payload)?;
        let index = struct_index(&nodes)?;
        let mut r = Reader::new(&nodes[index].payload);
        let flags = r.u32()?;
        let color = r.take(4)?.try_into().unwrap();
        let _unused = r.u32()?;
        let textured = r.u32()?;
        let surface = if r.tail().len() >= 12 {
            [r.f32()?, r.f32()?, r.f32()?]
        } else {
            [1., 1., 1.]
        };
        let texture = nodes
            .iter()
            .find(|c| c.kind == 6)
            .map(TextureRef::parse)
            .transpose()?;
        ensure!(
            textured == 0 || texture.is_some(),
            "textured material has no texture reference"
        );
        let plugins = extensions(&nodes)?;
        Ok(Self {
            flags,
            color,
            texture,
            surface,
            plugins,
            nodes,
        })
    }
    fn build(&self, version: u32) -> Result<Chunk> {
        ensure!(
            self.surface.iter().all(|v| v.is_finite()),
            "non-finite material surface value"
        );
        let mut nodes = self.nodes.clone();
        let index = struct_index(&nodes)?;
        let data = &mut nodes[index].payload;
        data[..4].copy_from_slice(&self.flags.to_le_bytes());
        data[4..8].copy_from_slice(&self.color);
        let old = u32::from_le_bytes(data[12..16].try_into().unwrap());
        let textured = if self.texture.is_some() {
            old.max(1)
        } else {
            0
        };
        data[12..16].copy_from_slice(&textured.to_le_bytes());
        if data.len() >= 28 {
            for (i, value) in self.surface.iter().enumerate() {
                data[16 + i * 4..20 + i * 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        replace_kind(
            &mut nodes,
            6,
            self.texture
                .as_ref()
                .map(|t| t.build(version))
                .transpose()?
                .into_iter()
                .collect(),
        );
        set_extensions(&mut nodes, &self.plugins, version)?;
        Ok(Chunk {
            kind: 7,
            version,
            payload: write_chunks(&nodes)?,
        })
    }
}

impl Geometry {
    pub fn effects2d(&self) -> Result<Option<super::plugins::Effects2d>> {
        self.plugins
            .iter()
            .find(|p| p.kind == 0x253f2f8)
            .map(|p| super::plugins::Effects2d::parse(&p.payload))
            .transpose()
    }
    pub fn parse(node: &Chunk) -> Result<Self> {
        let nodes = parse_chunks(&node.payload)?;
        let data = &nodes[struct_index(&nodes)?].payload;
        let mut r = Reader::new(data);
        let flags = r.u32()?;
        ensure!(
            flags & 0x01000000 == 0,
            "console/native geometry is not a PC vertex stream (flags 0x{flags:08X})"
        );
        let triangles = r.u32()? as usize;
        let vertices = r.u32()? as usize;
        let morphs = r.u32()? as usize;
        ensure!(
            vertices <= 4_000_000 && triangles <= 8_000_000 && morphs > 0 && morphs <= 256,
            "geometry counts exceed limits"
        );
        let legacy_surface = if super::chunk::version_number(node.version) < 0x34000 {
            r.take(12)?.to_vec()
        } else {
            Vec::new()
        };
        let colors = if flags & 8 != 0 {
            r.take(vertices.checked_mul(4).context("color count overflow")?)?
                .chunks_exact(4)
                .map(|p| p.try_into().unwrap())
                .collect()
        } else {
            Vec::new()
        };
        let uv_count = if flags & 0xff0000 != 0 {
            ((flags >> 16) & 255) as usize
        } else if flags & 0x80 != 0 {
            2
        } else if flags & 4 != 0 {
            1
        } else {
            0
        };
        ensure!(uv_count <= 8, "too many UV sets");
        let mut uv_sets = Vec::new();
        for _ in 0..uv_count {
            let mut set = Vec::new();
            for _ in 0..vertices {
                set.push([r.raw_f32()?, r.raw_f32()?]);
            }
            uv_sets.push(set);
        }
        let mut faces = Vec::new();
        for _ in 0..triangles {
            let b = r.u16()? as u32;
            let a = r.u16()? as u32;
            let material = r.u16()? as u32;
            let c = r.u16()? as u32;
            faces.push(Triangle {
                indices: [a, b, c],
                material,
            });
        }
        let mut morph_targets = Vec::new();
        for _ in 0..morphs {
            let center = r.vec3()?;
            let radius = r.f32()?;
            let positions_flag = r.u32()?;
            let normals_flag = r.u32()?;
            let mut positions = Vec::new();
            let mut normals = Vec::new();
            if positions_flag != 0 {
                for _ in 0..vertices {
                    positions.push(r.vec3()?);
                }
            }
            if normals_flag != 0 {
                for _ in 0..vertices {
                    normals.push(r.vec3()?);
                }
            }
            morph_targets.push(MorphTarget {
                center,
                radius,
                positions,
                normals,
                positions_flag,
                normals_flag,
            });
        }
        let trailing = r.tail().to_vec();
        let material_node = nodes
            .iter()
            .find(|c| c.kind == 8)
            .context("geometry has no material list")?;
        let material_nodes = parse_chunks(&material_node.payload)?;
        let mut mr = Reader::new(&material_nodes[struct_index(&material_nodes)?].payload);
        let count = mr.u32()? as usize;
        ensure!(count <= 65535, "too many geometry materials");
        let mut material_refs = Vec::new();
        for _ in 0..count {
            material_refs.push(mr.i32()?);
        }
        let mut unique = material_nodes.iter().filter(|c| c.kind == 7);
        let mut materials: Vec<Material> = Vec::new();
        for reference in &material_refs {
            if *reference < 0 {
                ensure!(*reference == -1, "invalid material-list reference");
                materials.push(Material::parse(
                    unique.next().context("material list truncated")?,
                )?);
            } else {
                materials.push(
                    materials
                        .get(*reference as usize)
                        .context("material sharing refers forward/outside list")?
                        .clone(),
                );
            }
        }
        ensure!(
            unique.next().is_none(),
            "material list has unreferenced material records"
        );
        let plugins = extensions(&nodes)?;
        let skin = plugins
            .iter()
            .find(|p| p.kind == 0x116)
            .map(|p| Skin::parse(&p.payload, vertices))
            .transpose()?;
        let bin_mesh = plugins
            .iter()
            .find(|p| p.kind == 0x50e)
            .map(|p| BinMesh::parse(&p.payload, vertices))
            .transpose()?;
        let night_colors = plugins
            .iter()
            .find(|p| p.kind == 0x253f2f9)
            .map(|p| NightColors::parse(&p.payload, vertices))
            .transpose()?;
        let result = Self {
            flags,
            colors,
            uv_sets,
            triangles: faces.clone(),
            original_triangles: faces,
            morph_targets: morph_targets.clone(),
            original_morphs: morph_targets,
            materials,
            plugins,
            skin,
            bin_mesh,
            night_colors,
            nodes,
            material_nodes,
            material_refs,
            original_vertex_count: vertices,
            original_uv_count: uv_count,
            legacy_surface,
            trailing,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn vertex_count(&self) -> usize {
        self.morph_targets
            .first()
            .filter(|m| m.positions_flag != 0 || !m.positions.is_empty())
            .map_or(self.original_vertex_count, |m| m.positions.len())
    }
    pub fn validate(&self) -> Result<()> {
        let count = self.vertex_count();
        ensure!(
            count <= 4_000_000 && !self.morph_targets.is_empty() && self.morph_targets.len() <= 256,
            "invalid geometry counts"
        );
        ensure!(self.uv_sets.len() <= 8, "too many UV sets");
        ensure!(
            self.colors.is_empty() || self.colors.len() == count,
            "prelight colors do not match vertices"
        );
        for set in &self.uv_sets {
            ensure!(set.len() == count, "UV data does not match geometry");
        }
        for morph in &self.morph_targets {
            ensure!(
                (morph.positions.is_empty() || morph.positions.len() == count)
                    && (morph.normals.is_empty() || morph.normals.len() == count),
                "morph vertex arrays disagree"
            );
            ensure!(
                morph
                    .positions
                    .iter()
                    .chain(&morph.normals)
                    .chain(std::iter::once(&morph.center))
                    .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
                    && morph.radius.is_finite(),
                "non-finite geometry values"
            );
        }
        for face in &self.triangles {
            ensure!(
                face.indices.iter().all(|i| (*i as usize) < count),
                "triangle vertex index out of bounds"
            );
            ensure!(
                (face.material as usize) < self.materials.len(),
                "triangle material index out of bounds"
            );
        }
        if let Some(skin) = &self.skin {
            skin.validate(count)?;
        }
        Ok(())
    }

    pub fn build(&self, version: u32) -> Result<Chunk> {
        self.validate()?;
        let count = self.vertex_count();
        let topology = count != self.original_vertex_count
            || self.triangles != self.original_triangles
            || self.uv_sets.len() != self.original_uv_count;
        if topology {
            ensure!(self.plugins.iter().all(|p|matches!(p.kind,0x50e|0x116|0x253f2f9)),"geometry edit invalidates an unknown dependent plugin; remove it explicitly or retain topology");
            ensure!(
                self.trailing.iter().all(|b| *b == 0),
                "geometry edit invalidates unknown trailing vertex data"
            );
        }
        let mut flags = self.flags;
        if self.colors.is_empty() {
            flags &= !8;
        } else {
            flags |= 8;
        }
        flags = (flags & !0xff0000) | ((self.uv_sets.len() as u32) << 16);
        // Retain equivalent source flag spellings on unchanged UV layouts.
        if self.uv_sets.len() == self.original_uv_count {
            flags = (flags & !0xff0000) | (self.flags & 0xff0000);
        } else {
            flags &= !(4 | 0x80);
            if self.uv_sets.len() == 1 {
                flags |= 4;
            } else if self.uv_sets.len() > 1 {
                flags |= 0x80;
            }
        }
        let mut data = Vec::new();
        for v in [
            flags,
            self.triangles.len() as u32,
            count as u32,
            self.morph_targets.len() as u32,
        ] {
            put_u32(&mut data, v);
        }
        data.extend_from_slice(&self.legacy_surface);
        for color in &self.colors {
            data.extend(color);
        }
        for set in &self.uv_sets {
            for uv in set {
                put_f32(&mut data, uv[0]);
                put_f32(&mut data, uv[1]);
            }
        }
        for face in &self.triangles {
            for v in [
                face.indices[1],
                face.indices[0],
                face.material,
                face.indices[2],
            ] {
                data.extend_from_slice(
                    &u16::try_from(v)
                        .context("DFF face index exceeds u16")?
                        .to_le_bytes(),
                );
            }
        }
        for (index, morph) in self.morph_targets.iter().enumerate() {
            let changed = self
                .original_morphs
                .get(index)
                .is_none_or(|original| original.positions != morph.positions);
            let (center, radius) = if changed && !morph.positions.is_empty() {
                bounds(&morph.positions)
            } else {
                (morph.center, morph.radius)
            };
            put_vec3(&mut data, center);
            put_f32(&mut data, radius);
            put_u32(
                &mut data,
                if morph.positions.is_empty() {
                    if count == 0 {
                        morph.positions_flag
                    } else {
                        0
                    }
                } else {
                    morph.positions_flag.max(1)
                },
            );
            put_u32(
                &mut data,
                if morph.normals.is_empty() {
                    if count == 0 {
                        morph.normals_flag
                    } else {
                        0
                    }
                } else {
                    morph.normals_flag.max(1)
                },
            );
            for position in &morph.positions {
                put_vec3(&mut data, *position);
            }
            for normal in &morph.normals {
                put_vec3(&mut data, *normal);
            }
        }
        data.extend_from_slice(&self.trailing);
        let mut nodes = self.nodes.clone();
        let index = struct_index(&nodes)?;
        nodes[index].payload = data;
        let mut material_nodes = self.material_nodes.clone();
        let header_index = struct_index(&material_nodes)?;
        let mut header = Vec::new();
        put_u32(&mut header, self.materials.len() as u32);
        let mut material_chunks = Vec::new();
        for (index, material) in self.materials.iter().enumerate() {
            let reference = self.material_refs.get(index).copied().unwrap_or(-1);
            if reference >= 0
                && (reference as usize) < index
                && self.materials.get(reference as usize) == Some(material)
            {
                put_u32(&mut header, reference as u32);
            } else {
                put_u32(&mut header, u32::MAX);
                material_chunks.push(material.build(version)?);
            }
        }
        material_nodes[header_index].payload = header;
        replace_kind(&mut material_nodes, 7, material_chunks);
        let list_index = nodes.iter().position(|c| c.kind == 8).unwrap();
        nodes[list_index].payload = write_chunks(&material_nodes)?;
        let mut plugins = self.plugins.clone();
        let bin = if topology {
            Some(BinMesh::from_triangles(&self.triangles))
        } else {
            self.bin_mesh.clone()
        };
        for (kind, payload) in [
            (0x50e, bin.as_ref().map(BinMesh::build).transpose()?),
            (
                0x116,
                self.skin.as_ref().map(|s| s.build(count)).transpose()?,
            ),
            (
                0x253f2f9,
                self.night_colors
                    .as_ref()
                    .map(|s| s.build(count))
                    .transpose()?,
            ),
        ] {
            replace_kind(
                &mut plugins,
                kind,
                payload
                    .map(|payload| Chunk {
                        kind,
                        version,
                        payload,
                    })
                    .into_iter()
                    .collect(),
            );
        }
        set_extensions(&mut nodes, &plugins, version)?;
        Ok(Chunk {
            kind: 0xf,
            version,
            payload: write_chunks(&nodes)?,
        })
    }

    pub fn effective_triangles(&self) -> Vec<Triangle> {
        if !self.triangles.is_empty() {
            return self.triangles.clone();
        }
        let mut out = Vec::new();
        if let Some(bin) = &self.bin_mesh {
            for mesh in &bin.meshes {
                if bin.flags & 1 == 0 {
                    for indices in mesh.indices.chunks_exact(3) {
                        out.push(Triangle {
                            indices: indices.try_into().unwrap(),
                            material: mesh.material,
                        });
                    }
                } else {
                    let mut strip = Vec::new();
                    for &index in &mesh.indices {
                        if index == u32::MAX || index == 0xffff {
                            strip.clear();
                            continue;
                        }
                        strip.push(index);
                        if strip.len() >= 3 {
                            let n = strip.len();
                            let mut face = [strip[n - 3], strip[n - 2], strip[n - 1]];
                            if n % 2 == 0 {
                                face.swap(0, 1);
                            }
                            if face[0] != face[1] && face[1] != face[2] && face[2] != face[0] {
                                out.push(Triangle {
                                    indices: face,
                                    material: mesh.material,
                                });
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

pub(crate) fn bounds(positions: &[Vec3]) -> (Vec3, f32) {
    let mut min = positions[0];
    let mut max = min;
    for p in positions {
        min = min.min(*p);
        max = max.max(*p);
    }
    let center = (min + max) * 0.5;
    let radius = positions
        .iter()
        .map(|p| (*p - center).length())
        .fold(0f32, f32::max);
    (center, radius)
}
