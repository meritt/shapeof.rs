use std::collections::HashMap;

use crate::format::Format;

pub const OBJECT: u8 = 1;
pub const ARRAY: u8 = 2;
pub const STRING: u8 = 4;
pub const INTEGER: u8 = 8;
pub const NUMBER: u8 = 16;
pub const BOOLEAN: u8 = 32;
pub const NULL: u8 = 64;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Shared {
    #[default]
    Unset,
    None,
    One(Format),
}

#[derive(Debug, Default)]
pub struct Node {
    pub types: u8,
    pub objects: u32,
    pub props: Vec<Prop>,
    pub items: Option<Box<Node>>,
    pub format: Shared,
    index: HashMap<Box<str>, usize>,
    next: usize,
}

#[derive(Debug)]
pub struct Prop {
    pub name: Box<str>,
    pub count: u32,
    pub node: Node,
    stamp: u32,
}

impl Node {
    pub fn wants_format(&self) -> bool {
        self.format != Shared::None
    }

    pub fn add_format(&mut self, format: Option<Format>) {
        let seen = format.map_or(Shared::None, Shared::One);

        self.format = match self.format {
            Shared::Unset => seen,
            current if current == seen => current,
            _ => Shared::None,
        };
    }

    pub fn prop(&mut self, name: &str, stamp: u32) -> &mut Prop {
        let guess = self.next;
        let position = if self.props.get(guess).is_some_and(|prop| &*prop.name == name) {
            guess
        } else if let Some(&position) = self.index.get(name) {
            position
        } else {
            let position = self.props.len();

            self.props.push(Prop { name: name.into(), count: 0, node: Node::default(), stamp: 0 });
            self.index.insert(name.into(), position);
            position
        };

        self.next = position + 1;

        let prop = &mut self.props[position];

        if prop.stamp != stamp {
            prop.stamp = stamp;
            prop.count += 1;
        }

        prop
    }
}
