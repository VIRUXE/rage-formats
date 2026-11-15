use super::chunk::{hex, unhex};
use crate::ytd::{full_mip_count, level_size, stride_for, to_dds_layout, to_ytd_layout};
use crate::{rage_joaat, TextureFormat, YtdTexture};
use anyhow::{bail, ensure, Context, Result};

#[derive(Debug, Clone)]
pub struct NativeTexture {
    pub platform: u32,
    pub filter_flags: u32,
    pub name: String,
    pub mask: String,
    pub raster_format: u32,
    pub d3d_format: u32,
    pub width: u16,
    pub height: u16,
    pub depth: u8,
    pub raster_type: u8,
    pub flags: u8,
    pub palette: Vec<u8>,
    pub mips: Vec<Vec<u8>>,
    original_header: Vec<u8>,
    trailing: Vec<u8>,
}

fn word(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}
fn text(bytes: &[u8]) -> Result<String> {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).context("texture name is not UTF-8")
}

impl NativeTexture {
    pub fn to_json(&self, source_index: usize) -> json::JsonValue {
        json::object! {
            source_index:source_index, platform:self.platform, filter_flags:self.filter_flags,
            name:self.name.clone(),mask:self.mask.clone(),raster_format:self.raster_format,d3d_format:self.d3d_format,
            width:self.width,height:self.height,depth:self.depth,raster_type:self.raster_type,flags:self.flags,
            palette:hex(&self.palette),mips:json::JsonValue::Array(self.mips.iter().map(|m|hex(m).into()).collect())
        }
    }

    pub fn edited(&self, value: &json::JsonValue) -> Result<Self> {
        for (key, _) in value.entries() {
            ensure!(
                matches!(
                    key,
                    "source_index"
                        | "platform"
                        | "filter_flags"
                        | "name"
                        | "mask"
                        | "raster_format"
                        | "d3d_format"
                        | "width"
                        | "height"
                        | "depth"
                        | "raster_type"
                        | "flags"
                        | "palette"
                        | "mips"
                ),
                "unknown texture field '{key}'"
            );
        }
        let mut out = self.clone();
        out.platform = value["platform"]
            .as_u32()
            .context("texture platform must be u32")?;
        out.filter_flags = value["filter_flags"]
            .as_u32()
            .context("texture filter_flags must be u32")?;
        out.name = value["name"]
            .as_str()
            .context("texture needs a name")?
            .to_string();
        out.mask = value["mask"]
            .as_str()
            .context("texture needs a mask string")?
            .to_string();
        out.raster_format = value["raster_format"]
            .as_u32()
            .context("texture raster_format must be u32")?;
        out.d3d_format = value["d3d_format"]
            .as_u32()
            .context("texture d3d_format must be u32")?;
        out.width = value["width"]
            .as_u16()
            .context("texture width must be u16")?;
        out.height = value["height"]
            .as_u16()
            .context("texture height must be u16")?;
        out.depth = value["depth"].as_u8().context("texture depth must be u8")?;
        out.raster_type = value["raster_type"]
            .as_u8()
            .context("texture raster_type must be u8")?;
        out.flags = value["flags"].as_u8().context("texture flags must be u8")?;
        out.palette = unhex(
            value["palette"]
                .as_str()
                .context("texture palette must be hex")?,
        )?;
        ensure!(value["mips"].is_array(), "texture mips must be an array");
        ensure!(value["mips"].len() <= 14, "too many texture mip levels");
        out.mips = value["mips"]
            .members()
            .map(|v| unhex(v.as_str().context("mip must be hex")?))
            .collect::<Result<_>>()?;
        out.validate()?;
        Ok(out)
    }

    pub(crate) fn preserve_metadata(&mut self, old: &Self) {
        self.original_header = old.original_header.clone();
        self.trailing = old.trailing.clone();
    }

    /// Standard DDS preserves every source mip, including rectangular chains
    /// whose tiny tail levels cannot use the existing legacy YTD adapter.
    pub fn to_dds(&self) -> Result<Vec<u8>> {
        let direct = self
            .block_format()
            .filter(|f| *f != TextureFormat::DXT1 || !self.has_alpha());
        let format = direct.unwrap_or(TextureFormat::A8B8G8R8);
        let mut top = YtdTexture {
            name: self.name.clone(),
            name_hash: 0,
            width: self.width,
            height: self.height,
            depth: 1,
            format,
            levels: self.available_mips() as u8,
            stride: stride_for(format, self.width, self.height),
            pixel_data: Vec::new(),
        };
        top.pixel_data = if direct.is_some() {
            self.mips[0].clone()
        } else {
            self.decode_mip(0)?
        };
        let mut bytes = top.to_dds();
        bytes.truncate(128);
        for (level, mip) in self.mips.iter().take(self.available_mips()).enumerate() {
            if direct.is_some() {
                let (w, h) = self.mip_dimensions(level)?;
                bytes.extend_from_slice(&mip[..level_size(format, w, h)]);
            } else {
                bytes.extend(self.decode_mip(level)?);
            }
        }
        Ok(bytes)
    }
    pub fn parse(data: &[u8]) -> Result<Self> {
        ensure!(data.len() >= 88, "native texture header is truncated");
        let platform = word(data, 0);
        ensure!(
            matches!(platform, 8 | 9),
            "unsupported native texture platform {platform} (PC D3D8/9 required)"
        );
        let raster_format = word(data, 72);
        ensure!(
            raster_format & 0x6000 != 0x6000,
            "texture enables both PAL4 and PAL8"
        );
        let palette_bytes = if raster_format & 0x4000 != 0 {
            128
        } else if raster_format & 0x2000 != 0 {
            1024
        } else {
            0
        };
        let mut at: usize = 88 + palette_bytes;
        let palette = data
            .get(88..at)
            .context("texture palette is truncated")?
            .to_vec();
        let mut mips = Vec::new();
        for level in 0..data[85] {
            let length_bytes = data
                .get(at..at + 4)
                .with_context(|| format!("mip {level} size is truncated"))?;
            let length = word(length_bytes, 0) as usize;
            at += 4;
            let end = at.checked_add(length).context("mip size overflow")?;
            mips.push(
                data.get(at..end)
                    .with_context(|| format!("mip {level} data is truncated"))?
                    .to_vec(),
            );
            at = end;
        }
        let texture = Self {
            platform,
            filter_flags: word(data, 4),
            name: text(&data[8..40])?,
            mask: text(&data[40..72])?,
            raster_format,
            d3d_format: word(data, 76),
            width: u16::from_le_bytes(data[80..82].try_into().unwrap()),
            height: u16::from_le_bytes(data[82..84].try_into().unwrap()),
            depth: data[84],
            raster_type: data[86],
            flags: data[87],
            palette,
            mips,
            original_header: data[..88].to_vec(),
            trailing: data[at..].to_vec(),
        };
        texture.validate()?;
        Ok(texture)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.platform, 8 | 9),
            "unsupported native texture platform {}",
            self.platform
        );
        ensure!(
            (1..=8192).contains(&self.width) && (1..=8192).contains(&self.height),
            "invalid texture dimensions {}x{}",
            self.width,
            self.height
        );
        ensure!(
            !self.name.is_empty()
                && self.name.is_ascii()
                && self.name.len() <= 32
                && !self.name.contains('\0'),
            "texture name must contain 1..32 ASCII bytes"
        );
        ensure!(
            self.mask.is_ascii() && self.mask.len() <= 32 && !self.mask.contains('\0'),
            "invalid texture mask name"
        );
        ensure!(
            !self.mips.is_empty()
                && self.mips.len() <= (16 - self.width.max(self.height).leading_zeros()) as usize,
            "invalid mip count {}",
            self.mips.len()
        );
        ensure!(
            self.platform != 9 || self.flags & 2 == 0,
            "cubemap TXD rasters are not supported"
        );
        ensure!(
            self.raster_format & 0x6000 != 0x6000,
            "texture enables both PAL4 and PAL8"
        );
        if self.platform == 8 {
            ensure!(
                matches!(self.flags, 0 | 1 | 3 | 5),
                "unsupported D3D8 compression code {}",
                self.flags
            );
        }
        if self.platform == 9 && self.block_format().is_none() {
            ensure!(
                matches!(
                    self.d3d_format,
                    20 | 21 | 22 | 23 | 24 | 25 | 26 | 28 | 32 | 41 | 50 | 51
                ),
                "unsupported D3D9 raster format {}",
                self.d3d_format
            );
        }
        let format = self.block_format();
        if format.is_none() {
            ensure!(
                matches!(self.depth, 4 | 8 | 16 | 24 | 32),
                "unsupported raster depth {}",
                self.depth
            );
        }
        let palette = if self.raster_format & 0x4000 != 0 {
            128
        } else if self.raster_format & 0x2000 != 0 {
            1024
        } else {
            0
        };
        ensure!(
            self.palette.len() == palette,
            "palette size does not match raster format"
        );
        let mut missing = false;
        for (level, mip) in self.mips.iter().enumerate() {
            let (w, h) = self.mip_dimensions(level)?;
            if mip.is_empty() {
                ensure!(
                    level > 0 && format.is_some() && (w < 4 || h < 4),
                    "empty pixel data for non-tail mip {level}"
                );
                missing = true;
                continue;
            }
            ensure!(
                !missing,
                "mip {level} contains data after an empty tail record"
            );
            let minimum = if let Some(format) = format {
                level_size(format, w, h)
            } else {
                (w as usize * self.depth as usize).div_ceil(8) * h as usize
            };
            ensure!(
                mip.len() >= minimum,
                "texture '{}' mip {level} needs {minimum} bytes, has {}",
                self.name,
                mip.len()
            );
        }
        Ok(())
    }

    pub fn mip_dimensions(&self, level: usize) -> Result<(u16, u16)> {
        ensure!(level < self.mips.len(), "mip index {level} out of range");
        Ok(((self.width >> level).max(1), (self.height >> level).max(1)))
    }

    pub fn block_format(&self) -> Option<TextureFormat> {
        let code = if self.platform == 8 {
            match self.flags {
                1 => 0x31545844,
                3 => 0x33545844,
                5 => 0x35545844,
                _ => 0,
            }
        } else {
            self.d3d_format
        };
        match code {
            0x31545844 => Some(TextureFormat::DXT1),
            0x33545844 => Some(TextureFormat::DXT3),
            0x35545844 => Some(TextureFormat::DXT5),
            _ => None,
        }
    }

    pub fn available_mips(&self) -> usize {
        self.mips
            .iter()
            .position(Vec::is_empty)
            .unwrap_or(self.mips.len())
    }

    pub fn has_alpha(&self) -> bool {
        if self.platform == 8 {
            self.d3d_format != 0
        } else {
            self.flags & 1 != 0
        }
    }

    pub fn decode_mip(&self, level: usize) -> Result<Vec<u8>> {
        let (w, h) = self.mip_dimensions(level)?;
        let data = &self.mips[level];
        ensure!(!data.is_empty(), "mip {level} has no source pixel data");
        if let Some(format) = self.block_format() {
            if format == TextureFormat::DXT1 && self.has_alpha() {
                let mut argb = vec![0u32; w as usize * h as usize];
                texture2ddecoder::decode_bc1a(data, w as usize, h as usize, &mut argb)
                    .map_err(|e| anyhow::anyhow!("DXT1 alpha decoding failed: {e}"))?;
                return Ok(argb
                    .into_iter()
                    .flat_map(|c| [(c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8])
                    .collect());
            }
            return crate::decompress_texture(&YtdTexture {
                name: self.name.clone(),
                name_hash: 0,
                width: w,
                height: h,
                depth: 1,
                format,
                levels: 1,
                stride: stride_for(format, w, h),
                pixel_data: data.clone(),
            });
        }
        let row_bytes = (w as usize * self.depth as usize).div_ceil(8);
        let stride = if data.len() % h as usize == 0 {
            data.len() / h as usize
        } else {
            row_bytes
        };
        ensure!(stride >= row_bytes, "raster row is truncated");
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h as usize {
            for x in 0..w as usize {
                let row = &data[y * stride..y * stride + row_bytes];
                let rgba = if !self.palette.is_empty() {
                    let index = if self.depth == 4 {
                        (row[x / 2] >> ((x % 2) * 4)) & 15
                    } else {
                        row[x]
                    };
                    let at = index as usize * 4;
                    self.palette
                        .get(at..at + 4)
                        .context("palette index exceeds palette")?
                        .try_into()
                        .unwrap()
                } else {
                    self.decode_pixel(row, x)?
                };
                out.extend_from_slice(&rgba);
            }
        }
        Ok(out)
    }

    fn decode_pixel(&self, row: &[u8], x: usize) -> Result<[u8; 4]> {
        if self.platform == 9 {
            if self.d3d_format == 28 && self.depth == 8 {
                return Ok([255, 255, 255, row[x]]);
            }
            if self.d3d_format == 51 && self.depth == 16 {
                return Ok([row[x * 2], row[x * 2], row[x * 2], row[x * 2 + 1]]);
            }
        }
        let raster = self.raster_format & 0xf00;
        let c5 = |v: u16| ((v * 255 + 15) / 31) as u8;
        let c6 = |v: u16| ((v * 255 + 31) / 63) as u8;
        match (self.depth, raster) {
            (16, _) => {
                let v = u16::from_le_bytes(row[x * 2..x * 2 + 2].try_into().unwrap());
                Ok(match raster {
                    0x100 => [
                        c5((v >> 10) & 31),
                        c5((v >> 5) & 31),
                        c5(v & 31),
                        if v & 0x8000 != 0 { 255 } else { 0 },
                    ],
                    0x200 => [c5((v >> 11) & 31), c6((v >> 5) & 63), c5(v & 31), 255],
                    0x300 => [
                        ((v >> 8) & 15) as u8 * 17,
                        ((v >> 4) & 15) as u8 * 17,
                        (v & 15) as u8 * 17,
                        ((v >> 12) & 15) as u8 * 17,
                    ],
                    0xa00 => [c5((v >> 10) & 31), c5((v >> 5) & 31), c5(v & 31), 255],
                    _ => bail!("unsupported packed raster 0x{:X}", self.raster_format),
                })
            }
            (24, 0x600) => Ok([row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255]),
            (32, 0x500 | 0x600) => {
                let p = &row[x * 4..x * 4 + 4];
                let alpha = if raster == 0x600 { 255 } else { p[3] };
                Ok(if self.platform == 9 && self.d3d_format == 32 {
                    [p[0], p[1], p[2], alpha]
                } else {
                    [p[2], p[1], p[0], alpha]
                })
            }
            (8, 0x400) => Ok([row[x], row[x], row[x], 255]),
            _ => bail!(
                "unsupported raster format 0x{:X}, depth {}, D3D {}",
                self.raster_format,
                self.depth,
                self.d3d_format
            ),
        }
    }

    pub fn to_ytd_texture(&self) -> Result<YtdTexture> {
        let direct = self.block_format().filter(|format| {
            self.available_mips() <= full_mip_count(*format, self.width, self.height) as usize
                && (*format != TextureFormat::DXT1 || !self.has_alpha())
        });
        let format = direct.unwrap_or(TextureFormat::A8B8G8R8);
        let mut dds = Vec::new();
        for level in 0..self.available_mips() {
            if direct.is_some() {
                let (w, h) = self.mip_dimensions(level)?;
                dds.extend_from_slice(&self.mips[level][..level_size(format, w, h)]);
            } else {
                dds.extend(self.decode_mip(level)?);
            }
        }
        let stride = stride_for(format, self.width, self.height);
        let levels = self.available_mips() as u8;
        let pixel_data = to_ytd_layout(format, self.width, self.height, stride, levels, &dds);
        ensure!(to_dds_layout(format,self.width,self.height,stride,levels,&pixel_data)==dds,
            "texture '{}' has mip tails that cannot be represented losslessly by the current YTD layout",self.name);
        Ok(YtdTexture {
            name: self.name.clone(),
            name_hash: rage_joaat(&self.name.to_lowercase()),
            width: self.width,
            height: self.height,
            depth: 1,
            format,
            levels,
            stride,
            pixel_data,
        })
    }

    pub fn from_ytd(texture: &YtdTexture, game: super::Game) -> Result<Self> {
        ensure!(texture.depth <= 1, "TXD cannot represent volume textures");
        let platform = if game == super::Game::SanAndreas {
            9u32
        } else {
            8
        };
        let direct = matches!(
            texture.format,
            TextureFormat::DXT1 | TextureFormat::DXT3 | TextureFormat::DXT5
        );
        let mut header = vec![0u8; 88];
        header[..4].copy_from_slice(&platform.to_le_bytes());
        header[4..8].copy_from_slice(&0x1101u32.to_le_bytes());
        write_name(&mut header[8..40], &texture.name)?;
        let raster = match texture.format {
            TextureFormat::DXT1 => 0x200u32,
            TextureFormat::DXT3 | TextureFormat::DXT5 => 0x300,
            _ => 0x500,
        };
        header[72..76]
            .copy_from_slice(&(raster | if texture.levels > 1 { 0x8000 } else { 0 }).to_le_bytes());
        let code = if platform == 8 {
            1
        } else if direct {
            texture.format as u32
        } else {
            21
        };
        header[76..80].copy_from_slice(&code.to_le_bytes());
        header[80..82].copy_from_slice(&texture.width.to_le_bytes());
        header[82..84].copy_from_slice(&texture.height.to_le_bytes());
        header[84] = if direct { 16 } else { 32 };
        header[85] = texture.levels;
        header[86] = 4;
        header[87] = if platform == 8 {
            match texture.format {
                TextureFormat::DXT1 => 1,
                TextureFormat::DXT3 => 3,
                TextureFormat::DXT5 => 5,
                _ => 0,
            }
        } else if direct {
            9
        } else {
            1
        };
        let chain = to_dds_layout(
            texture.format,
            texture.width,
            texture.height,
            texture.stride,
            texture.levels,
            &texture.pixel_data,
        );
        let mut at = 0;
        let (mut w, mut h) = (texture.width, texture.height);
        for _ in 0..texture.levels {
            let length = level_size(texture.format, w, h);
            let mip = chain
                .get(at..at + length)
                .context("source mip chain truncated")?;
            let data = if direct {
                mip.to_vec()
            } else {
                let mut source = texture.clone();
                source.width = w;
                source.height = h;
                source.levels = 1;
                source.stride = stride_for(source.format, w, h);
                source.pixel_data = mip.to_vec();
                let rgba = crate::decompress_texture(&source)?;
                rgba.chunks_exact(4)
                    .flat_map(|p| [p[2], p[1], p[0], p[3]])
                    .collect()
            };
            header.extend_from_slice(&(data.len() as u32).to_le_bytes());
            header.extend(data);
            at += length;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        Self::parse(&header)
    }

    pub fn build(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut data = self.original_header.clone();
        data[..4].copy_from_slice(&self.platform.to_le_bytes());
        data[4..8].copy_from_slice(&self.filter_flags.to_le_bytes());
        // Preserve garbage after a source name's first NUL unless that name changes.
        if text(&data[8..40])? != self.name {
            write_name(&mut data[8..40], &self.name)?;
        }
        if text(&data[40..72])? != self.mask {
            write_name(&mut data[40..72], &self.mask)?;
        }
        data[72..76].copy_from_slice(&self.raster_format.to_le_bytes());
        data[76..80].copy_from_slice(&self.d3d_format.to_le_bytes());
        data[80..82].copy_from_slice(&self.width.to_le_bytes());
        data[82..84].copy_from_slice(&self.height.to_le_bytes());
        data[84] = self.depth;
        data[85] = self.mips.len() as u8;
        data[86] = self.raster_type;
        data[87] = self.flags;
        data.extend_from_slice(&self.palette);
        for mip in &self.mips {
            data.extend_from_slice(&(u32::try_from(mip.len())?).to_le_bytes());
            data.extend(mip);
        }
        data.extend_from_slice(&self.trailing);
        Ok(data)
    }
}

fn write_name(bytes: &mut [u8], name: &str) -> Result<()> {
    ensure!(
        name.is_ascii() && name.len() <= bytes.len() && !name.contains('\0'),
        "invalid TXD name '{name}'"
    );
    bytes.fill(0);
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    Ok(())
}
