//! Independently implemented classic PC GTA formats. RPF/IMG supplies bytes;
//! these codecs retain unknown chunks and never infer a platform from a name.
pub mod chunk;
pub mod document;
pub mod txd;
pub mod raster;
mod reader;
pub mod plugins;
pub mod geometry;
pub mod dff;
mod dff_drawable;
pub use chunk::{parse_chunks, write_chunks, Chunk};
pub use document::{Document, Format};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game { Iii, ViceCity, SanAndreas }
