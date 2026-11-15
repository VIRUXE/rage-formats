use super::geometry::{extensions, replace_kind, set_extensions, struct_index, text, Geometry};
use super::plugins::HAnim;
use super::reader::{put_f32, put_u32, Reader};
use super::{parse_chunks, write_chunks, Chunk, Document, Format};
use crate::{DrawableEntry, Mat4};
use anyhow::{ensure, Context, Result};
#[path = "dff_json.rs"]
mod editable;

#[derive(Debug, Clone)]
pub struct Frame {
    pub name: String,
    pub parent: i32,
    pub flags: u32,
    pub transform: Mat4,
    pub hanim: Option<HAnim>,
    pub plugins: Vec<Chunk>,
}
#[derive(Debug, Clone)]
pub struct Atomic {
    pub frame: usize,
    pub geometry: usize,
    pub flags: u32,
    pub plugins: Vec<Chunk>,
    nodes: Vec<Chunk>,
}
#[derive(Debug, Clone)]
struct Clump {
    root: usize,
    nodes: Vec<Chunk>,
    frame_nodes: Vec<Chunk>,
    geometry_nodes: Vec<Chunk>,
    frame_start: usize,
    frame_count: usize,
    geometry_start: usize,
    geometry_count: usize,
    atomic_start: usize,
    atomic_count: usize,
    frame_tail: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct Dff {
    pub frames: Vec<Frame>,
    pub geometries: Vec<Geometry>,
    pub atomics: Vec<Atomic>,
    document: Document,
    clumps: Vec<Clump>,
    original_parents: Vec<i32>,
    original_hanim: Vec<Option<HAnim>>,
}

impl Dff {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let document = Document::parse(data, Format::Dff)?;
        let mut frames = Vec::new();
        let mut geometries = Vec::new();
        let mut atomics = Vec::new();
        let mut clumps = Vec::new();
        for (root, node) in document
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == 0x10)
        {
            let nodes = parse_chunks(&node.payload)?;
            let mut header = Reader::new(&nodes[struct_index(&nodes)?].payload);
            let declared_atomics = header.u32()? as usize;
            let frame_start = frames.len();
            let geometry_start = geometries.len();
            let atomic_start = atomics.len();
            let frame_list = nodes
                .iter()
                .find(|c| c.kind == 0xe)
                .context("DFF clump has no frame list")?;
            let frame_nodes = parse_chunks(&frame_list.payload)?;
            let mut r = Reader::new(&frame_nodes[struct_index(&frame_nodes)?].payload);
            let frame_count = r.u32()? as usize;
            ensure!(frame_count <= 100_000, "too many DFF frames");
            let mut frame_exts = frame_nodes.iter().filter(|c| c.kind == 3);
            for _ in 0..frame_count {
                let mut matrix = [0.; 16];
                matrix[15] = 1.;
                for group in 0..4 {
                    for axis in 0..3 {
                        matrix[group * 4 + axis] = r.f32()?;
                    }
                }
                let local_parent = r.i32()?;
                ensure!(
                    local_parent >= -1
                        && (local_parent < 0 || (local_parent as usize) < frame_count),
                    "frame parent index out of bounds"
                );
                let flags = r.u32()?;
                let plugins = frame_exts
                    .next()
                    .map(|c| parse_chunks(&c.payload))
                    .transpose()?
                    .unwrap_or_default();
                let name = plugins
                    .iter()
                    .find(|p| p.kind == 0x253f2fe)
                    .map(|p| text(&p.payload))
                    .transpose()?
                    .unwrap_or_default();
                let hanim = plugins
                    .iter()
                    .find(|p| p.kind == 0x11e)
                    .map(|p| HAnim::parse(&p.payload))
                    .transpose()?;
                frames.push(Frame {
                    name,
                    parent: if local_parent < 0 {
                        -1
                    } else {
                        frame_start as i32 + local_parent
                    },
                    flags,
                    transform: Mat4(matrix),
                    hanim,
                    plugins,
                });
            }
            ensure!(
                frame_exts.next().is_none(),
                "too many frame extension records"
            );
            let frame_tail = r.tail().to_vec();
            let geometry_list = nodes
                .iter()
                .find(|c| c.kind == 0x1a)
                .context("DFF clump has no geometry list")?;
            let geometry_nodes = parse_chunks(&geometry_list.payload)?;
            let mut gr = Reader::new(&geometry_nodes[struct_index(&geometry_nodes)?].payload);
            let geometry_count = gr.u32()? as usize;
            ensure!(geometry_count <= 65535, "too many DFF geometries");
            for geometry in geometry_nodes.iter().filter(|c| c.kind == 0xf) {
                geometries.push(
                    Geometry::parse(geometry)
                        .with_context(|| format!("DFF geometry {}", geometries.len()))?,
                );
            }
            ensure!(
                geometries.len() - geometry_start == geometry_count,
                "DFF geometry count disagrees with records"
            );
            for atomic in nodes.iter().filter(|c| c.kind == 0x14) {
                let atomic_nodes = parse_chunks(&atomic.payload)?;
                let mut ar = Reader::new(&atomic_nodes[struct_index(&atomic_nodes)?].payload);
                let frame = ar.u32()? as usize;
                let geometry = ar.u32()? as usize;
                let flags = ar.u32()?;
                ensure!(
                    frame < frame_count && geometry < geometry_count,
                    "atomic frame/geometry index out of bounds"
                );
                let plugins = extensions(&atomic_nodes)?;
                atomics.push(Atomic {
                    frame: frame + frame_start,
                    geometry: geometry + geometry_start,
                    flags,
                    plugins,
                    nodes: atomic_nodes,
                });
            }
            let atomic_count = atomics.len() - atomic_start;
            ensure!(
                atomic_count == declared_atomics,
                "DFF atomic count disagrees with records"
            );
            clumps.push(Clump {
                root,
                nodes,
                frame_nodes,
                geometry_nodes,
                frame_start,
                frame_count,
                geometry_start,
                geometry_count,
                atomic_start,
                atomic_count,
                frame_tail,
            });
        }
        let original_parents = frames.iter().map(|f| f.parent).collect();
        let original_hanim = frames.iter().map(|f| f.hanim.clone()).collect();
        let result = Self {
            document,
            frames,
            geometries,
            atomics,
            clumps,
            original_parents,
            original_hanim,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.frames.len() == self.clumps.iter().map(|c| c.frame_count).sum::<usize>()
                && self.geometries.len()
                    == self.clumps.iter().map(|c| c.geometry_count).sum::<usize>()
                && self.atomics.len() == self.clumps.iter().map(|c| c.atomic_count).sum::<usize>(),
            "changing clump membership requires an explicit clump structure"
        );
        let mut state = vec![0u8; self.frames.len()];
        for start in 0..self.frames.len() {
            let mut chain = Vec::new();
            let mut at = Some(start);
            while let Some(index) = at {
                ensure!(index < self.frames.len(), "frame parent out of bounds");
                if state[index] == 2 {
                    break;
                }
                ensure!(state[index] != 1, "DFF frame hierarchy contains a cycle");
                state[index] = 1;
                chain.push(index);
                let frame = &self.frames[index];
                ensure!(
                    frame.transform.0.iter().all(|v| v.is_finite()),
                    "non-finite frame transform"
                );
                ensure!(
                    frame.transform.0[3] == 0.
                        && frame.transform.0[7] == 0.
                        && frame.transform.0[11] == 0.
                        && frame.transform.0[15] == 1.,
                    "DFF frame transform must be affine"
                );
                ensure!(frame.parent >= -1, "negative frame parent other than -1");
                at = if frame.parent < 0 {
                    None
                } else {
                    Some(frame.parent as usize)
                };
            }
            for index in chain {
                state[index] = 2;
            }
        }
        for clump in &self.clumps {
            for frame in &self.frames[clump.frame_start..clump.frame_start + clump.frame_count] {
                ensure!(
                    frame.parent < 0
                        || (frame.parent as usize) >= clump.frame_start
                            && (frame.parent as usize) < clump.frame_start + clump.frame_count,
                    "frame parent crosses clumps"
                );
            }
            for atomic in &self.atomics[clump.atomic_start..clump.atomic_start + clump.atomic_count]
            {
                ensure!(
                    atomic.frame >= clump.frame_start
                        && atomic.frame < clump.frame_start + clump.frame_count
                        && atomic.geometry >= clump.geometry_start
                        && atomic.geometry < clump.geometry_start + clump.geometry_count,
                    "atomic reference crosses clumps or is out of range"
                );
            }
        }
        for geometry in &self.geometries {
            geometry.validate()?;
        }
        Ok(())
    }

    pub fn frame_world(&self, index: usize) -> Result<Mat4> {
        ensure!(index < self.frames.len(), "frame index out of bounds");
        let mut world = Mat4::identity();
        let mut at = Some(index);
        let mut seen = std::collections::HashSet::new();
        while let Some(index) = at {
            ensure!(seen.insert(index), "DFF frame hierarchy cycle");
            let frame = self
                .frames
                .get(index)
                .context("frame parent out of bounds")?;
            world = frame.transform.mul(&world);
            at = if frame.parent < 0 {
                None
            } else {
                Some(frame.parent as usize)
            };
        }
        Ok(world)
    }

    pub fn build(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let reparented = self
            .frames
            .iter()
            .zip(&self.original_parents)
            .any(|(f, p)| f.parent != *p);
        if reparented
            && self
                .frames
                .iter()
                .any(|f| f.hanim.as_ref().is_some_and(|h| !h.nodes.is_empty()))
        {
            ensure!(
                self.frames
                    .iter()
                    .zip(&self.original_hanim)
                    .any(|(f, h)| f.hanim != *h),
                "reparenting a rig requires explicitly updated HAnim hierarchy metadata"
            );
        }
        let mut document = self.document.clone();
        for clump in &self.clumps {
            let version = document.chunks[clump.root].version;
            let mut nodes = clump.nodes.clone();
            let mut frame_nodes = clump.frame_nodes.clone();
            let mut data = Vec::new();
            put_u32(&mut data, clump.frame_count as u32);
            let mut frame_extensions = Vec::new();
            for frame in &self.frames[clump.frame_start..clump.frame_start + clump.frame_count] {
                for group in 0..4 {
                    for axis in 0..3 {
                        put_f32(&mut data, frame.transform.0[group * 4 + axis]);
                    }
                }
                let parent = if frame.parent < 0 {
                    -1
                } else {
                    frame.parent - clump.frame_start as i32
                };
                put_u32(&mut data, parent as u32);
                put_u32(&mut data, frame.flags);
                let mut plugins = frame.plugins.clone();
                if let Some(name) = plugins.iter_mut().find(|p| p.kind == 0x253f2fe) {
                    if text(&name.payload)? != frame.name {
                        name.payload = frame.name.as_bytes().to_vec();
                    }
                } else if !frame.name.is_empty() {
                    plugins.push(Chunk {
                        kind: 0x253f2fe,
                        version,
                        payload: frame.name.as_bytes().to_vec(),
                    });
                }
                replace_kind(
                    &mut plugins,
                    0x11e,
                    frame
                        .hanim
                        .as_ref()
                        .map(|h| h.build())
                        .transpose()?
                        .map(|payload| Chunk {
                            kind: 0x11e,
                            version,
                            payload,
                        })
                        .into_iter()
                        .collect(),
                );
                frame_extensions.push(Chunk {
                    kind: 3,
                    version,
                    payload: write_chunks(&plugins)?,
                });
            }
            data.extend_from_slice(&clump.frame_tail);
            let index = struct_index(&frame_nodes)?;
            frame_nodes[index].payload = data;
            replace_kind(&mut frame_nodes, 3, frame_extensions);
            let frame_index = nodes.iter().position(|c| c.kind == 0xe).unwrap();
            nodes[frame_index].payload = write_chunks(&frame_nodes)?;
            let mut geometry_nodes = clump.geometry_nodes.clone();
            let original: Vec<_> = geometry_nodes.iter().filter(|c| c.kind == 0xf).collect();
            let geometry_chunks = self.geometries
                [clump.geometry_start..clump.geometry_start + clump.geometry_count]
                .iter()
                .enumerate()
                .map(|(i, g)| g.build(original[i].version))
                .collect::<Result<Vec<_>>>()?;
            replace_kind(&mut geometry_nodes, 0xf, geometry_chunks);
            let index = nodes.iter().position(|c| c.kind == 0x1a).unwrap();
            nodes[index].payload = write_chunks(&geometry_nodes)?;
            let mut atomic_chunks = Vec::new();
            for atomic in &self.atomics[clump.atomic_start..clump.atomic_start + clump.atomic_count]
            {
                let mut atomic_nodes = atomic.nodes.clone();
                let index = struct_index(&atomic_nodes)?;
                let data = &mut atomic_nodes[index].payload;
                data[..4]
                    .copy_from_slice(&((atomic.frame - clump.frame_start) as u32).to_le_bytes());
                data[4..8].copy_from_slice(
                    &((atomic.geometry - clump.geometry_start) as u32).to_le_bytes(),
                );
                data[8..12].copy_from_slice(&atomic.flags.to_le_bytes());
                set_extensions(&mut atomic_nodes, &atomic.plugins, version)?;
                atomic_chunks.push(Chunk {
                    kind: 0x14,
                    version,
                    payload: write_chunks(&atomic_nodes)?,
                });
            }
            replace_kind(&mut nodes, 0x14, atomic_chunks);
            document.chunks[clump.root].payload = write_chunks(&nodes)?;
        }
        document.build()
    }

    pub fn drawables(&self) -> Result<Vec<DrawableEntry>> {
        super::dff_drawable::drawables(self)
    }
}
