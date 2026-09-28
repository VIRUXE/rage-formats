//! `.ydr` entry points, ported from CodeWalker's `YdrFile` (`YdrXml.GetXml`, `XmlYdr.GetYdr`):
//! read a drawable resource into a block graph, write one back (resource version 165), and
//! convert between a `.ydr` and its XML.

use std::path::Path;

use anyhow::{Context, Result};

use super::drawable::Drawable;
use super::xml::XmlOut;
use super::{BlockId, Graph, Reader};
use crate::names::NameTable;
use crate::resource::SYSTEM_BASE;

/// Reads a `.ydr` into a graph; returns it with the [`Drawable`] root.
pub fn read_ydr(file: &[u8]) -> Result<(Graph, BlockId)> {
    let mut r = Reader::open(file)?;
    let mut g = Graph::new();
    let root = Drawable::read(&mut r, &mut g, SYSTEM_BASE)?.context("the resource has no root block")?;
    Ok((g, root))
}

/// Lays the graph out and writes it as a `.ydr` (resource version 165).
pub fn write_ydr(g: &mut Graph, root: BlockId) -> Result<Vec<u8>> {
    let pages = g.get::<Drawable>(root).pages.context("the drawable has no pages info")?;
    g.build(root, pages, 165)
}

/// `YdrXml.GetXml`: the `<Drawable>` document.
pub fn xml_of(g: &Graph, root: BlockId, names: &NameTable, dds_dir: Option<&Path>) -> Result<String> {
    let mut x = XmlOut::new();
    x.open("Drawable");
    g.get::<Drawable>(root).write_xml(&mut x, g, names, dds_dir)?;
    x.close("Drawable");
    Ok(x.out)
}

/// A `.ydr` as CodeWalker's XML; embedded textures are written to `dds_dir` when given.
pub fn dump_ydr_xml(file: &[u8], names: &NameTable, dds_dir: Option<&Path>) -> Result<String> {
    let (g, root) = read_ydr(file)?;
    xml_of(&g, root, names, dds_dir)
}

/// `XmlYdr.GetYdr` then a write: the `.ydr` for a `<Drawable>` document.
pub fn build_ydr_from_xml(xml: &str, dds_dir: Option<&Path>) -> Result<Vec<u8>> {
    let doc = roxmltree::Document::parse(xml).context("the XML is not well formed")?;
    let mut g = Graph::new();
    let root = Drawable::read_xml(doc.root_element(), &mut g, dds_dir)?;
    write_ydr(&mut g, root)
}
