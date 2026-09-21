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
#[cfg(feature = "meta")]
pub mod ymt;
#[cfg(feature = "meta")]
pub mod ytyp;
#[cfg(feature = "gtxd")]
pub mod gtxd;
#[cfg(feature = "collision")]
pub mod ybn;
#[cfg(feature = "meta")]
pub mod ymap;
#[cfg(feature = "nav")]
pub mod ynv;
pub mod texture_utils;
#[cfg(feature = "gtxd")]
mod rbf;

pub use hash::rage_joaat;
pub use math::{Vec2, Vec3, Vec4, Mat4};
pub use resource::{build_rsc7, is_fxap, FXAP_MAGIC, build_rsc7_paged, prepare_rsc7, rsc7_flags_for_pages, rsc7_flags_for_size, resource_size_from_flags, resource_version_from_flags,
                   RSC7_MAGIC, RSC8_MAGIC, SYSTEM_BASE, GRAPHICS_BASE};
pub use ytd::{parse_ytd, TextureFormat, YtdTexture};
pub use ydd::{parse_ydd, parse_ydr, parse_drawables, Drawable, DrawableBounds, DrawableEntry,
              DrawableGeometry, DrawableKind, DrawableLod, DrawableModel, GeometryBounds,
              IndexBuffer, LodLevel, ShaderFx, ShaderGroup, ShaderParameter, ShaderParameterValue,
              UnifiedVertex, VertexAttribute, VertexAttributeValue, VertexBuffer,
              VertexBufferLayout, VertexComponent, VertexComponentType, VertexDeclaration,
              VertexSemantic, BUMP_SAMPLER, DIFFUSE_SAMPLER, SPEC_SAMPLER, TEXTURE_SAMPLER};
pub use vertex::{parse_gen9_declaration, G9_DECLARATION_SIZE, G9_FORMAT_COUNT};
pub use yft::{parse_yft, wheel_slot, Fragment, FragmentChild, FragmentPart, WheelSlot};
#[cfg(feature = "meta")]
pub use ymt::{parse_ymt, PedVariationInfo};
#[cfg(feature = "meta")]
pub use ytyp::{parse_archetype_txds, parse_ytyp, Archetype, ArchetypeTxd, MloDef, MloEntitySet, MloPortal, MloRoom, Ytyp};
#[cfg(feature = "gtxd")]
pub use gtxd::{parse_txd_relationships, TxdRelationship};
#[cfg(feature = "meta")]
pub use ymap::{parse_ymap_entities, parse_ymap_mlo_instances, MloInstance, YmapEntity};
#[cfg(feature = "collision")]
pub use ybn::{parse_ybn, Bound, BoundGeometry, BoundKind, BoundTransform, BoundTriangle, Triangle, Ybn};
#[cfg(feature = "nav")]
pub use ynv::{parse_ynv, serialize_ynv, cell_bounds, cell_file_name, cell_for_position, NavEdge, NavEdgeEnd, NavPoint,
              NavPoly, NavPortal, Ynv, ADJACENT_NONE};
pub use texture_utils::decompress_texture;
#[cfg(feature = "image")]
pub use texture_utils::{to_rgba_image, fit_max_size, encode_image, ImageFormat};
#[cfg(feature = "image")]
pub use image;
