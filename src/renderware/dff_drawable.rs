use super::dff::Dff;
use crate::{
    rage_joaat, Drawable, DrawableBounds, DrawableEntry, DrawableGeometry, DrawableLod,
    DrawableModel, IndexBuffer, LodLevel, ShaderFx, ShaderGroup, ShaderParameter,
    ShaderParameterValue, Vec3, VertexBuffer, VertexBufferLayout, VertexComponent,
    VertexComponentType, VertexDeclaration, VertexSemantic,
};
use anyhow::{ensure, Context, Result};

/// Rasterizer adapter only. Structured rig export uses the original frame and
/// skin data rather than trying to recover it from these flattened vertices.
pub(crate) fn drawables(dff: &Dff) -> Result<Vec<DrawableEntry>> {
    dff.validate()?;
    let mut out = Vec::new();
    for (atomic_index, atomic) in dff.atomics.iter().enumerate() {
        if atomic.flags & 4 == 0 {
            continue;
        }
        let source = &dff.geometries[atomic.geometry];
        let morph = source
            .morph_targets
            .first()
            .context("geometry has no morph target")?;
        ensure!(
            !morph.positions.is_empty(),
            "geometry has no renderable position stream"
        );
        let world = dff.frame_world(atomic.frame)?;
        let inverse = world.inverse();
        let positions: Vec<_> = morph
            .positions
            .iter()
            .map(|p| world.transform_point(*p).xyz())
            .collect();
        let triangles = source.effective_triangles();
        if triangles.is_empty() {
            continue;
        }
        let used_all: std::collections::BTreeSet<_> =
            triangles.iter().flat_map(|t| t.indices).collect();
        let mut min = positions[*used_all.first().unwrap() as usize];
        let mut max = min;
        for &index in &used_all {
            let p = positions[index as usize];
            min = min.min(p);
            max = max.max(p);
        }
        let center = (min + max) * 0.5;
        let radius = used_all
            .iter()
            .map(|i| (positions[*i as usize] - center).length())
            .fold(0f32, f32::max);
        let mut shaders = Vec::new();
        for material in &source.materials {
            let mut parameters = Vec::new();
            if let Some(texture) = &material.texture {
                parameters.push(ShaderParameter {
                    name_hash: crate::DIFFUSE_SAMPLER,
                    data_type: 0,
                    data_pointer: 0,
                    value: ShaderParameterValue::Texture {
                        name: texture.name.clone(),
                        name_hash: rage_joaat(&texture.name.to_lowercase()),
                    },
                });
            }
            let alpha = material.color[3] < 255;
            shaders.push(ShaderFx {
                name_hash: rage_joaat(if alpha { "alpha" } else { "default" }),
                file_name_hash: rage_joaat(if alpha { "alpha.sps" } else { "default.sps" }),
                render_bucket: if alpha { 1 } else { 0 },
                render_bucket_mask: if alpha { 2 } else { 1 },
                parameter_count: parameters.len() as u8,
                texture_parameter_count: parameters.len() as u8,
                parameters,
            });
        }
        let mut geometries = Vec::new();
        for face in &triangles {
            for &index in &face.indices {
                ensure!(
                    source.uv_sets.iter().all(|set| set
                        .get(index as usize)
                        .is_some_and(|uv| uv.iter().all(|v| v.is_finite()))),
                    "geometry has a non-finite UV on referenced vertex {index}"
                );
            }
        }
        for (material_index, material) in source.materials.iter().enumerate() {
            let selected: Vec<_> = triangles
                .iter()
                .filter(|t| t.material as usize == material_index)
                .collect();
            if selected.is_empty() {
                continue;
            }
            let used: Vec<_> = selected
                .iter()
                .flat_map(|t| t.indices)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            ensure!(
                used.len() <= 65535,
                "renderer adapter needs split geometry above 65535 referenced vertices"
            );
            let remap: std::collections::BTreeMap<_, _> = used
                .iter()
                .enumerate()
                .map(|(i, v)| (*v, i as u32))
                .collect();
            let components = components(source.uv_sets.len(), source.skin.is_some());
            let stride = components.iter().map(|c| c.size as u16).sum::<u16>();
            let mut data = Vec::new();
            for &source_index in &used {
                let index = source_index as usize;
                let position = &positions[index];
                for component in &components {
                    match component.semantic {
                        VertexSemantic::Position => {
                            for value in [position.x, position.y, position.z] {
                                data.extend_from_slice(&value.to_le_bytes());
                            }
                        }
                        VertexSemantic::Normal => {
                            let n = morph.normals.get(index).copied().unwrap_or(Vec3::Z);
                            let normal = Vec3::new(
                                n.x * inverse.0[0] + n.y * inverse.0[1] + n.z * inverse.0[2],
                                n.x * inverse.0[4] + n.y * inverse.0[5] + n.z * inverse.0[6],
                                n.x * inverse.0[8] + n.y * inverse.0[9] + n.z * inverse.0[10],
                            )
                            .normalize();
                            for value in [normal.x, normal.y, normal.z] {
                                data.extend_from_slice(&value.to_le_bytes());
                            }
                        }
                        VertexSemantic::Colour0 => {
                            let mut color = source.colors.get(index).copied().unwrap_or([255; 4]);
                            for (value, factor) in color.iter_mut().zip(material.color) {
                                *value = ((*value as u16 * factor as u16 + 127) / 255) as u8;
                            }
                            data.extend(color);
                        }
                        VertexSemantic::Colour1 => {
                            data.extend(
                                source
                                    .night_colors
                                    .as_ref()
                                    .and_then(|n| n.colors.get(index))
                                    .copied()
                                    .unwrap_or([255; 4]),
                            );
                        }
                        VertexSemantic::BlendWeights => {
                            for value in source.skin.as_ref().unwrap().weights[index] {
                                data.extend_from_slice(&value.to_le_bytes());
                            }
                        }
                        VertexSemantic::BlendIndices => {
                            data.extend(source.skin.as_ref().unwrap().indices[index])
                        }
                        _ => {
                            let set = component.semantic_index as usize - 6;
                            for value in source.uv_sets[set][index] {
                                data.extend_from_slice(&value.to_le_bytes());
                            }
                        }
                    }
                }
            }
            let mut flags = 0u32;
            let mut types = 0u64;
            for component in &components {
                flags |= 1 << component.semantic_index;
                types |= (match component.component_type {
                    VertexComponentType::Float3 => 6,
                    VertexComponentType::Float2 => 5,
                    VertexComponentType::Float4 => 7,
                    VertexComponentType::UByte4 => 8,
                    _ => 9,
                } as u64)
                    << (component.semantic_index * 4);
            }
            let declaration = VertexDeclaration {
                flags,
                stride,
                unknown_6h: 0,
                count: components.len() as u8,
                types,
                components,
            };
            let mut indices: Vec<u32> = selected
                .iter()
                .flat_map(|t| t.indices)
                .map(|i| remap[&i])
                .collect();
            let a = Vec3::new(world.0[0], world.0[1], world.0[2]);
            let b = Vec3::new(world.0[4], world.0[5], world.0[6]);
            let c = Vec3::new(world.0[8], world.0[9], world.0[10]);
            if a.cross(b).dot(c) < 0. {
                for face in indices.chunks_exact_mut(3) {
                    face.swap(1, 2);
                }
            }
            geometries.push(DrawableGeometry {
                shader_id: material_index as u16,
                indices_count: indices.len() as u32,
                triangles_count: (indices.len() / 3) as u32,
                vertices_count: used.len() as u16,
                vertex_stride: stride,
                vertex_buffer: Some(VertexBuffer {
                    vertex_stride: stride,
                    vertex_count: used.len() as u32,
                    data_pointer: 0,
                    info_pointer: 0,
                    declaration: Some(declaration),
                    data,
                    layout: VertexBufferLayout::Legacy,
                }),
                index_buffer: Some(IndexBuffer {
                    indices_count: indices.len() as u32,
                    indices_pointer: 0,
                    indices,
                }),
            });
        }
        let frame = &dff.frames[atomic.frame];
        let name = if frame.name.is_empty() {
            format!("atomic_{atomic_index}")
        } else {
            frame.name.clone()
        };
        let hash = rage_joaat(&name.to_lowercase());
        out.push(DrawableEntry {
            hash,
            name: name.clone(),
            drawable: Drawable {
                name,
                name_hash: hash,
                bounds: DrawableBounds {
                    center,
                    sphere_radius: radius,
                    box_min: min,
                    box_max: max,
                },
                lod_distances: [200., 100., 50., 25.],
                render_masks: [255; 4],
                shader_group: Some(ShaderGroup {
                    textures: Vec::new(),
                    shaders,
                }),
                lods: vec![DrawableLod {
                    level: LodLevel::High,
                    models: vec![DrawableModel {
                        skeleton_binding: 0,
                        render_mask_flags: 255,
                        shader_mapping: geometries.iter().map(|g| g.shader_id).collect(),
                        geometries,
                    }],
                }],
            },
        });
    }
    Ok(out)
}

fn components(uvs: usize, skin: bool) -> Vec<VertexComponent> {
    let mut specs = vec![(
        VertexSemantic::Position,
        0,
        VertexComponentType::Float3,
        12,
        3,
    )];
    if skin {
        specs.push((
            VertexSemantic::BlendWeights,
            1,
            VertexComponentType::Float4,
            16,
            4,
        ));
        specs.push((
            VertexSemantic::BlendIndices,
            2,
            VertexComponentType::UByte4,
            4,
            4,
        ));
    }
    specs.push((
        VertexSemantic::Normal,
        3,
        VertexComponentType::Float3,
        12,
        3,
    ));
    specs.push((
        VertexSemantic::Colour0,
        4,
        VertexComponentType::Colour,
        4,
        4,
    ));
    specs.push((
        VertexSemantic::Colour1,
        5,
        VertexComponentType::Colour,
        4,
        4,
    ));
    let semantics = [
        VertexSemantic::TexCoord0,
        VertexSemantic::TexCoord1,
        VertexSemantic::TexCoord2,
        VertexSemantic::TexCoord3,
        VertexSemantic::TexCoord4,
        VertexSemantic::TexCoord5,
        VertexSemantic::TexCoord6,
        VertexSemantic::TexCoord7,
    ];
    for (index, semantic) in semantics.iter().take(uvs).enumerate() {
        specs.push((
            *semantic,
            index as u8 + 6,
            VertexComponentType::Float2,
            8,
            2,
        ));
    }
    let mut offset = 0;
    specs
        .into_iter()
        .map(
            |(semantic, semantic_index, component_type, size, component_count)| {
                let component = VertexComponent {
                    semantic,
                    semantic_index,
                    component_type,
                    offset,
                    size,
                    component_count,
                };
                offset += size as u16;
                component
            },
        )
        .collect()
}
