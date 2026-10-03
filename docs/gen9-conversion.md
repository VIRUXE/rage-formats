# Gen9 (Enhanced) -> Legacy conversion: findings and plan

Status (2026-10-03): `.ytd` conversion is done and verified (`gen9::Edition`, `Texture::read_gen9`,
`convert_ytd_to_legacy`; `rage resource convert` in rage-cli). Drawables (`.ydr`/`.ydd`) and fragments (`.yft`)
are researched but **not implemented**. Tracking: #13 (umbrella), #15 (drawables), #16 (fragments).

## Why

A FiveM server's `console_exclusive` pack was extracted from the **Legacy** install, where the Gen9-only DLC cars
(`s95`, `arbitergt`, `astron2`, `cyclone2`, `ignus2`) are ~1 KB placeholders (2 triangles, a `blank` shader).
Spawning one crashes the client. The real models exist only in GTA V Enhanced, in a layout the legacy game
(FiveM b3751) cannot load. CodeWalker's `Gen9Converter` only goes Legacy -> Gen9, and a Gen9 read followed by a
legacy write in CodeWalker produces broken output (`VertexBuffer.InfoPointer` 0, stray SRV blocks, wrong
`ShaderFX` sizes), so CodeWalker is a layout spec here, not a converter.

## Container and versions

Same RSC7 header, page flags and deflate in both editions. `.ybn`, meta and `.ymap` are byte-identical.

| ext | Legacy | Gen9 |
|---|---|---|
| ytd | 13 | 5 |
| ydr / ydd | 165 | 159 (CodeWalker also reads 154) |
| yft | 162 | 171 |
| ypt | 68 | 71 |

Every typed block's **VFT word differs** between editions (constant per type across all files), for example adder:
FragType Gen9 `0x4068C7A0` vs legacy `0x40573148`; drawable `0x406E5710` vs `0x40606BE8`; shader group
`0x406BB978` / `0x40613940`; model `0x406BB948` / `0x40610AB8`; geometry `0x406BB7E8` / `0x40618898`;
VertexBuffer `0x406B9108` / `0x4061D418`; IndexBuffer `0x406B90B8` / `0x40613238`; skeleton `0x406BE318` /
`0x40613DA0`. Write the legacy values (CodeWalker-built files load, so the game probably ignores them: inferred).

## What differs in a drawable (verified on real files)

| Struct | Gen9 | Legacy | Notes |
|---|---|---|---|
| Texture | 128 B (80 B base + 48 B incl. SRV) | 144 B | **done**, see `Texture::read_gen9` |
| VertexBuffer | 64 B: count u32@8, stride u16@0xC, bind@0x10, data@0x18, SRV@0x30, decl@0x38 | 128 B (stride@8, flags@0xA, data1@0x10, count@0x18, data2@0x20, info@0x30) | legacy `Flags` always 0, `Data1 == Data2` |
| VertexDeclarationG9 | 320 B: offsets u32[52], sizes u8[52] (= stride, not size), types u8[52], data u64 | 16 B (`flags` bitmask + `types` nibbles) | legacy types always GTAV1 `0x7755555555996996` |
| IndexBuffer | 64 B: count@8, index size u16@0xC, bind@0x10, data@0x18, SRV@0x30 | 96 B | Gen9 index size is always 2 |
| ShaderFX | 64 B: name@0, preset@4 (always `0x6D657461` "meta"), params@8, texrefs@0x10, unknown@0x18, paramInfos@0x20, bucket@0x39, dataSize@0x3A, bucketMask@0x3C | 48 B | legacy counts/sizes derive in `write` from the params block |
| ShaderParamInfosG9 | 8 B header (NumBuffers, NumTextures, NumUnknowns, NumSamplers, NumParams, 0, 1, 12) + 8 B/param (hash, type&3, index, offset bits 8..19, length bits 20..31) | none | CodeWalker `Drawable.cs:1278-1342`, values `:663-775` |
| ShaderGroup | blocks size 0 | blocks size 4 | |
| Drawable, DrawableModel, DrawableGeometry, Skeleton, Bones, Lights, bounds | same | same | no `IsGen9` branch in CodeWalker |

Gen9 vertex and index buffers, and every VB/IB SRV pointer, are non-zero: **drop the SRV blocks**. Vertex and
index data sit in the **system** section in both editions for vehicle frags (graphics section is 0 bytes in all
596 base vehicles); our writer will put them in graphics pages as CodeWalker does (acceptable: inferred).

### Vertices: only a reorder (verified, 10,082 Gen9 geometries)

Formats in use: R32G32B32_FLOAT, R32G32B32A32_FLOAT, R32G32_(TYPELESS/FLOAT), R8G8B8A8_UNORM, R8G8B8A8_UINT.
No half-floats (10, 34), no R10G10B10A2, nothing dropped by the slot map, the legacy stride equals the Gen9
stride in every case. Gen9 element order is P,N,T,BW,BI,C,TC; legacy bit order is P,BW,BI,N,C,TC...,Tangent, so
conversion is a per-element reorder at the same stride. Slot -> legacy flag bit: 0 POSITION -> 0, 4 NORMAL -> 3,
8 TANGENT -> 14, 16 BLENDWEIGHT -> 1, 20 BLENDINDICES -> 2, 24/25 COLOR -> 4/5, 28..33 TEXCOORD0-5 -> 6..11.
The six declarations in s95/adder map to legacy flags `0x40DF`, `0xDF`, `0x405F`, `0x40D9`, `0x5F`, `0x1DF`.
Still **error** (never byte-copy) on a format or slot outside this set: CodeWalker's fixed-GTAV1 copy would
corrupt half/packed formats. Index buffers: 16-bit only (refuse 32-bit); largest geometry 5,038 vertices.

### Shaders: the hard part

- All 37 distinct shader names in the Enhanced DLC (mpg9ec) and all 25 in patchday27ng are in CodeWalker's
  `ShadersGen9Conversion.xml` (248 shaders, 5,306 params: 1,277 textures, 1,306 samplers, 2,713 cbuffers,
  10 unknown). s95/s95_hi use 13: vehicle_paint4, vehicle_mesh, vehicle_vehglass_inner, vehicle_vehglass,
  vehicle_interior2, vehicle_shuts, vehicle_lightsemissive, vehicle_badges, vehicle_licenseplate,
  vehicle_dash_emissive, vehicle_dash_emissive_opaque, vehicle_tire, vehicle_detail2.
- Values are right: over 2,192 shader pairs (patchday27ng + adder) every cbuffer value and texture-reference name
  maps exactly via the XML `old` names. A Gen9 cbuffer of 4/8/12/16 B becomes 1 legacy Vec4 (zero padded), 32 B
  -> 2, 64 B -> 4, 80 B -> 5.
- **Legacy parameter subset, order and `FileName` cannot be derived from the Gen9 file.**
  - Legacy has textures first then vectors; vector order is the reverse of the Gen9 cbuffer order in ~80% of
    shaders (not for vehicle_tire), texture order differs from Gen9 in 548 of 1,027 vehicle shader instances.
  - Across 916 legacy files each shader name had exactly **one** ordered parameter list.
  - Params with no `old` (Gen9-only) are dropped: `depthbuffertex`, `diffusecolor2`, `hardalphablend`,
    `bloodzoneadjust`, `dirtlevelmod`, `matwheelprevworldviewproj0`, `hash_*`, all samplers. But some params that
    *do* have an `old` name are absent from every legacy instance (`bDebugDisplayDamageMap/Scale`,
    `DiffuseRampTextureSampler`, `SpecularRampTextureSampler`, `matDiffuseSpecularRampEnabled`,
    `matDiffuseColorTint`, `DirtLevelMod` on badges/dash_emissive): "map via XML, drop Gen9-only" over-produces.
  - The XML mapping is **per shader**: 17 Gen9 names map differently by shader (`specular`, `fresnel`,
    `diffusecolor`, `sundirection`, `hardalphablend`, `emissivemultiplier`, ...).
  - Legacy `FileName` (preset) is `<name>.sps` except 52/236 ydr and 815/4,030 ydd shaders (bucket 3 `default` ->
    `cutout.sps`, `default_tnt` -> `cutout_tnt.sps`, `normal_spec` -> `normal_spec_cutout.sps`, others unresolved
    hashes, four with 0); in vehicles 9 instances (vehicle_vehglass -> `0x84B70F7F`, `default` -> 0). Render
    bucket and mask are preserved.
  - Legacy texture `unknown_1h` is `[0, 2, 3, ...]` (first texture 0, then index + 1), not `i + 2` as CodeWalker and
    `shader.rs:108-111` do. Vector `unknown_1h` follows "160 upward".
  - Legacy `unknown_12h` is 32768 in all vanilla files.
- Five legacy names are missing from `src/names/shader_params.txt`: `diffuse3specmod`,
  `diffuseramptexturesampler`, `matdiffusespecularrampenabled`, `specularramptexturesampler`, `trackammoanimuv`.
- **Design:** ship two data files in `src/names/` (`include_str!`, joaat at load):
  1. `gen9_shaders.txt`, generated from the XML (only renames/drops; samplers omitted; `hash_XXXXXXXX` as
     `#XXXXXXXX`; global rename table plus per-shader overrides for the 17 ambiguous names). ~77 KB prototype.
  2. `legacy_shader_presets.txt`, **harvested from the legacy install**: per `(shader, preset.sps, bucket)` the
     ordered parameter list with kind and Vec4 count. This is the source of truth for subset, order and
     `FileName` (same idea as CodeWalker's `ShaderXmlDataCollection`). `tools/gen9-research/legacy_shader_params.json`
     already has the 25 vehicle shaders. A shader with no legacy equivalent is an error.
- Gen9 texture params point at `TextureBase` stubs (s95: 64, s95_hi: 74, plus null refs); they need legacy
  `TextureRef` stubs with `unknown_32h` set (it lands inside the Gen9 SRV pointer, so it must be set explicitly).

## Fragments (.yft) (verified on adder, sultan and all 596 base vehicles per edition)

Only the drawable content and a few scalars differ; physics, bounds, skeleton, bone transforms and vehicle glass
have identical layout and size (only VFT words and 1-ULP float noise differ).

- Fix-ups: `FragType.Unknown_B0h` (0xB0) is 0 in 596/596 legacy, non-zero in 596/596 Gen9 -> write 0;
  `FragBoneTransforms.Unknown_12h` is 1 legacy / 0 Gen9 -> write 1; `FragDrawable` 0x98-0xA7 `AllModels` pointer
  and size are null in Gen9 -> write them (the reader already falls back to the LOD-0 pointer, `drawable.rs:560`).
- Child drawables have a null ShaderGroup and index the parent's shaders by index: **never reorder or drop
  shaders**; keep geometry and vertex order (VGW shatter data is laid out against the glass geometry: inferred).
- `FragGlassWindow` (112 B legacy / 416 B Gen9) occurs in 0 of 1,374 scanned vehicles: not needed for s95.
  `FragVehicleGlassWindows` ("VGWH", no pointers, `TotalLength` at +8) is present in 246 of 596: copy as an opaque
  blob. Cloth in 6 of 596 (not s95); articulated bodies, lights, DrawableArray, LOD2/3: none.
- Block tree and sizes (CodeWalker `Frag.cs`, same in both editions): FragType 304 B; FragDrawable 336 B
  (DrawableBase 0xA8 + FragMatrix 64 B at 0xB0, bound 0xF0, matrices 0xF8..0x110, name 0x130); FragBoneTransforms
  32 + 48n; FragPhysicsLODGroup 48; FragPhysicsLOD 304; FragPhysTypeGroup 176 (name at +128); GroupNames block
  8n + 8 with **interior pointers** to `Group + 128`; FragPhysTypeChild 256; FragPhysEvtSet 48; FragPhysArchetype 224
  (its Bound is the **same block** as `LOD.Bound`); FragPhysTransforms 32 + 64n.
- rage-formats has no fragment blocks (`yft.rs` is a read-only renderer parser) and no shared `DrawableBase`
  (the 0xA8 base is inline in `Drawable`, `drawable.rs:400-598`). `Composite::read` hardcodes
  `owner_is_fragment: false` (`bounds.rs:1230`).
- **No byte-patching shortcut**: the drawable sub-blocks change size so blocks must move. Recommended: read the
  physics structs as typed blocks that keep their raw bytes (decode only pointers and counts), write them back
  with legacy VFTs; skip the CodeWalker XML/derivation code for now.

## Work breakdown (order)

1. **Data:** harvest `legacy_shader_presets.txt` from the legacy install (a rage-cli subcommand or ignored test
   walking every legacy drawable; `catalog/scan.rs` already iterates them); generate `gen9_shaders.txt` from the
   XML; add the 5 missing names. (M)
2. **Drawable reads:** thread `Edition` through `Drawable`/model/geometry/`ShaderGroup`; `IndexBuffer::read_gen9`
   (S); `VertexBuffer::read_gen9` with `VertexDeclarationG9` parse, format-aware flags/types and per-element
   reorder, erroring on anything outside the verified set (M); `ShaderFx::read_gen9` with ParamInfos, rename,
   filter and order by template, `FileName`, `unknown_12h`, Gen9 `TextureRef` (L).
3. **`convert_ydr_to_legacy`** (reuse `write_ydr`, `ydr.rs:25`) (S); `DrawableDictionary` block (FileBase,
   `Unk_18h = 1`, hashes@0x20, drawables ptr-array@0x30; `Drawable.cs:7027-7110`) and `convert_ydd_to_legacy`
   (M).
4. **Fragments:** `DrawableBase` refactor + `FragDrawable` (M); bounds owner flag (S); fragment blocks, `read_yft`/
   `write_yft` v162 (M); Gen9 dispatch, fix-ups above, `convert_yft_to_legacy` (S on top of 2).
5. **rage-cli:** wire the converters into `resource convert` (`resource_convert.rs`), teach `resource info/dump`
   the edition (it prints garbage shader names for Gen9 today), drop the heuristic Gen9 fallbacks in
   `vertex.rs:176` and `ydd.rs:719` (they read 0x80/0x60 bytes of 64 B structs, and the "no declaration" comment is
   wrong: Gen9 stores a 320 B one at 0x38). (S-M)

Risks: legacy parameter order/subset and `FileName` (the game may bind by order: inferred, so match vanilla
exactly); page placement of vertex data (retail vehicles keep it in system pages); skinned peds were not in the
drawable corpus (vehicles were, and use only the formats above).

## Test oracles

- **Vehicles (best):** all 916 `.yft` in `patchday27ng` exist in both installs (`dlcpacks/patchday27ng`); after the
  reorder 5,653 of 6,036 geometries are byte-identical, 359 are the same triangles renumbered, 23 re-authored
  (buffalo4, comet7, iwagen_hi, picador_hi, shinobi_hi, schwarzer_exh_2*), 1 differs (comet7 high g0). Skeletons
  identical; all 2,192 shaders check out. Also vanilla `adder.yft`/`adder_hi.yft` (patchday10ng): 37/44 and 23/24
  buffers byte-identical, the rest same triangles.
- **Props/maps:** same archives from both installs, e.g. `x64i.rpf` `levels/gta5/_citye/downtown_01/downtown.rpf`
  (314 `.ydd`), `icons.rpf` (63 `.ydr`), `dt1_*` (73 `.ydr`): 245/250 ydr and 3,861/4,037 ydd geometries
  byte-identical after the reorder; the rest are re-authored assets. Skip pairs whose geometry counts differ; allow
  a position epsilon.
- Ship as `#[ignore]` real-file tests gated by env vars pointing at both installs (see
  `tests/real_files.rs::gen9_ytd_converts_like_the_legacy_original`), plus `read_ydr`/`read_yft` of the converted
  bytes, a `rage screenshot` of both, and finally spawning a converted car in FiveM (`console_exclusive`:
  s95, arbitergt, astron2, cyclone2, ignus2).

## Research scripts

`tools/gen9-research/` holds the throwaway Python used to establish all of the above. They read RSC7 themselves
(header 16 B, deflate body, system at `0x50000000`, graphics at `0x60000000`) and take file or folder arguments.

| Script | Purpose |
|---|---|
| `g9probe.py <file> [--json out]` | walk a `.yft`/`.ydr` (legacy or Gen9) to geometries, VB, declaration, IB, ShaderFX, ParamInfos |
| `g9compare.py`, `g9diffgeoms.py`, `g9batch.py <gen9 dir> <legacy dir>` | per-file and pairwise Gen9 vs legacy oracle (vertex multiset, triangle sets, shader values, skeleton) |
| `g9scan.py`, `g9misc.py`, `g9bounds.py` | bulk statistics, FragType/glass/bound checks |
| `g9legshaders.py <legacy folder> [gen9 files]` | harvest legacy ordered shader param lists -> `legacy_shader_params.json` |
| `fragwalk.py`, `cmp*.py`, `shared.py`, `survey.py`, `b0.py` | FragType tree walker, block diffs, shared-pointer and feature survey |
| `stats.py`, `stats2.py`, `cmp.py`, `xmlstats.py`, `xml2.py` | drawable (ydr/ydd) corpus comparison; `ShadersGen9Conversion.xml` statistics |

Installs: Enhanced `...\Grand Theft Auto V Enhanced` (`rage ... --exe GTA5_Enhanced.exe`), legacy `...\Grand
Theft Auto V`. Extract matching archives with `rage extract -r <archive> '<pattern>' -o <dir> --exe <exe>`.
