//! # DOM implementation for html5ever
//!
//! A minimal `RcDom` that implements html5ever's [`TreeSink`] trait and stores
//! nodes as `Rc<Node>` with parent/child/sibling links, allowing efficient tree
//! traversal by the converter.
//!
//! ## Safety
//!
//! All mutation goes through `RefCell` borrows that are released before the
//! tree is walked, so no borrow is held across a call back into the converter.

use std::{borrow::Cow, cell::RefCell, rc::Rc};

use html5ever::{
    Attribute, ParseOpts, QualName,
    interface::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink},
    parse_document,
    tendril::{StrTendril, TendrilSink},
};

pub(super) struct Node {
    pub(super) data: NodeData,
    parent: RefCell<Option<Handle>>,
    first_child: RefCell<Option<Handle>>,
    last_child: RefCell<Option<Handle>>,
    prev_sibling: RefCell<Option<Handle>>,
    next_sibling: RefCell<Option<Handle>>,
}

// NodeData variants are required by html5ever's TreeSink trait for a complete
// DOM. ProcessingInstruction is created by the parser but not processed for
// Markdown output.
#[allow(dead_code)]
pub(super) enum NodeData {
    Document,
    Doctype {
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    },
    Text {
        contents: RefCell<StrTendril>,
    },
    Comment {
        contents: StrTendril,
    },
    Element {
        name: QualName,
        attrs: RefCell<Vec<Attribute>>,
        template_contents: Option<Handle>,
        mathml_annotation_xml_integration_point: bool,
    },
    ProcessingInstruction {
        target: StrTendril,
        contents: StrTendril,
    },
}

pub(super) type Handle = Rc<Node>;

fn detach(node: &Handle) {
    let parent = node.parent.borrow_mut().take();
    let prev = node.prev_sibling.borrow_mut().take();
    let next = node.next_sibling.borrow_mut().take();

    if let Some(ref next_n) = next {
        (*next_n.prev_sibling.borrow_mut()).clone_from(&prev);
    } else if let Some(ref parent_n) = parent {
        (*parent_n.last_child.borrow_mut()).clone_from(&prev);
    }
    if let Some(ref prev_n) = prev {
        *prev_n.next_sibling.borrow_mut() = next;
    } else if let Some(ref parent_n) = parent {
        *parent_n.first_child.borrow_mut() = None;
    }
}

fn append_child(parent: &Handle, child: &Handle) {
    detach(child);
    *child.parent.borrow_mut() = Some(Rc::clone(parent));
    if let Some(last) = parent.last_child.borrow_mut().take() {
        *child.prev_sibling.borrow_mut() = Some(Rc::clone(&last));
        debug_assert!(last.next_sibling.borrow().is_none());
        *last.next_sibling.borrow_mut() = Some(Rc::clone(child));
    } else {
        debug_assert!(parent.first_child.borrow().is_none());
        *parent.first_child.borrow_mut() = Some(Rc::clone(child));
    }
    *parent.last_child.borrow_mut() = Some(Rc::clone(child));
}

fn insert_before(sibling: &Handle, new_node: &Handle) {
    detach(new_node);
    (*new_node.parent.borrow_mut()).clone_from(&sibling.parent.borrow());
    *new_node.next_sibling.borrow_mut() = Some(Rc::clone(sibling));
    let prev = sibling.prev_sibling.borrow_mut().take();
    if let Some(ref prev_n) = prev {
        *new_node.prev_sibling.borrow_mut() = Some(Rc::clone(prev_n));
        *prev_n.next_sibling.borrow_mut() = Some(Rc::clone(new_node));
    } else if let Some(parent_n) = sibling.parent.borrow().as_ref() {
        *parent_n.first_child.borrow_mut() = Some(Rc::clone(new_node));
    }
    *sibling.prev_sibling.borrow_mut() = Some(Rc::clone(new_node));
}

pub(super) fn iter_children(node: &Handle) -> ChildIter {
    ChildIter {
        next: node.first_child.borrow().clone(),
    }
}

pub(super) struct ChildIter {
    next: Option<Handle>,
}

impl Iterator for ChildIter {
    type Item = Handle;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next.take()?;
        self.next.clone_from(&current.next_sibling.borrow());
        Some(current)
    }
}

pub(super) struct RcDom {
    pub(super) document: Handle,
    quirks_mode: RefCell<QuirksMode>,
}

/// Allocate a fresh node with no parent/child/sibling links.
fn make_node(data: NodeData) -> Handle {
    Rc::new(Node {
        data,
        parent: RefCell::new(None),
        first_child: RefCell::new(None),
        last_child: RefCell::new(None),
        prev_sibling: RefCell::new(None),
        next_sibling: RefCell::new(None),
    })
}

impl RcDom {
    pub(super) fn parse(html: &[u8]) -> Result<Self, crate::Error> {
        let dom = Self {
            document: Rc::new(Node {
                data: NodeData::Document,
                parent: RefCell::new(None),
                first_child: RefCell::new(None),
                last_child: RefCell::new(None),
                prev_sibling: RefCell::new(None),
                next_sibling: RefCell::new(None),
            }),
            quirks_mode: RefCell::new(QuirksMode::NoQuirks),
        };
        let dom = parse_document(dom, ParseOpts::default())
            .from_utf8()
            .read_from(&mut &html[..])
            .map_err(|e| crate::Error::Parse(format!("{e}")))?;
        Ok(dom)
    }

    fn append_common<P, A>(child: NodeOrText<Handle>, previous: P, append: A)
    where
        P: FnOnce() -> Option<Handle>,
        A: FnOnce(Handle),
    {
        let node = match child {
            NodeOrText::AppendText(text) => {
                if let Some(prev) = previous()
                    && let NodeData::Text { ref contents } = prev.data
                {
                    contents.borrow_mut().push_tendril(&text);
                    return;
                }
                make_node(NodeData::Text {
                    contents: RefCell::new(text),
                })
            }
            NodeOrText::AppendNode(n) => n,
        };
        append(node);
    }
}

impl TreeSink for RcDom {
    type Handle = Handle;
    type Output = Self;
    type ElemName<'a>
        = &'a QualName
    where
        Self: 'a;

    fn finish(self) -> Self::Output {
        self
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> Handle {
        Rc::clone(&self.document)
    }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> Self::ElemName<'a> {
        debug_assert!(
            matches!(&target.data, NodeData::Element { .. }),
            "elem_name called on non-element node"
        );
        match &target.data {
            NodeData::Element { name, .. } => name,
            _ => unreachable!("not an element"),
        }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        make_node(NodeData::Element {
            name,
            attrs: RefCell::new(attrs),
            template_contents: if flags.template {
                Some(make_node(NodeData::Document))
            } else {
                None
            },
            mathml_annotation_xml_integration_point: flags.mathml_annotation_xml_integration_point,
        })
    }

    fn create_comment(&self, text: StrTendril) -> Handle {
        make_node(NodeData::Comment { contents: text })
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> Handle {
        make_node(NodeData::ProcessingInstruction {
            target,
            contents: data,
        })
    }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        let parent = Rc::clone(parent);
        Self::append_common(
            child,
            || parent.last_child.borrow().clone(),
            |node| append_child(&parent, &node),
        );
    }

    fn append_before_sibling(&self, sibling: &Handle, child: NodeOrText<Handle>) {
        let sibling = Rc::clone(sibling);
        Self::append_common(
            child,
            || sibling.prev_sibling.borrow().clone(),
            |node| insert_before(&sibling, &node),
        );
    }

    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        prev_element: &Handle,
        child: NodeOrText<Handle>,
    ) {
        if element.parent.borrow().is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let node = make_node(NodeData::Doctype {
            name,
            public_id,
            system_id,
        });
        append_child(&self.document, &node);
    }

    fn get_template_contents(&self, target: &Handle) -> Handle {
        debug_assert!(
            matches!(
                &target.data,
                NodeData::Element {
                    template_contents: Some(..),
                    ..
                }
            ),
            "get_template_contents called on non-template node"
        );
        match &target.data {
            NodeData::Element {
                template_contents: Some(contents),
                ..
            } => Rc::clone(contents),
            _ => unreachable!("not a template element"),
        }
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        Rc::ptr_eq(x, y)
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        *self.quirks_mode.borrow_mut() = mode;
    }

    fn add_attrs_if_missing(&self, target: &Handle, new_attrs: Vec<Attribute>) {
        if let NodeData::Element { ref attrs, .. } = target.data {
            let mut existing = attrs.borrow_mut();
            for new_attr in new_attrs {
                let already_present = existing.iter().any(|a| a.name == new_attr.name);
                if !already_present {
                    existing.push(new_attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &Handle) {
        detach(target);
    }

    #[allow(clippy::assigning_clones)]
    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        // The loop reads each child's next sibling before reparenting it. The
        // `while let` pattern moves `next`, so `clone_from` cannot replace the
        // assignment below (clippy::assigning_clones suggests it, but the
        // suggestion is unsound here).
        let mut next = node.first_child.borrow().clone();
        while let Some(child) = next {
            debug_assert!(
                child
                    .parent
                    .borrow()
                    .as_ref()
                    .is_some_and(|p| Rc::ptr_eq(p, node))
            );
            next = child.next_sibling.borrow().clone();
            append_child(new_parent, &child);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &Handle) -> bool {
        match &handle.data {
            NodeData::Element {
                mathml_annotation_xml_integration_point,
                ..
            } => *mathml_annotation_xml_integration_point,
            _ => false,
        }
    }
}
