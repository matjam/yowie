//! A small element tree built from quick-xml events.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::{Error, Result};

/// One XML element: its name, attributes, child elements and text.
#[derive(Debug, Default)]
pub(crate) struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    /// Character data directly inside the element, entities resolved.
    pub text: String,
}

impl Element {
    /// Returns the attribute called `name`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Returns the first child element called `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Iterates over the child elements called `name`.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |c| c.name == name)
    }
}

fn xml_err(e: impl std::fmt::Display) -> Error {
    Error::Xml(e.to_string())
}

fn element(start: &BytesStart<'_>) -> Result<Element> {
    let name = start.local_name().as_ref().to_string();
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(xml_err)?;
        let key = attr.key.local_name().as_ref().to_string();
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(xml_err)?
            .into_owned();
        attrs.push((key, value));
    }
    Ok(Element {
        name,
        attrs,
        ..Default::default()
    })
}

/// Resolves the name inside `&name;`: the five predefined entities and
/// numeric character references.
fn resolve_ref(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// Parses an XML document and returns its root element.
pub(crate) fn parse(data: &[u8]) -> Result<Element> {
    let text = std::str::from_utf8(data).map_err(xml_err)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    loop {
        match reader.read_event().map_err(xml_err)? {
            Event::Start(e) => stack.push(element(&e)?),
            Event::Empty(e) => finish(element(&e)?, &mut stack, &mut root),
            Event::End(_) => {
                let done = stack.pop().ok_or_else(|| xml_err("unbalanced end tag"))?;
                finish(done, &mut stack, &mut root);
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&t.xml10_content());
                }
            }
            Event::CData(c) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&c);
                }
            }
            Event::GeneralRef(r) => {
                let ch =
                    resolve_ref(&r).ok_or_else(|| xml_err(format!("unknown entity &{};", &*r)))?;
                if let Some(top) = stack.last_mut() {
                    top.text.push(ch);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(xml_err("unclosed element"));
    }
    root.ok_or_else(|| xml_err("document has no root element"))
}

fn finish(done: Element, stack: &mut [Element], root: &mut Option<Element>) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(done),
        None => *root = Some(done),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_tree_with_entities_and_cdata() {
        let root = parse(
            b"<?xml version=\"1.0\"?><a x=\"1&amp;2\"><!-- c --><b/><c>t&lt;1&#33;<![CDATA[<raw>]]></c></a>",
        )
        .unwrap();
        assert_eq!(root.name, "a");
        assert_eq!(root.attr("x"), Some("1&2"));
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.child("c").unwrap().text, "t<1!<raw>");
        assert!(root.child("b").unwrap().children.is_empty());
    }

    #[test]
    fn malformed_documents_fail() {
        for bad in ["", "<a>", "<a></b>", "<a>&bogus;</a>", "text"] {
            assert!(
                matches!(parse(bad.as_bytes()), Err(Error::Xml(_))),
                "{bad:?}"
            );
        }
    }
}
