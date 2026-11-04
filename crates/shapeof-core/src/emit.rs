use crate::format::Format;
use crate::json::Value;
use crate::tree::{ARRAY, BOOLEAN, INTEGER, NULL, NUMBER, Node, OBJECT, STRING};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    OpenAi,
    Anthropic,
    Plain,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Notes {
    pub empty: Vec<Vec<String>>,
    pub unions: usize,
    pub optional: usize,
    pub depth: usize,
    pub properties: usize,
    pub names: usize,
    pub root_object: bool,
}

const ORDER: [(u8, &str); 7] = [
    (OBJECT, "object"),
    (ARRAY, "array"),
    (STRING, "string"),
    (INTEGER, "integer"),
    (NUMBER, "number"),
    (BOOLEAN, "boolean"),
    (NULL, "null"),
];

struct Context<'a> {
    target: Target,
    notes: Notes,
    path: Vec<&'a str>,
}

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

fn allowed(target: Target, format: Format) -> bool {
    target != Target::OpenAi || format != Format::Uri
}

impl<'a> Context<'a> {
    fn emit(&mut self, node: &'a Node, nullable: bool) -> Value {
        let mut types = node.types;

        if types & NUMBER != 0 {
            types &= !INTEGER;
        }

        if nullable {
            types |= NULL;
        }

        if types == 0 {
            self.notes.empty.push(self.path.iter().map(|part| (*part).to_owned()).collect());
            return if self.target == Target::OpenAi {
                Value::Map(vec![("type".into(), text("string"))])
            } else {
                Value::Map(vec![])
            };
        }

        let mut schemas = Vec::new();

        if types & OBJECT != 0 {
            schemas.push(self.object(node));
        }

        if types & ARRAY != 0 {
            schemas.push(self.array(node));
        }

        let simple: Vec<&str> = ORDER
            .iter()
            .filter(|(bit, _)| bit & types != 0 && bit & (OBJECT | ARRAY) == 0)
            .map(|(_, name)| *name)
            .collect();

        if !simple.is_empty() {
            let kind = if let [one] = simple[..] {
                text(one)
            } else {
                Value::List(simple.iter().map(|name| text(name)).collect())
            };
            let mut schema = vec![("type".to_owned(), kind)];

            if types & STRING != 0
                && let crate::tree::Shared::One(format) = node.format
                && allowed(self.target, format)
            {
                schema.push(("format".into(), text(format.name())));
            }

            schemas.push(Value::Map(schema));
        }

        if simple.len() > 1 || schemas.len() > 1 {
            self.notes.unions += 1;
        }

        if schemas.len() == 1 { schemas.remove(0) } else { Value::Map(vec![("anyOf".into(), Value::List(schemas))]) }
    }

    fn object(&mut self, node: &'a Node) -> Value {
        let all = self.target == Target::OpenAi;
        let mut properties = Vec::with_capacity(node.props.len());
        let mut required = Vec::new();

        self.notes.depth = self.notes.depth.max(self.path.len() + 1);

        for prop in &node.props {
            let always = prop.count == node.objects;

            self.notes.properties += 1;
            self.notes.names += prop.name.encode_utf16().count();
            self.path.push(&prop.name);

            let schema = self.emit(&prop.node, all && !always);

            self.path.pop();
            properties.push((prop.name.to_string(), schema));

            if always || all {
                required.push(text(&prop.name));
            } else {
                self.notes.optional += 1;
            }
        }

        let mut schema = vec![
            ("type".to_owned(), text("object")),
            ("properties".to_owned(), Value::Map(properties)),
            ("required".to_owned(), Value::List(required)),
        ];

        if self.target != Target::Plain {
            schema.push(("additionalProperties".into(), Value::Bool(false)));
        }

        Value::Map(schema)
    }

    fn array(&mut self, node: &'a Node) -> Value {
        self.path.push("[]");

        let schema = node.items.as_deref().map_or_else(|| Value::Map(vec![]), |items| self.emit(items, false));

        self.path.pop();
        Value::Map(vec![("type".into(), text("array")), ("items".into(), schema)])
    }
}

pub fn emit(root: &Node, target: Target) -> (Value, Notes) {
    let mut context = Context { target, notes: Notes::default(), path: Vec::new() };
    let body = context.emit(root, false);
    let mut notes = context.notes;

    notes.root_object = root.types == OBJECT;

    let schema = match (target, body) {
        (Target::Plain, Value::Map(entries)) => {
            let mut all = vec![("$schema".to_owned(), text("https://json-schema.org/draft/2020-12/schema"))];

            all.extend(entries);
            Value::Map(all)
        }
        (_, body) => body,
    };

    (schema, notes)
}
