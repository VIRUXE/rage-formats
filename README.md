# rage-formats

Parsers, and where it matters writers, for the RAGE resource formats GTA V
streams: the RSC7-wrapped files found inside `.rpf` archives. Bytes in,
plain Rust structs out. No game install, no unsafe, no C dependencies.

Getting the bytes out of an archive is
[`rpf-archive`](https://github.com/VIRUXE/rpf-archive-rs)'s job; drawing
what this crate parsed is [`rage-render`](https://github.com/VIRUXE/rage-render)'s;
[`rage-cli`](https://github.com/VIRUXE/rage-cli) is the command line over
all three.

```toml
[dependencies]
rage-formats = "0.1"
```

## What it reads and writes

| Format | Read | Write | Entry point | You get |
|---|:-:|:-:|---|---|
| `.ytd` texture dictionary | yes | | `parse_ytd` | `YtdTexture` per texture, decoded to RGBA by `texture_utils` |
| `.ydr` drawable | yes | | `parse_ydr`, `parse_drawables` | `Drawable`: bounds, LODs, models, geometry, shader group, embedded textures |
| `.ydd` drawable dictionary | yes | | `parse_ydd`, `parse_drawables` | one `DrawableEntry` per member, uniquely named |
| `.yft` fragment | yes | | `parse_yft` | `Fragment`: main drawable, physics children with transforms, bone pose |
| `.ybn` collision | yes | | `parse_ybn` | the `phBound` tree; `Ybn::triangles()` flattens it to world space |
| `.ynv` navmesh | yes | yes | `parse_ynv`, `serialize_ynv` | `Ynv`: polygons with vertices, flags and edge adjacency, portals, points |
| `.ymap` placements | yes | | `parse_ymap_entities` | `YmapEntity` per entity, with `to_world()` |
| `.ytyp` archetypes | yes | | `parse_ytyp` | every archetype's box and texture dictionary; each MLO's entities and rooms |
| `.ymt` ped variation | yes | | `parse_ymt` | `PedVariationInfo` |
| `gtxd.ymt` / `gtxd.meta` | yes | | `parse_txd_relationships` | texture dictionary parent chain |
| RSC7 container | yes | yes | `prepare_rsc7`, `build_rsc7`, `build_rsc7_paged` | sections in, a valid file out |

Hashing is `rage_joaat`, the Jenkins one-at-a-time every name in these
formats is looked up by.

## How the formats fit together

```
 .rpf archive (rpf-archive)
   |
   +-- RSC7 file: 16-byte header, deflated body
         |         version, system and graphics page flags
         v
     prepare_rsc7  ->  system section + graphics section
         |
         v
     ResReader     resolves 0x50000000-based (system) and 0x60000000-based
         |         (graphics) virtual pointers into slices
         v
     one parser per format, chasing pointers block by block
```

Three families of file share that container but differ inside:

- **Fixed-layout resources** (`ytd`, `ydr`, `ydd`, `yft`, `ybn`, `ynv`):
  structs with pointers to other structs, laid out by the engine's own C++.
  The parsers here mirror those layouts field by field.
- **Meta** (`ytyp`, `ymap`, ped `ymt`): a self-describing block table, each
  block tagged with a structure-name hash, with packed block:offset pointers
  between them. `ytyp.rs` walks the table once; `ymap.rs` and the MLO reader
  reuse that.
- **RBF** (`gtxd.ymt`): a flat record stream with its own descriptor table;
  `rbf.rs` reads it whole.

Every byte offset was ported from CodeWalker.Core (`Bounds.cs`, `Nav.cs`,
`YnvFile.cs`, `MetaTypes.cs`, `DrawableBase.cs` and friends) and checked
against retail files.

## Examples

### Drawables

`parse_drawables` reads any drawable-bearing resource, a lone `.ydr`, a
`.ydd` dictionary or a `.yft` fragment, into one flat list of named
entries, so a caller can stay agnostic about which it was handed:

```rust
use rage_formats::{parse_drawables, DrawableKind};

let entries = parse_drawables(&ydd_bytes, DrawableKind::Ydd)?;
for entry in &entries {
    println!("{} (0x{:08X}): {} triangle(s)", entry.name, entry.hash, entry.drawable.geometry_count());
}
```

Each `DrawableEntry { hash, name, drawable }` carries a `Drawable` with
bounds, LODs, shader group and geometry. Entries that would otherwise share
a name (dictionary members with no name of their own, or a fragment's extra
drawables with no names array) get unique `0x…` names instead of
overwriting each other.

### Textures to images

`.ytd` textures decode straight to an `image::RgbaImage`, ready to resize
and encode:

```rust
use rage_formats::{parse_ytd, texture_utils::{to_rgba_image, fit_max_size, encode_image, ImageFormat}};

let textures = parse_ytd(&ytd_bytes)?;
for texture in &textures {
    let image = to_rgba_image(texture)?;
    let image = fit_max_size(image, 512);
    let png_bytes = encode_image(&image, ImageFormat::Png, 90)?;
    std::fs::write(format!("{}.png", texture.name), png_bytes)?;
}
```

The `image` feature (on by default) provides those three helpers; without
it `decompress_texture` still gives raw RGBA bytes and the crate has no
`image` dependency.

### Collision to world-space triangles

A `.ybn` is a tree: a composite whose children are triangle meshes, each
placed by its own transform. `triangles()` applies the transforms and hands
back the soup:

```rust
use rage_formats::parse_ybn;

let ybn = parse_ybn(&ybn_bytes)?;
for t in ybn.triangles() {
    let n = t.normal();
    if n.z > 0.7 { /* walkable */ }
}
```

An MLO's collision is in the interior's own space; place it with the
entity that positions the MLO in the world:

```rust
use rage_formats::{parse_ymap_entities, parse_ybn};

let placement = parse_ymap_entities(&milo_ymap)?.into_iter().find(|e| e.is_mlo_instance).unwrap();
let world: Vec<_> = parse_ybn(&ybn_bytes)?.triangles().into_iter()
    .map(|mut t| { for v in &mut t.vertices { *v = placement.to_world(*v); } t })
    .collect();
```

`to_world` applies scale, the stored rotation (map entities store the
inverse quaternion, as CodeWalker's `YmapEntityDef` does) and translation.

### Navmesh cells, read and write

```rust
use rage_formats::{parse_ynv, serialize_ynv, NavEdge, NavPoly, Vec3, cell_file_name, cell_for_position};

let mut cell = parse_ynv(&std::fs::read("navmesh[108][96].ynv")?)?;
println!("area {} polys {} portals {}", cell.area_id, cell.polys.len(), cell.portals.len());

let mut poly = NavPoly::new(vec![
    Vec3::new(-580.0, -1064.0, 21.3), Vec3::new(-578.0, -1064.0, 21.3),
    Vec3::new(-578.0, -1062.0, 21.3), Vec3::new(-580.0, -1062.0, 21.3),
]);
poly.set_interior(true);
poly.set_flat_ground(true);
poly.edges[1] = NavEdge::neighbour(cell.area_id, 42); // edge 1 joins polygon 42 of this cell
cell.polys.push(poly);

std::fs::write("navmesh[108][96].ynv", serialize_ynv(&cell)?)?;
assert_eq!(cell_for_position(-578.0, -1063.0), (36, 32));
assert_eq!(cell_file_name(36, 32), "navmesh[108][96].ynv");
```

Polygons are counter-clockwise; edge `i` runs from vertex `i` to `i+1` and
names the one polygon across it (`NavEdge::NONE` when there is none). The
writer rebuilds everything derived: quantised vertices deduplicated inside
the cell box, index and edge lists in 16 KiB parts, the adjacent-area table,
portal links, per-polygon cell boxes and part ids, and the two-level sector
quadtree, then wraps it in an RSC7 file whose blocks never straddle a page.
A retail cell survives parse, serialize, parse with every polygon, edge,
flag, portal and point equal, and a second serialize is byte-identical.

### MLO definitions

```rust
use rage_formats::parse_ytyp;

let ytyp = parse_ytyp(&int_ytyp)?;
for mlo in &ytyp.mlos {
    for room in &mlo.rooms { println!("room {:?}..{:?}", room.bb_min, room.bb_max); }
    for e in &mlo.entities { println!("prop {:#010x} at {:?}", e.archetype_hash, e.position); }
}
let boxes: std::collections::HashMap<u32, _> = ytyp.archetypes.iter().map(|a| (a.name_hash, (a.bb_min, a.bb_max))).collect();
```

Entities inside an MLO are relative to the MLO origin; apply the entity's
own `to_world` and then the MLO instance's.

## Features

| Feature | Default | Effect |
|---|:-:|---|
| `image` | on | `to_rgba_image`, `fit_max_size`, `encode_image`, `ImageFormat`, re-exports `image` |
| `test-support` | off | exposes `ydd::tests::minimal_ydr_sections` for downstream tests |

## Testing

`cargo test` runs on hand-built byte fixtures and needs no game. The
integration tests in `tests/real_files.rs` are `#[ignore]` and read real
files from environment variables:

```sh
RAGE_TEST_YNV='C:\...\navmesh[108][96].ynv' \
RAGE_TEST_YBN='C:\...\stream\ybn\interior.ybn' \
RAGE_TEST_YMAP='C:\...\stream\ymap\interior_milo_.ymap' \
RAGE_TEST_MLO_DIR='C:\...\stream' \
cargo test --test real_files -- --ignored --nocapture
```

They print what they found and assert the navmesh round trip.

## License

[Unlicense](LICENSE), public domain.
