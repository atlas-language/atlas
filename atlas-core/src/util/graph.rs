use crate::{Error, ErrorKind};
use std::cell::Cell;
use std::rc::Rc;
use slab::Slab;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug)]
#[derive(PartialEq,Eq)]
// Allows for recursive node references by updating the reference after construction
pub struct NodeRef(Rc<Cell<usize>>);

impl Clone for NodeRef {
    fn clone(&self) -> Self {
        NodeRef(self.0.clone())
    }
}

impl std::hash::Hash for NodeRef {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.0.get().hash(h)
    }
}

impl NodeRef {
    pub fn temp() -> NodeRef {
        NodeRef(Rc::new(Cell::new(0)))
    }

    pub fn set_to(&mut self, r: &NodeRef) {
        if self.0.get() != 0 { panic!("Can only set a temporary ref!") }
        self.0.set(r.0.get())
    }
}

pub trait Node {
    fn out_edges(&self) -> Vec<NodeRef>;
}

pub struct Graph<N : Node> {
    nodes: Slab<N>,
    root: Option<NodeRef>
}

impl<N : Node> Default for Graph<N> {
    fn default() -> Self {
        Self { nodes: Slab::new(), root: None }
    }
}

impl<N: Node> Graph<N> {
    pub fn new() -> Self { Self::default() }
    pub fn insert(&mut self, node: N) -> NodeRef {
        let key = self.nodes.insert(node);
        NodeRef(Rc::new(Cell::new(key + 1)))
    }

    pub fn insert_at(&mut self, r: &NodeRef, node: N) {
        let key = self.nodes.insert(node);
        r.0.set(key + 1)
    }

    pub fn set_root(&mut self, r: NodeRef) {
        self.root = Some(r)
    }

    pub fn get_root(&self) -> Option<&NodeRef> {
        self.root.as_ref()
    }

    pub fn get<'g>(&'g self, r: &NodeRef) -> Option<&N> {
        if r.0.get() == 0 { return None }
        self.nodes.get(r.0.get() - 1)
    }

    pub fn flatten(&self) -> Result<Flattened, Error> {
        let mut in_edges =  HashMap::new();
        let mut order  = Vec::new();

        let mut seen = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(self.root.clone().ok_or(Error::new_const(ErrorKind::Compile,
                    "No graph root set"))?);
        while let Some(nr) = queue.pop_back() {
            if seen.insert(nr.clone()) {
                let n = self.get(&nr)
                    .ok_or(Error::new_const(ErrorKind::Compile,
                        "Invalid graph node"))?;
                order.push(nr.clone());
                for c in n.out_edges() {
                    in_edges.entry(c.clone()).or_insert(Vec::new()).push(nr.clone());
                    queue.push_back(c);
                }
            }
        }
        Ok(Flattened { in_edges, order })
    }
}

pub struct Flattened {
    pub in_edges: HashMap<NodeRef, Vec<NodeRef>>,
    pub order: Vec<NodeRef>
}

use std::fmt;
impl<N : fmt::Debug + Node> fmt::Debug for Graph<N> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        writeln!(fmt, "Graph {{")?;
        for (k, node) in self.nodes.iter() {
            writeln!(fmt, "    {}: {:?}", k + 1, node)?;
        }
        write!(fmt, "}}")
    }
}
impl<N : fmt::Display + Node> fmt::Display for Graph<N> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Graph {{")?;
        for (i, (k, node)) in self.nodes.iter().enumerate() {
            if i > 0 { write!(fmt, ", ")? }
            write!(fmt, "{}: {}", k, node)?;
        }
        write!(fmt, "}}")
    }
}