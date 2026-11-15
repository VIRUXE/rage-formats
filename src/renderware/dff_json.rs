//! Editable scene fields sit alongside the original chunks, retaining opaque
//! plugin data and exact bit patterns for source UV sentinels.
use super::Dff;
use crate::renderware::chunk::{hex, unhex};
use crate::renderware::{
    geometry::{Material, TextureRef, Triangle},
    plugins::{HAnim, HAnimNode, NightColors, Skin},
    Chunk,
};
use crate::{Mat4, Vec3};
use anyhow::{ensure, Context, Result};
use json::JsonValue as J;

fn fields(value: &J, names: &[&str]) -> Result<()> {
    ensure!(value.is_object(), "scene record must be an object");
    for (key, _) in value.entries() {
        ensure!(names.contains(&key), "unknown scene field '{key}'");
    }
    for key in names {
        ensure!(value.has_key(key), "scene record is missing '{key}'");
    }
    Ok(())
}
fn array<'a>(value: &'a J, max: usize) -> Result<&'a [J]> {
    if let J::Array(values) = value {
        ensure!(values.len() <= max, "scene array exceeds {max} elements");
        Ok(values)
    } else {
        anyhow::bail!("scene field must be an array")
    }
}
fn fjson(value: f32) -> J {
    if value.is_finite() {
        value.into()
    } else {
        json::object! {bits:value.to_bits()}
    }
}
fn fvalue(value: &J) -> Result<f32> {
    if value.is_object() {
        fields(value, &["bits"])?;
        Ok(f32::from_bits(
            value["bits"].as_u32().context("float bits must be u32")?,
        ))
    } else {
        // json 0.12 scales decimal mantissas directly in f32 in as_f32,
        // which can lose an ULP. Scale in f64 before rounding to the wire f32.
        let n = value.as_f64().context("expected a float")? as f32;
        ensure!(n.is_finite(), "non-finite floats require explicit bits");
        Ok(n)
    }
}
fn frow<const N: usize>(value: &J) -> Result<[f32; N]> {
    let values = array(value, N)?;
    ensure!(values.len() == N, "float row needs {N} values");
    let mut out = [0.; N];
    for (i, v) in values.iter().enumerate() {
        out[i] = fvalue(v)?;
    }
    Ok(out)
}
fn brow<const N: usize>(value: &J) -> Result<[u8; N]> {
    let values = array(value, N)?;
    ensure!(values.len() == N, "byte row needs {N} values");
    let mut out = [0; N];
    for (i, v) in values.iter().enumerate() {
        out[i] = v.as_u8().context("byte component must be 0..255")?;
    }
    Ok(out)
}
fn fj(values: impl IntoIterator<Item = f32>) -> J {
    J::Array(values.into_iter().map(fjson).collect())
}
fn bj(values: impl IntoIterator<Item = u8>) -> J {
    J::Array(values.into_iter().map(Into::into).collect())
}
fn vj(v: Vec3) -> J {
    fj([v.x, v.y, v.z])
}
fn vv(value: &J) -> Result<Vec3> {
    let p = frow::<3>(value)?;
    Ok(Vec3::new(p[0], p[1], p[2]))
}
fn plugins_json(plugins: &[Chunk]) -> J {
    J::Array(
        plugins
            .iter()
            .map(|p| json::object! {kind:p.kind,version:p.version,payload:hex(&p.payload)})
            .collect(),
    )
}
fn plugins_value(value: &J) -> Result<Vec<Chunk>> {
    array(value, 100_000)?
        .iter()
        .map(|v| {
            fields(v, &["kind", "version", "payload"])?;
            Ok(Chunk {
                kind: v["kind"].as_u32().context("plugin kind must be u32")?,
                version: v["version"]
                    .as_u32()
                    .context("plugin version must be u32")?,
                payload: unhex(
                    v["payload"]
                        .as_str()
                        .context("plugin payload must be hex")?,
                )?,
            })
        })
        .collect()
}
fn hjson(h: &HAnim) -> J {
    json::object! {version:h.version,id:h.id,flags:h.flags,keyframe_size:h.keyframe_size,nodes:J::Array(h.nodes.iter().map(|n|json::object!{id:n.id,index:n.index,flags:n.flags}).collect())}
}
fn hvalue(value: &J, original: Option<&HAnim>) -> Result<Option<HAnim>> {
    if value.is_null() {
        return Ok(None);
    }
    fields(value, &["version", "id", "flags", "keyframe_size", "nodes"])?;
    let mut h = original
        .cloned()
        .unwrap_or_else(|| HAnim::new(0, Vec::new()));
    h.version = value["version"]
        .as_u32()
        .context("HAnim version must be u32")?;
    h.id = value["id"].as_i32().context("HAnim id must be i32")?;
    h.flags = value["flags"].as_u32().context("HAnim flags must be u32")?;
    h.keyframe_size = value["keyframe_size"]
        .as_u32()
        .context("HAnim keyframe_size must be u32")?;
    h.nodes = array(&value["nodes"], 65535)?
        .iter()
        .map(|v| {
            fields(v, &["id", "index", "flags"])?;
            Ok(HAnimNode {
                id: v["id"].as_i32().context("bone id must be i32")?,
                index: v["index"].as_u32().context("bone index must be u32")?,
                flags: v["flags"].as_u32().context("bone flags must be u32")?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(Some(h))
}
fn sjson(s: &Skin) -> J {
    json::object! {used_bones:bj(s.used_bones.iter().copied()),max_weights:s.max_weights,reserved:s.reserved,indices:J::Array(s.indices.iter().map(|v|bj(*v)).collect()),weights:J::Array(s.weights.iter().map(|v|fj(*v)).collect()),inverse_bind_matrices:J::Array(s.inverse_bind_matrices.iter().map(|m|fj(m.0)).collect())}
}
fn svalue(value: &J, original: Option<&Skin>) -> Result<Option<Skin>> {
    if value.is_null() {
        return Ok(None);
    }
    fields(
        value,
        &[
            "used_bones",
            "max_weights",
            "reserved",
            "indices",
            "weights",
            "inverse_bind_matrices",
        ],
    )?;
    let indices = array(&value["indices"], 4_000_000)?
        .iter()
        .map(brow::<4>)
        .collect::<Result<Vec<_>>>()?;
    let weights = array(&value["weights"], 4_000_000)?
        .iter()
        .map(frow::<4>)
        .collect::<Result<Vec<_>>>()?;
    let matrices = array(&value["inverse_bind_matrices"], 255)?
        .iter()
        .map(|v| frow::<16>(v).map(Mat4))
        .collect::<Result<Vec<_>>>()?;
    let mut skin = if let Some(old) = original {
        old.clone()
    } else {
        Skin::new(indices.clone(), weights.clone(), matrices.clone())?
    };
    skin.indices = indices;
    skin.weights = weights;
    skin.inverse_bind_matrices = matrices;
    skin.used_bones = array(&value["used_bones"], 255)?
        .iter()
        .map(|v| v.as_u8().context("used bone must be a byte"))
        .collect::<Result<_>>()?;
    skin.max_weights = value["max_weights"]
        .as_u8()
        .context("max_weights must be u8")?;
    skin.reserved = value["reserved"]
        .as_u8()
        .context("skin reserved must be u8")?;
    skin.validate(skin.indices.len())?;
    Ok(Some(skin))
}

impl Dff {
    pub fn editable_scene(&self) -> J {
        let frames=J::Array(self.frames.iter().enumerate().map(|(i,f)|json::object!{
            source_index:i,name:f.name.clone(),parent:f.parent,flags:f.flags,transform:fj(f.transform.0),hanim:f.hanim.as_ref().map(hjson).unwrap_or(J::Null),plugins:plugins_json(&f.plugins)
        }).collect());
        let geometries=J::Array(self.geometries.iter().enumerate().map(|(i,g)|{
            let morphs=J::Array(g.morph_targets.iter().map(|m|json::object!{center:vj(m.center),radius:fjson(m.radius),positions:J::Array(m.positions.iter().copied().map(vj).collect()),normals:J::Array(m.normals.iter().copied().map(vj).collect())}).collect());
            let materials=J::Array(g.materials.iter().enumerate().map(|(i,m)|json::object!{source_index:i,flags:m.flags,color:bj(m.color),surface:fj(m.surface),texture:m.texture.as_ref().map(|t|json::object!{name:t.name.clone(),mask:t.mask.clone(),filter_flags:t.filter_flags}).unwrap_or(J::Null),plugins:plugins_json(&m.plugins)}).collect());
            let triangles=J::Array(g.triangles.iter().map(|t|json::array![t.indices[0],t.indices[1],t.indices[2],t.material]).collect());
            let uvs=J::Array(g.uv_sets.iter().map(|set|J::Array(set.iter().map(|uv|fj(*uv)).collect())).collect());
            json::object!{source_index:i,flags:g.flags,colors:J::Array(g.colors.iter().map(|c|bj(*c)).collect()),uv_sets:uvs,triangles:triangles,morph_targets:morphs,materials:materials,
                skin:g.skin.as_ref().map(sjson).unwrap_or(J::Null),night_colors:g.night_colors.as_ref().map(|n|json::object!{marker:n.marker,colors:J::Array(n.colors.iter().map(|c|bj(*c)).collect())}).unwrap_or(J::Null),plugins:plugins_json(&g.plugins)}
        }).collect());
        let atomics=J::Array(self.atomics.iter().enumerate().map(|(i,a)|json::object!{source_index:i,frame:a.frame,geometry:a.geometry,flags:a.flags,plugins:plugins_json(&a.plugins)}).collect());
        json::object! {frames:frames,geometries:geometries,atomics:atomics}
    }

    pub fn apply_scene_edits(&mut self, value: &J) -> Result<()> {
        fields(value, &["frames", "geometries", "atomics"])?;
        let version = self.document.chunks[self.clumps[0].root].version;
        let frames = array(&value["frames"], 100_000)?;
        ensure!(
            frames.len() == self.frames.len(),
            "frame count changes require an explicit clump structure"
        );
        for (index, (f, v)) in self.frames.iter_mut().zip(frames).enumerate() {
            fields(
                v,
                &[
                    "source_index",
                    "name",
                    "parent",
                    "flags",
                    "transform",
                    "hanim",
                    "plugins",
                ],
            )?;
            ensure!(
                v["source_index"].as_usize() == Some(index),
                "frame source_index must retain its slot"
            );
            f.name = v["name"]
                .as_str()
                .context("frame name must be a string")?
                .into();
            ensure!(!f.name.contains('\0'), "frame name contains NUL");
            f.parent = v["parent"].as_i32().context("frame parent must be i32")?;
            f.flags = v["flags"].as_u32().context("frame flags must be u32")?;
            f.transform = Mat4(frow::<16>(&v["transform"])?);
            f.hanim = hvalue(&v["hanim"], f.hanim.as_ref())?;
            f.plugins = plugins_value(&v["plugins"])?;
        }
        let geometries = array(&value["geometries"], 65535)?;
        ensure!(
            geometries.len() == self.geometries.len(),
            "geometry count changes require an explicit clump structure"
        );
        for (index, (g, v)) in self.geometries.iter_mut().zip(geometries).enumerate() {
            fields(
                v,
                &[
                    "source_index",
                    "flags",
                    "colors",
                    "uv_sets",
                    "triangles",
                    "morph_targets",
                    "materials",
                    "skin",
                    "night_colors",
                    "plugins",
                ],
            )?;
            ensure!(
                v["source_index"].as_usize() == Some(index),
                "geometry source_index must retain its slot"
            );
            g.flags = v["flags"].as_u32().context("geometry flags must be u32")?;
            g.colors = array(&v["colors"], 4_000_000)?
                .iter()
                .map(brow::<4>)
                .collect::<Result<_>>()?;
            g.uv_sets = array(&v["uv_sets"], 8)?
                .iter()
                .map(|set| {
                    array(set, 4_000_000)?
                        .iter()
                        .map(frow::<2>)
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<_>>()?;
            g.triangles = array(&v["triangles"], 8_000_000)?
                .iter()
                .map(|face| {
                    let row = array(face, 4)?;
                    ensure!(
                        row.len() == 4,
                        "triangle needs three indices and a material"
                    );
                    Ok(Triangle {
                        indices: [
                            row[0].as_u32().context("triangle index must be u32")?,
                            row[1].as_u32().context("triangle index must be u32")?,
                            row[2].as_u32().context("triangle index must be u32")?,
                        ],
                        material: row[3].as_u32().context("triangle material must be u32")?,
                    })
                })
                .collect::<Result<_>>()?;
            let morphs = array(&v["morph_targets"], 256)?;
            ensure!(!morphs.is_empty(), "geometry needs a morph target");
            let template = g.morph_targets[0].clone();
            let mut edited = Vec::new();
            for (i, m) in morphs.iter().enumerate() {
                fields(m, &["center", "radius", "positions", "normals"])?;
                let mut out = g
                    .morph_targets
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| template.clone());
                out.center = vv(&m["center"])?;
                out.radius = fvalue(&m["radius"])?;
                out.positions = array(&m["positions"], 4_000_000)?
                    .iter()
                    .map(vv)
                    .collect::<Result<_>>()?;
                out.normals = array(&m["normals"], 4_000_000)?
                    .iter()
                    .map(vv)
                    .collect::<Result<_>>()?;
                edited.push(out);
            }
            g.morph_targets = edited;
            let mut materials = Vec::new();
            for (i, m) in array(&v["materials"], 65535)?.iter().enumerate() {
                fields(
                    m,
                    &[
                        "source_index",
                        "flags",
                        "color",
                        "surface",
                        "texture",
                        "plugins",
                    ],
                )?;
                ensure!(
                    m["source_index"].as_usize() == Some(i),
                    "material source_index must retain its slot"
                );
                let mut out = g
                    .materials
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| Material::new(version));
                out.flags = m["flags"].as_u32().context("material flags must be u32")?;
                out.color = brow::<4>(&m["color"])?;
                out.surface = frow::<3>(&m["surface"])?;
                out.plugins = plugins_value(&m["plugins"])?;
                if m["texture"].is_null() {
                    out.texture = None;
                } else {
                    let t = &m["texture"];
                    fields(t, &["name", "mask", "filter_flags"])?;
                    let name = t["name"]
                        .as_str()
                        .context("texture name must be string")?
                        .to_string();
                    let mask = t["mask"]
                        .as_str()
                        .context("texture mask must be string")?
                        .to_string();
                    let filter = t["filter_flags"]
                        .as_u32()
                        .context("texture flags must be u32")?;
                    if let Some(old) = out.texture.as_mut() {
                        old.name = name;
                        old.mask = mask;
                        old.filter_flags = filter;
                    } else {
                        out.texture = Some(TextureRef::new(name, mask, filter, version));
                    }
                }
                materials.push(out);
            }
            g.materials = materials;
            g.skin = svalue(&v["skin"], g.skin.as_ref())?;
            g.night_colors = if v["night_colors"].is_null() {
                None
            } else {
                let n = &v["night_colors"];
                fields(n, &["marker", "colors"])?;
                let colors = array(&n["colors"], 4_000_000)?
                    .iter()
                    .map(brow::<4>)
                    .collect::<Result<Vec<_>>>()?;
                let mut out = g
                    .night_colors
                    .clone()
                    .unwrap_or_else(|| NightColors::new(Vec::new()));
                out.marker = n["marker"]
                    .as_u32()
                    .context("night color marker must be u32")?;
                out.colors = colors;
                Some(out)
            };
            g.plugins = plugins_value(&v["plugins"])?;
        }
        let atomics = array(&value["atomics"], 100_000)?;
        ensure!(
            atomics.len() == self.atomics.len(),
            "atomic count changes require an explicit clump structure"
        );
        for (index, (a, v)) in self.atomics.iter_mut().zip(atomics).enumerate() {
            fields(
                v,
                &["source_index", "frame", "geometry", "flags", "plugins"],
            )?;
            ensure!(
                v["source_index"].as_usize() == Some(index),
                "atomic source_index must retain its slot"
            );
            a.frame = v["frame"]
                .as_usize()
                .context("atomic frame must be an index")?;
            a.geometry = v["geometry"]
                .as_usize()
                .context("atomic geometry must be an index")?;
            a.flags = v["flags"].as_u32().context("atomic flags must be u32")?;
            a.plugins = plugins_value(&v["plugins"])?;
        }
        self.validate()
    }
}
