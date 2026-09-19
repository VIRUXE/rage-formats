//! Map placements (`.ymap`, RSC7 "Meta" format): the entities a map chunk
//! places — archetype, position, rotation, scale. This reads just the
//! `CMapData.entities` list, which is what's needed to put an MLO's
//! interior-local data (collision, rooms) into world space.
//!
//! Layout from CodeWalker.Core `MetaTypes.cs` (`CMapData`, `CEntityDef`,
//! `CMloInstanceDef`); block walking shared with `ytyp.rs`.

use anyhow::{bail, Context, Result};

use crate::math::Vec3;
use crate::resource::{f32_le, prepare_rsc7, u16_le, u32_le, u64_le, vec3_le, ResReader};
use crate::ytyp::{decode_meta_pointer, read_meta_blocks};

/// `MetaName` hashes (exact-case Jenkins of the structure name).
const HASH_CMAPDATA: u32 = 3_545_841_574;
const HASH_CENTITYDEF: u32 = 3_461_354_627;
const HASH_CMLOINSTANCEDEF: u32 = 164_374_718;

/// One placed entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YmapEntity {
    /// Lowercase JOAAT of the archetype name.
    pub archetype_hash: u32,
    pub flags: u32,
    pub guid: u32,
    pub position: Vec3,
    /// Quaternion as stored: x, y, z, w.
    pub rotation: [f32; 4],
    pub scale_xy: f32,
    pub scale_z: f32,
    pub parent_index: i32,
    pub lod_dist: f32,
    /// True for a `CMloInstanceDef` (an interior placement).
    pub is_mlo_instance: bool,
}

impl YmapEntity {
    /// Transforms an entity-local point into world space (scale, then the
    /// stored rotation, then translation). Map entities store the *inverse*
    /// rotation, so the conjugate is applied — matching CodeWalker's
    /// `YmapEntityDef.SetOrientation` (`Orientation = Quaternion.Invert(rotation)`).
    pub fn to_world(&self, local: Vec3) -> Vec3 {
        let scaled = Vec3::new(local.x * self.scale_xy, local.y * self.scale_xy, local.z * self.scale_z);
        let [x, y, z, w] = self.rotation;
        // conjugate: rotate by (-x, -y, -z, w)
        rotate(scaled, [-x, -y, -z, w]) + self.position
    }
}

/// Rotates `v` by the unit quaternion `q` (x, y, z, w).
pub fn rotate(v: Vec3, q: [f32; 4]) -> Vec3 {
    let u = Vec3::new(q[0], q[1], q[2]);
    let s = q[3];
    // v' = 2 (u·v) u + (s² − u·u) v + 2 s (u × v)
    u * (2.0 * u.dot(v)) + v * (s * s - u.dot(u)) + u.cross(v) * (2.0 * s)
}

/// Parses the entity list of a `.ymap`.
pub fn parse_ymap_entities(data: &[u8]) -> Result<Vec<YmapEntity>> {
    let (system, graphics) = prepare_rsc7(data)?;
    let reader = ResReader { system: &system, graphics: &graphics };
    let blocks = read_meta_blocks(&reader).context("ymap")?;
    let map = blocks.iter().find(|b| b.name_hash == HASH_CMAPDATA).context("ymap: CMapData block not found")?;
    const ENTITIES_OFFSET: usize = 96;
    if map.data.len() < ENTITIES_OFFSET + 16 {
        bail!("ymap: CMapData block too small");
    }
    let ptr = u64_le(&map.data, ENTITIES_OFFSET);
    let count = u16_le(&map.data, ENTITIES_OFFSET + 8) as usize;
    let Some((arr_block, arr_off)) = decode_meta_pointer(ptr) else { return Ok(Vec::new()) };
    let arr = blocks.get(arr_block).context("ymap: entities pointer block out of range")?;
    let ptrs = arr.data.get(arr_off..arr_off + count * 8).context("ymap: entities pointer array out of bounds")?;

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let Some((bi, off)) = decode_meta_pointer(u64_le(ptrs, i * 8)) else { continue };
        let Some(block) = blocks.get(bi) else { continue };
        let is_mlo_instance = match block.name_hash {
            HASH_CENTITYDEF => false,
            HASH_CMLOINSTANCEDEF => true,
            _ => continue,
        };
        let Some(e) = block.data.get(off..off + 128) else { continue };
        out.push(read_entity(e, is_mlo_instance));
    }
    Ok(out)
}

/// Decodes a `CEntityDef` (the first 128 bytes of a `CMloInstanceDef` too).
pub(crate) fn read_entity(e: &[u8], is_mlo_instance: bool) -> YmapEntity {
    YmapEntity {
        archetype_hash: u32_le(e, 8),
        flags: u32_le(e, 12),
        guid: u32_le(e, 16),
        position: vec3_le(e, 32),
        rotation: [f32_le(e, 48), f32_le(e, 52), f32_le(e, 56), f32_le(e, 60)],
        scale_xy: f32_le(e, 64),
        scale_z: f32_le(e, 68),
        parent_index: u32_le(e, 72) as i32,
        lod_dist: f32_le(e, 76),
        is_mlo_instance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_by_quarter_turn_about_z() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let r = rotate(Vec3::new(1.0, 0.0, 0.0), [0.0, 0.0, h, h]);
        assert!((r - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5, "{r:?}");
    }

    #[test]
    fn to_world_applies_conjugate_then_translation() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let e = YmapEntity {
            archetype_hash: 0, flags: 0, guid: 0, position: Vec3::new(10.0, 20.0, 30.0),
            rotation: [0.0, 0.0, h, h], scale_xy: 1.0, scale_z: 1.0, parent_index: -1, lod_dist: 0.0, is_mlo_instance: true,
        };
        // Stored quaternion is the inverse, so a +90° store rotates -90°.
        let p = e.to_world(Vec3::new(1.0, 0.0, 0.0));
        assert!((p - Vec3::new(10.0, 19.0, 30.0)).length() < 1e-5, "{p:?}");
    }
}
