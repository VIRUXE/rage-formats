# rage-formats

Parsers for the RAGE resource formats GTA V streams — the RSC7-wrapped
files found inside `.rpf` archives — into plain Rust structs. Getting the
bytes out of an archive is [`rpf-archive`](https://github.com/VIRUXE/rpf-archive-rs)'s
job; drawing them is [`rage-render`](https://github.com/VIRUXE/rage-render)'s.

| Format | Function | What you get |
|---|---|---|
| `.ytd` | `parse_ytd` | textures (`YtdTexture`), decoded by `texture_utils` |
| `.ydr` / `.ydd` / `.yft` | `parse_drawables`, `parse_ydr`, `parse_ydd`, `parse_yft` | `Drawable`s with bounds, LODs, shader group, geometry; `Fragment` children |
| `.ymt` (ped variation) | `parse_ymt` | `PedVariationInfo` |
| `.ytyp` | `parse_archetype_txds` | archetype → texture dictionary bindings |
| `gtxd.ymt` / `gtxd.meta` | `parse_txd_relationships` | texture dictionary parent chain |

```toml
[dependencies]
rage-formats = "0.1"
```

The `image` feature (on by default) adds `to_rgba_image`, `fit_max_size`
and `encode_image`; without it textures still decode to raw RGBA bytes via
`decompress_texture` and the crate has no `image` dependency.

### Drawables

`parse_drawables` reads any drawable-bearing resource — a lone `.ydr`, a
`.ydd` dictionary, or a `.yft` fragment — into one flat list of named
entries, letting the caller stay agnostic about which of the three it was
handed:

```rust
use rage_formats::{parse_drawables, DrawableKind};

let entries = parse_drawables(&ydd_bytes, DrawableKind::Ydd)?;
for entry in &entries {
    println!("{} (0x{:08X}): {} triangle(s)", entry.name, entry.hash, entry.drawable.geometry_count());
}
```

Each `DrawableEntry { hash, name, drawable }` carries a `Drawable` — bounds,
LODs, shader group and geometry. Entries that would otherwise share a name
(dictionary members with no name of their own, or a fragment's extra
drawables with no names array) are automatically given unique `0x…` names
instead of overwriting each other.

### Textures to images

`.ytd` textures decode straight to an `image::RgbaImage`, ready to resize and
encode:

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

## License

[Unlicense](LICENSE) — public domain.
