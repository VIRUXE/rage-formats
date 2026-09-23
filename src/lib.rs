//! RAGE resource formats: the RSC7-wrapped files GTA V streams — textures
//! (`.ytd`), drawables (`.ydr`/`.ydd`), fragments (`.yft`), archetype and
//! ped metadata (`.ytyp`/`.ymt`), texture-dictionary relationships
//! (`gtxd.ymt`) — parsed from bytes into plain Rust structs. Getting those
//! bytes out of an `.rpf` is the `rpf-archive` crate's job; drawing them is
//! `rage-render`'s.

pub mod hash;
pub mod math;
pub mod resource;
pub mod vertex;
pub mod ytd;
pub mod ydd;
pub mod yft;
pub mod ymt;
pub mod ytyp;
pub mod gtxd;
pub mod ybn;
pub mod ymap;
pub mod ynv;
pub mod value;
pub mod pso;
pub mod meta_schema;
pub mod names;
pub mod xml;
pub mod json;
pub mod ymf;
pub mod texture_utils;
pub mod dds;
#[cfg(feature = "encode")]
pub mod texture_encode;
mod rbf;

pub use hash::rage_joaat;
pub use math::{Vec2, Vec3, Vec4, Mat4};
pub use resource::{build_rsc7, build_rsc7_with_flags, pack_pages, rsc7_page_count, PagedLayout, is_fxap, FXAP_MAGIC, build_rsc7_paged, prepare_rsc7, rsc7_flags_for_pages, rsc7_flags_for_size, resource_size_from_flags, resource_version_from_flags,
                   RSC7_MAGIC, RSC8_MAGIC, SYSTEM_BASE, GRAPHICS_BASE};
pub use ytd::{parse_ytd, serialize_ytd, full_mip_count, level_size, mip_chain_size, stride_for, TextureFormat, YtdTexture, YTD_VERSION};
pub use dds::parse_dds;
#[cfg(feature = "encode")]
pub use texture_encode::{auto_format, encode_texture, is_normal_map_name, EncodeFormat};
pub use ydd::{parse_ydd, parse_ydr, parse_drawables, Drawable, DrawableBounds, DrawableEntry,
              DrawableGeometry, DrawableKind, DrawableLod, DrawableModel, GeometryBounds,
              IndexBuffer, LodLevel, ShaderFx, ShaderGroup, ShaderParameter, ShaderParameterValue,
              UnifiedVertex, VertexAttribute, VertexAttributeValue, VertexBuffer,
              VertexBufferLayout, VertexComponent, VertexComponentType, VertexDeclaration,
              VertexSemantic, BUMP_SAMPLER, DIFFUSE_SAMPLER, SPEC_SAMPLER, TEXTURE_SAMPLER};
pub use yft::{parse_yft, wheel_slot, Fragment, FragmentChild, FragmentPart, WheelSlot};
pub use ymt::{parse_ymt, PedVariationInfo};
pub use ytyp::{parse_archetype_txds, parse_ytyp, Archetype, ArchetypeTxd, MloDef, MloEntitySet, MloPortal, MloRoom, Ytyp};
pub use gtxd::{parse_txd_relationships, TxdRelationship};
pub use ymap::{parse_ymap, parse_ymap_entities, parse_ymap_header, parse_ymap_mlo_instances, set_map_name, MloInstance, Ymap, YmapEntity, YmapHeader};
pub use ybn::{parse_ybn, Bound, BoundGeometry, BoundKind, BoundTransform, BoundTriangle, Triangle, Ybn};
pub use ynv::{parse_ynv, serialize_ynv, cell_bounds, cell_file_name, cell_for_position, NavEdge, NavEdgeEnd, NavPoint,
              NavPoly, NavPortal, Ynv, ADJACENT_NONE};
pub use value::{MetaArray, MetaDump, MetaStruct, MetaValue};
pub use pso::{dump_pso, is_pso, parse_pso, PsoFile, PSO_MAGIC};
pub use meta_schema::{dump_meta, parse_meta, HashSite, MetaFile};
pub use names::NameTable;
pub use xml::{from_xml, to_xml};
pub use json::to_json;
pub use ymf::{dump_metadata, dump_ymf, parse_ymf, Dependencies, HashName, HdTxdBinding, ImapDependency, InteriorBounds, Manifest, ManifestFormat, MapDataGroup, MetaContainer};
pub use texture_utils::decompress_texture;
#[cfg(feature = "image")]
pub use texture_utils::{to_rgba_image, fit_max_size, encode_image, ImageFormat};
#[cfg(feature = "image")]
pub use image;
