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
use crate::ytyp::{decode_meta_pointer, read_meta_blocks, read_meta_u32_array, MetaBlock};

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

/// An interior placement: where a `.ymap` puts an MLO, which of its entity
/// sets start switched on, and which floor and room group it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct MloInstance {
    /// The placement itself (`is_mlo_instance` is always true).
    pub entity: YmapEntity,
    pub group_id: u32,
    pub floor_id: u32,
    /// Name hashes of the `CMloEntitySet`s this placement enables.
    pub default_entity_sets: Vec<u32>,
    pub num_exit_portals: u32,
}

/// Walks a `.ymap`'s Meta blocks and reads `CMapData.entities` as a list of
/// packed pointers, which the two entity readers below then decode.
fn map_entity_pointers(data: &[u8]) -> Result<(Vec<MetaBlock>, Vec<u64>)> {
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
    let Some((arr_block, arr_off)) = decode_meta_pointer(ptr) else { return Ok((blocks, Vec::new())) };
    let ptrs = {
        let arr = blocks.get(arr_block).context("ymap: entities pointer block out of range")?;
        let bytes = arr.data.get(arr_off..arr_off + count * 8).context("ymap: entities pointer array out of bounds")?;
        (0..count).map(|i| u64_le(bytes, i * 8)).collect()
    };
    Ok((blocks, ptrs))
}

/// Parses the entity list of a `.ymap`.
pub fn parse_ymap_entities(data: &[u8]) -> Result<Vec<YmapEntity>> {
    let (blocks, ptrs) = map_entity_pointers(data)?;
    let mut out = Vec::with_capacity(ptrs.len());
    for ptr in ptrs {
        let Some((bi, off)) = decode_meta_pointer(ptr) else { continue };
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

/// Parses just the interior placements of a `.ymap` — the entities
/// `parse_ymap_entities` flags as MLO instances, with the extra
/// `CMloInstanceDef` fields that `YmapEntity` has no room for.
pub fn parse_ymap_mlo_instances(data: &[u8]) -> Result<Vec<MloInstance>> {
    let (blocks, ptrs) = map_entity_pointers(data)?;
    let mut out = Vec::new();
    for ptr in ptrs {
        let Some((bi, off)) = decode_meta_pointer(ptr) else { continue };
        let Some(block) = blocks.get(bi) else { continue };
        if block.name_hash != HASH_CMLOINSTANCEDEF {
            continue;
        }
        // A CMloInstanceDef is a 128-byte CEntityDef plus 32 bytes of its own.
        let Some(e) = block.data.get(off..off + 160) else { continue };
        out.push(MloInstance {
            entity: read_entity(e, true),
            group_id: u32_le(e, 128),
            floor_id: u32_le(e, 132),
            default_entity_sets: read_meta_u32_array(&blocks, e, 136),
            num_exit_portals: u32_le(e, 152),
        });
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
    use crate::hash::rage_joaat;
    use crate::resource::{build_rsc7, SYSTEM_BASE};

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

    // ─── MLO instance fixture ─────────────────────────────────────────────────

    fn put_u16(s: &mut [u8], off: usize, v: u16) { s[off..off + 2].copy_from_slice(&v.to_le_bytes()); }
    fn put_u32(s: &mut [u8], off: usize, v: u32) { s[off..off + 4].copy_from_slice(&v.to_le_bytes()); }
    fn put_u64(s: &mut [u8], off: usize, v: u64) { s[off..off + 8].copy_from_slice(&v.to_le_bytes()); }
    fn put_f32(s: &mut [u8], off: usize, v: f32) { s[off..off + 4].copy_from_slice(&v.to_le_bytes()); }
    fn put_vec3(s: &mut [u8], off: usize, v: Vec3) {
        put_f32(s, off, v.x);
        put_f32(s, off + 4, v.y);
        put_f32(s, off + 8, v.z);
    }
    /// A Meta array field: packed pointer at `off`, `count1` eight bytes later.
    fn put_array(s: &mut [u8], off: usize, block_index: usize, count: u16) {
        put_u64(s, off, block_index as u64 + 1);
        put_u16(s, off + 8, count);
    }

    /// `(structure-name hash, offset, length)` of each data block.
    const YMAP_BLOCKS: [(u32, usize, usize); 4] = [
        (HASH_CMAPDATA, 0x0B0, 368),        // 0: CMapData
        (0, 0x220, 8),                      // 1: entities pointer array
        (HASH_CMLOINSTANCEDEF, 0x230, 160), // 2: the one CMloInstanceDef
        (0, 0x2D0, 8),                      // 3: u32 array: two entity-set names
    ];

    const INSTANCE_POSITION: Vec3 = Vec3 { x: 100.0, y: 200.0, z: 30.0 };

    /// A hand-built `.ymap` placing one interior with two default entity sets
    /// switched on.
    fn minimal_mlo_ymap(instance_block_hash: u32) -> Vec<u8> {
        build_rsc7(2, &mlo_ymap_system(instance_block_hash), &[])
    }

    /// [`minimal_mlo_ymap`]'s Meta system section, before it is wrapped.
    fn mlo_ymap_system(instance_block_hash: u32) -> Vec<u8> {
        let mut sys = vec![0u8; 0x300];

        put_u64(&mut sys, 0x30, SYSTEM_BASE + 0x70); // DataBlocksPointer
        put_u16(&mut sys, 0x4C, YMAP_BLOCKS.len() as u16); // DataBlocksCount
        for (i, &(hash, off, len)) in YMAP_BLOCKS.iter().enumerate() {
            let h = 0x70 + i * 16;
            put_u32(&mut sys, h, if i == 2 { instance_block_hash } else { hash });
            put_u32(&mut sys, h + 4, len as u32);
            put_u64(&mut sys, h + 8, SYSTEM_BASE + off as u64);
        }

        // CMapData.entities -> block 1, one entry, pointing at block 2.
        put_array(&mut sys, YMAP_BLOCKS[0].1 + 96, 1, 1);
        put_u64(&mut sys, YMAP_BLOCKS[1].1, 3);

        // CMloInstanceDef: a CEntityDef, then the interior-only fields.
        let e = YMAP_BLOCKS[2].1;
        put_u32(&mut sys, e + 8, rage_joaat("v_test_mlo")); // archetypeName
        put_vec3(&mut sys, e + 32, INSTANCE_POSITION);
        put_f32(&mut sys, e + 60, 1.0); // rotation w
        put_f32(&mut sys, e + 64, 1.0); // scaleXY
        put_f32(&mut sys, e + 68, 1.0); // scaleZ
        put_u32(&mut sys, e + 72, -1i32 as u32); // parentIndex
        put_u32(&mut sys, e + 128, 5); // groupId
        put_u32(&mut sys, e + 132, 2); // floorId
        put_array(&mut sys, e + 136, 3, 2); // defaultEntitySets
        put_u32(&mut sys, e + 152, 3); // numExitPortals

        let u = YMAP_BLOCKS[3].1;
        put_u32(&mut sys, u, rage_joaat("set_kitchen"));
        put_u32(&mut sys, u + 4, rage_joaat("set_hall"));

        sys
    }

    #[test]
    fn parses_mlo_instance_with_default_entity_sets() {
        let instances = parse_ymap_mlo_instances(&minimal_mlo_ymap(HASH_CMLOINSTANCEDEF))
            .expect("fixture should parse");
        assert_eq!(instances.len(), 1);
        let i = &instances[0];
        assert_eq!(i.entity.archetype_hash, rage_joaat("v_test_mlo"));
        assert_eq!(i.entity.position, INSTANCE_POSITION);
        assert!(i.entity.is_mlo_instance);
        assert_eq!(i.group_id, 5);
        assert_eq!(i.floor_id, 2);
        assert_eq!(i.default_entity_sets, vec![rage_joaat("set_kitchen"), rage_joaat("set_hall")]);
        assert_eq!(i.num_exit_portals, 3);
    }

    #[test]
    fn plain_entities_are_not_mlo_instances() {
        let ymap = minimal_mlo_ymap(HASH_CENTITYDEF);
        assert!(parse_ymap_mlo_instances(&ymap).expect("should parse").is_empty());
        // The same file still yields the placement through the entity list.
        let entities = parse_ymap_entities(&ymap).expect("should parse");
        assert_eq!(entities.len(), 1);
        assert!(!entities[0].is_mlo_instance);
    }

    #[test]
    fn mlo_instance_without_default_entity_sets_yields_an_empty_list() {
        let mut sys = mlo_ymap_system(HASH_CMLOINSTANCEDEF);
        put_u64(&mut sys, YMAP_BLOCKS[2].1 + 136, 0); // defaultEntitySets: null
        put_u16(&mut sys, YMAP_BLOCKS[2].1 + 144, 0);
        let instances = parse_ymap_mlo_instances(&build_rsc7(2, &sys, &[])).expect("should parse");
        assert_eq!(instances.len(), 1);
        assert!(instances[0].default_entity_sets.is_empty());
    }
}
