//! `.ybn` entry points, ported from CodeWalker's `YbnFile` and `YbnXml`/`XmlYbn`: read a bounds resource
//! into a block graph, write one back (resource version 43), and convert between a `.ybn` and its XML
//! (a `<BoundsFile>` holding the root `<Bounds type="...">`).

use anyhow::{bail, Context, Result};

use super::base::PagesInfo;
use super::bounds::BoundBlock;
use super::xml::{child, XmlOut};
use super::{BlockId, Graph, Reader};
use crate::resource::SYSTEM_BASE;

/// Reads a `.ybn` into a graph; returns it with the root bound.
pub fn read_ybn(file: &[u8]) -> Result<(Graph, BlockId)> {
    let mut r = Reader::open(file)?;
    let mut g = Graph::new();
    let root = BoundBlock::read(&mut r, &mut g, SYSTEM_BASE, None)?.context("the resource has no root block")?;
    Ok((g, root))
}

/// Derives what a bound read from a file lacks, lays the graph out and writes it as a `.ybn` (resource version 43).
pub fn write_ybn(g: &mut Graph, root: BlockId) -> Result<Vec<u8>> {
    let pages = g.get::<BoundBlock>(root).common().pages.context("the bound has no pages info")?;
    BoundBlock::prepare_tree(g, root);
    g.build(root, pages, 43)
}

/// `YbnXml.GetXml`: the `<BoundsFile>` document.
pub fn xml_of(g: &Graph, root: BlockId) -> String {
    let mut x = XmlOut::new();
    x.open("BoundsFile");
    g.get::<BoundBlock>(root).write_xml(&mut x, g, None);
    x.close("BoundsFile");
    x.out
}

/// A `.ybn` as CodeWalker's XML.
pub fn dump_ybn_xml(file: &[u8]) -> Result<String> {
    let (g, root) = read_ybn(file)?;
    Ok(xml_of(&g, root))
}

/// `XmlYbn.GetYbn` then a write: the `.ybn` for a `<BoundsFile>` document (a bare `<Bounds>` root is accepted too).
pub fn build_ybn_from_xml(xml: &str) -> Result<Vec<u8>> {
    let doc = roxmltree::Document::parse(xml).context("the XML is not well formed")?;
    let root = doc.root_element();
    let node = if root.tag_name().name() == "Bounds" { Some(root) } else { child(root, "Bounds") };
    let Some(node) = node else { bail!("the XML has no <Bounds> element") };
    let mut g = Graph::new();
    let Some(bound) = BoundBlock::read_xml(node, &mut g, None)? else { bail!("the XML has a bound of type None") };
    let pages = g.add(PagesInfo::default());
    g.get_mut::<BoundBlock>(bound).set_pages(Some(pages));
    write_ybn(&mut g, bound)
}
