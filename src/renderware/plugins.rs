//! Typed plugin records with unmodeled trailing fields retained verbatim.
use super::reader::{put_f32, put_u32, put_vec3, Reader};
use crate::Mat4;
use anyhow::{ensure, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct HAnimNode {
    pub id: i32,
    pub index: u32,
    pub flags: u32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct HAnim {
    pub version: u32,
    pub id: i32,
    pub flags: u32,
    pub keyframe_size: u32,
    pub nodes: Vec<HAnimNode>,
    trailing: Vec<u8>,
}
impl HAnim {
    pub fn new(id: i32, nodes: Vec<HAnimNode>) -> Self {
        Self {
            version: 0x100,
            id,
            flags: 0,
            keyframe_size: 36,
            nodes,
            trailing: Vec::new(),
        }
    }
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let version = r.u32()?;
        let id = r.i32()?;
        let count = r.u32()? as usize;
        ensure!(count <= 65535, "too many HAnim nodes");
        let (flags, keyframe_size) = if count > 0 {
            (r.u32()?, r.u32()?)
        } else {
            (0, 0)
        };
        let mut nodes = Vec::new();
        for _ in 0..count {
            nodes.push(HAnimNode {
                id: r.i32()?,
                index: r.u32()?,
                flags: r.u32()?,
            });
        }
        Ok(Self {
            version,
            id,
            flags,
            keyframe_size,
            nodes,
            trailing: r.tail().to_vec(),
        })
    }
    pub fn build(&self) -> Result<Vec<u8>> {
        ensure!(self.nodes.len() <= 65535, "too many HAnim nodes");
        let mut out = Vec::new();
        for n in [self.version, self.id as u32, self.nodes.len() as u32] {
            put_u32(&mut out, n);
        }
        if !self.nodes.is_empty() {
            put_u32(&mut out, self.flags);
            put_u32(&mut out, self.keyframe_size);
        }
        for n in &self.nodes {
            for v in [n.id as u32, n.index, n.flags] {
                put_u32(&mut out, v);
            }
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skin {
    pub used_bones: Vec<u8>,
    pub max_weights: u8,
    pub reserved: u8,
    pub indices: Vec<[u8; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub inverse_bind_matrices: Vec<Mat4>,
    legacy_markers: Vec<u32>,
    matrix_padding: Vec<[u32; 4]>,
    trailing: Vec<u8>,
}
impl Skin {
    pub fn new(
        indices: Vec<[u8; 4]>,
        weights: Vec<[f32; 4]>,
        inverse_bind_matrices: Vec<Mat4>,
    ) -> Result<Self> {
        let used_bones = indices
            .iter()
            .zip(&weights)
            .flat_map(|(i, w)| i.iter().zip(w).filter(|(_, w)| **w > 0.).map(|(i, _)| *i))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let max_weights = weights
            .iter()
            .map(|w| w.iter().filter(|w| **w > 0.).count() as u8)
            .max()
            .unwrap_or(1)
            .max(1);
        let matrix_padding = vec![[0, 0, 0, 1f32.to_bits()]; inverse_bind_matrices.len()];
        let skin = Self {
            used_bones,
            max_weights,
            reserved: 0,
            indices,
            weights,
            inverse_bind_matrices,
            matrix_padding,
            legacy_markers: Vec::new(),
            trailing: vec![0; 12],
        };
        skin.validate(skin.indices.len())?;
        Ok(skin)
    }
    pub fn parse(data: &[u8], vertices: usize) -> Result<Self> {
        let mut r = Reader::new(data);
        let bones = r.u8()? as usize;
        let used = r.u8()? as usize;
        let max_weights = r.u8()?;
        let reserved = r.u8()?;
        ensure!(
            bones > 0 && max_weights <= 4,
            "invalid skin bone/weight count"
        );
        let used_bones = r.take(used)?.to_vec();
        let mut indices = Vec::new();
        for row in r
            .take(
                vertices
                    .checked_mul(4)
                    .ok_or_else(|| anyhow::anyhow!("skin index count overflow"))?,
            )?
            .chunks_exact(4)
        {
            indices.push(row.try_into().unwrap());
        }
        let mut weights = Vec::new();
        for _ in 0..vertices {
            weights.push([r.f32()?, r.f32()?, r.f32()?, r.f32()?]);
        }
        let mut matrices = Vec::new();
        let mut legacy_markers = Vec::new();
        let mut matrix_padding = Vec::new();
        for _ in 0..bones {
            if max_weights == 0 {
                legacy_markers.push(r.u32()?);
            }
            let (matrix, padding) = r.rw_matrix()?;
            matrices.push(matrix);
            matrix_padding.push(padding);
        }
        let result = Self {
            used_bones,
            max_weights,
            reserved,
            indices,
            weights,
            inverse_bind_matrices: matrices,
            legacy_markers,
            matrix_padding,
            trailing: r.tail().to_vec(),
        };
        result.validate(vertices)?;
        Ok(result)
    }
    pub fn validate(&self, vertices: usize) -> Result<()> {
        let bones = self.inverse_bind_matrices.len();
        ensure!(bones > 0 && bones <= 255, "invalid skin bone count");
        ensure!(
            self.indices.len() == vertices && self.weights.len() == vertices,
            "skin vertex arrays do not match geometry"
        );
        ensure!(
            self.used_bones.iter().all(|b| (*b as usize) < bones),
            "skin used-bone index out of bounds"
        );
        for (indices, weights) in self.indices.iter().zip(&self.weights) {
            ensure!(
                weights
                    .iter()
                    .all(|w| w.is_finite() && *w >= 0. && *w <= 1.001),
                "invalid skin weight"
            );
            ensure!(
                weights.iter().sum::<f32>() <= 1.05,
                "skin weights sum to more than one"
            );
            for (index, weight) in indices.iter().zip(weights) {
                ensure!(
                    *weight == 0. || (*index as usize) < bones,
                    "skin bone index out of bounds"
                );
            }
        }
        ensure!(
            self.inverse_bind_matrices
                .iter()
                .all(|m| m.0.iter().all(|f| f.is_finite())),
            "non-finite inverse bind matrix"
        );
        Ok(())
    }
    pub fn build(&self, vertices: usize) -> Result<Vec<u8>> {
        self.validate(vertices)?;
        ensure!(self.used_bones.len() <= 255, "too many used bones");
        let mut out = vec![
            self.inverse_bind_matrices.len() as u8,
            self.used_bones.len() as u8,
            self.max_weights,
            self.reserved,
        ];
        out.extend_from_slice(&self.used_bones);
        for row in &self.indices {
            out.extend_from_slice(row);
        }
        for row in &self.weights {
            for value in row {
                put_f32(&mut out, *value);
            }
        }
        for (i, matrix) in self.inverse_bind_matrices.iter().enumerate() {
            if self.max_weights == 0 {
                put_u32(&mut out, *self.legacy_markers.get(i).unwrap_or(&0xdeaddead));
            }
            ensure!(
                matrix.0[3] == 0. && matrix.0[7] == 0. && matrix.0[11] == 0. && matrix.0[15] == 1.,
                "skin inverse bind matrix must be affine"
            );
            let padding = self
                .matrix_padding
                .get(i)
                .copied()
                .unwrap_or([0, 0, 0, 1f32.to_bits()]);
            for group in 0..4 {
                for axis in 0..3 {
                    put_f32(&mut out, matrix.0[group * 4 + axis]);
                }
                put_u32(&mut out, padding[group]);
            }
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinMeshGroup {
    pub material: u32,
    pub indices: Vec<u32>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct BinMesh {
    pub flags: u32,
    pub indices_count: u32,
    pub meshes: Vec<BinMeshGroup>,
    trailing: Vec<u8>,
}
impl BinMesh {
    pub fn parse(data: &[u8], vertices: usize) -> Result<Self> {
        let mut r = Reader::new(data);
        let flags = r.u32()?;
        let count = r.u32()? as usize;
        let indices_count = r.u32()?;
        ensure!(count <= 65535, "too many bin meshes");
        let mut meshes = Vec::new();
        let mut total = 0usize;
        for _ in 0..count {
            let length = r.u32()? as usize;
            let material = r.u32()?;
            ensure!(length <= data.len() / 4, "invalid bin mesh index count");
            let mut indices = Vec::new();
            for _ in 0..length {
                let index = r.u32()?;
                ensure!(
                    (index as usize) < vertices
                        || flags & 1 != 0 && (index == 0xffff || index == u32::MAX),
                    "bin mesh index out of bounds"
                );
                indices.push(index);
            }
            total = total
                .checked_add(length)
                .ok_or_else(|| anyhow::anyhow!("bin mesh index overflow"))?;
            meshes.push(BinMeshGroup { material, indices });
        }
        ensure!(
            total == indices_count as usize,
            "bin mesh index count disagrees with records"
        );
        Ok(Self {
            flags,
            indices_count,
            meshes,
            trailing: r.tail().to_vec(),
        })
    }
    pub fn build(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let count = self.meshes.iter().map(|m| m.indices.len()).sum::<usize>();
        ensure!(count <= u32::MAX as usize, "too many bin mesh indices");
        for value in [self.flags, self.meshes.len() as u32, count as u32] {
            put_u32(&mut out, value);
        }
        for mesh in &self.meshes {
            put_u32(&mut out, mesh.indices.len() as u32);
            put_u32(&mut out, mesh.material);
            for index in &mesh.indices {
                put_u32(&mut out, *index);
            }
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }
    pub fn from_triangles(triangles: &[super::geometry::Triangle]) -> Self {
        let mut meshes: Vec<BinMeshGroup> = Vec::new();
        for triangle in triangles {
            let index = meshes
                .iter()
                .position(|m| m.material == triangle.material)
                .unwrap_or_else(|| {
                    meshes.push(BinMeshGroup {
                        material: triangle.material,
                        indices: Vec::new(),
                    });
                    meshes.len() - 1
                });
            meshes[index].indices.extend(triangle.indices);
        }
        Self {
            flags: 0,
            indices_count: triangles.len() as u32 * 3,
            meshes,
            trailing: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NightColors {
    pub marker: u32,
    pub colors: Vec<[u8; 4]>,
    trailing: Vec<u8>,
}
impl NightColors {
    pub fn new(colors: Vec<[u8; 4]>) -> Self {
        Self {
            marker: if colors.is_empty() { 0 } else { 1 },
            colors,
            trailing: Vec::new(),
        }
    }
    pub fn parse(data: &[u8], vertices: usize) -> Result<Self> {
        let mut r = Reader::new(data);
        let marker = r.u32()?;
        let colors = if marker == 0 {
            Vec::new()
        } else {
            r.take(
                vertices
                    .checked_mul(4)
                    .ok_or_else(|| anyhow::anyhow!("night color count overflow"))?,
            )?
            .chunks_exact(4)
            .map(|p| p.try_into().unwrap())
            .collect()
        };
        Ok(Self {
            marker,
            colors,
            trailing: r.tail().to_vec(),
        })
    }
    pub fn build(&self, vertices: usize) -> Result<Vec<u8>> {
        ensure!(
            self.marker == 0 && self.colors.is_empty() || self.colors.len() == vertices,
            "night colors do not match geometry"
        );
        let mut out = Vec::new();
        put_u32(&mut out, self.marker);
        for c in &self.colors {
            out.extend(c);
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionalEffectTexture {
    pub flag: u32,
    pub version: u32,
    pub texture: Option<super::geometry::TextureRef>,
}
impl OptionalEffectTexture {
    fn parse(r: &mut Reader<'_>) -> Result<Self> {
        let flag = r.u32()?;
        if flag == 0 {
            return Ok(Self {
                flag,
                version: 0,
                texture: None,
            });
        }
        let kind = r.u32()?;
        let length = r.u32()? as usize;
        let version = r.u32()?;
        ensure!(kind == 6, "material effect expected a texture chunk");
        let chunk = super::Chunk {
            kind,
            version,
            payload: r.take(length)?.to_vec(),
        };
        let texture = super::geometry::TextureRef::parse(&chunk)?;
        Ok(Self {
            flag,
            version,
            texture: Some(texture),
        })
    }
    fn write(&self, out: &mut Vec<u8>) -> Result<()> {
        put_u32(
            out,
            if self.texture.is_some() {
                self.flag.max(1)
            } else {
                0
            },
        );
        if let Some(texture) = &self.texture {
            out.extend(super::write_chunks(&[texture.build(self.version)?])?);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MaterialEffectPass {
    None,
    Bump {
        coefficient: f32,
        bump: OptionalEffectTexture,
        base: OptionalEffectTexture,
    },
    Environment {
        coefficient: f32,
        framebuffer_alpha: u32,
        texture: OptionalEffectTexture,
    },
    Dual {
        source_blend: u32,
        destination_blend: u32,
        texture: OptionalEffectTexture,
    },
    UvTransform,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialEffects {
    pub effect: u32,
    pub passes: Vec<MaterialEffectPass>,
    trailing: Vec<u8>,
}
impl MaterialEffects {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let effect = r.u32()?;
        let mut passes = Vec::new();
        for _ in 0..2 {
            if r.tail().len() < 4 {
                break;
            }
            let kind = r.u32()?;
            passes.push(match kind {
                0 => MaterialEffectPass::None,
                1 => MaterialEffectPass::Bump {
                    coefficient: r.f32()?,
                    bump: OptionalEffectTexture::parse(&mut r)?,
                    base: OptionalEffectTexture::parse(&mut r)?,
                },
                2 => MaterialEffectPass::Environment {
                    coefficient: r.f32()?,
                    framebuffer_alpha: r.u32()?,
                    texture: OptionalEffectTexture::parse(&mut r)?,
                },
                4 => MaterialEffectPass::Dual {
                    source_blend: r.u32()?,
                    destination_blend: r.u32()?,
                    texture: OptionalEffectTexture::parse(&mut r)?,
                },
                5 => MaterialEffectPass::UvTransform,
                _ => anyhow::bail!("unsupported material effect pass {kind}"),
            });
        }
        Ok(Self {
            effect,
            passes,
            trailing: r.tail().to_vec(),
        })
    }
    pub fn build(&self) -> Result<Vec<u8>> {
        ensure!(self.passes.len() <= 2, "MatFX holds at most two passes");
        let mut out = Vec::new();
        put_u32(&mut out, self.effect);
        for pass in &self.passes {
            match pass {
                MaterialEffectPass::None => put_u32(&mut out, 0),
                MaterialEffectPass::Bump {
                    coefficient,
                    bump,
                    base,
                } => {
                    ensure!(coefficient.is_finite(), "non-finite bump coefficient");
                    put_u32(&mut out, 1);
                    put_f32(&mut out, *coefficient);
                    bump.write(&mut out)?;
                    base.write(&mut out)?;
                }
                MaterialEffectPass::Environment {
                    coefficient,
                    framebuffer_alpha,
                    texture,
                } => {
                    ensure!(
                        coefficient.is_finite(),
                        "non-finite environment coefficient"
                    );
                    put_u32(&mut out, 2);
                    put_f32(&mut out, *coefficient);
                    put_u32(&mut out, *framebuffer_alpha);
                    texture.write(&mut out)?;
                }
                MaterialEffectPass::Dual {
                    source_blend,
                    destination_blend,
                    texture,
                } => {
                    put_u32(&mut out, 4);
                    put_u32(&mut out, *source_blend);
                    put_u32(&mut out, *destination_blend);
                    texture.write(&mut out)?;
                }
                MaterialEffectPass::UvTransform => put_u32(&mut out, 5),
            }
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReflectionMaterial {
    pub scale: [f32; 2],
    pub offset: [f32; 2],
    pub intensity: f32,
    trailing: Vec<u8>,
}
impl ReflectionMaterial {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        Ok(Self {
            scale: [r.f32()?, r.f32()?],
            offset: [r.f32()?, r.f32()?],
            intensity: r.f32()?,
            trailing: r.tail().to_vec(),
        })
    }
    pub fn build(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for value in self
            .scale
            .into_iter()
            .chain(self.offset)
            .chain([self.intensity])
        {
            ensure!(value.is_finite(), "non-finite reflection material value");
            put_f32(&mut out, value);
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct SpecularMaterial {
    pub coefficient: f32,
    pub texture: String,
    original_name: Vec<u8>,
    trailing: Vec<u8>,
}
impl SpecularMaterial {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let coefficient = r.f32()?;
        let original_name = r.take(24)?.to_vec();
        let texture = super::geometry::text(&original_name)?;
        Ok(Self {
            coefficient,
            texture,
            original_name,
            trailing: r.tail().to_vec(),
        })
    }
    pub fn build(&self) -> Result<Vec<u8>> {
        ensure!(
            self.coefficient.is_finite(),
            "non-finite specular coefficient"
        );
        ensure!(
            self.texture.is_ascii() && self.texture.len() <= 24 && !self.texture.contains('\0'),
            "specular texture name must fit 24 ASCII bytes"
        );
        let mut out = Vec::new();
        put_f32(&mut out, self.coefficient);
        let mut name = self.original_name.clone();
        if super::geometry::text(&name)? != self.texture {
            name.fill(0);
            name[..self.texture.len()].copy_from_slice(self.texture.as_bytes());
        }
        out.extend(name);
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Effect2dRecord {
    pub position: crate::Vec3,
    pub kind: u32,
    pub data: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Effects2d {
    pub records: Vec<Effect2dRecord>,
    trailing: Vec<u8>,
}
impl Effects2d {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let count = r.u32()? as usize;
        ensure!(count <= 100_000, "too many 2DFX records");
        let mut records = Vec::new();
        for _ in 0..count {
            let position = r.vec3()?;
            let kind = r.u32()?;
            let size = r.u32()? as usize;
            records.push(Effect2dRecord {
                position,
                kind,
                data: r.take(size)?.to_vec(),
            });
        }
        Ok(Self {
            records,
            trailing: r.tail().to_vec(),
        })
    }
    pub fn build(&self) -> Result<Vec<u8>> {
        ensure!(self.records.len() <= 100_000, "too many 2DFX records");
        let mut out = Vec::new();
        put_u32(&mut out, self.records.len() as u32);
        for record in &self.records {
            ensure!(
                record.position.x.is_finite()
                    && record.position.y.is_finite()
                    && record.position.z.is_finite(),
                "non-finite 2DFX position"
            );
            put_vec3(&mut out, record.position);
            put_u32(&mut out, record.kind);
            put_u32(&mut out, u32::try_from(record.data.len())?);
            out.extend_from_slice(&record.data);
        }
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }
}
