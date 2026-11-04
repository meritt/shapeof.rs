use std::fmt::Write;

const LINE: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Text(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

pub fn quote(text: &str, out: &mut String) {
    out.push('"');

    for char in text.chars() {
        match char {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0'..='\u{1F}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(char));
            }
            _ => out.push(char),
        }
    }

    out.push('"');
}

fn inline(items: &[Value]) -> Option<String> {
    let mut line = String::from("[");

    for (index, item) in items.iter().enumerate() {
        let Value::Text(text) = item else {
            return None;
        };

        if index > 0 {
            line.push_str(", ");
        }

        let start = line.len();

        quote(text, &mut line);

        if line[start + 1..line.len() - 1].contains('"') {
            return None;
        }
    }

    line.push(']');
    (line.encode_utf16().count() <= LINE).then_some(line)
}

fn write(value: &Value, indent: usize, out: &mut String) {
    let pad = |out: &mut String, depth: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', depth));
    };

    match value {
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Text(text) => quote(text, out),
        Value::List(items) if items.is_empty() => out.push_str("[]"),
        Value::Map(entries) if entries.is_empty() => out.push_str("{}"),
        Value::List(items) => {
            if let Some(line) = inline(items) {
                out.push_str(&line);
                return;
            }

            out.push('[');

            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }

                pad(out, indent + 2);
                write(item, indent + 2, out);
            }

            pad(out, indent);
            out.push(']');
        }
        Value::Map(entries) => {
            out.push('{');

            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }

                pad(out, indent + 2);
                quote(key, out);
                out.push_str(": ");
                write(item, indent + 2, out);
            }

            pad(out, indent);
            out.push('}');
        }
    }
}

pub fn pretty(value: &Value) -> String {
    let mut out = String::new();

    write(value, 0, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    #[test]
    fn writes_like_json_stringify() {
        let value = Value::Map(vec![
            ("type".into(), Value::List(vec![text("string"), text("null")])),
            ("required".into(), Value::List(vec![])),
            ("properties".into(), Value::Map(vec![])),
            ("say \"hi\"\n".into(), Value::List(vec![text("a\"b"), text("c")])),
        ]);

        assert_eq!(
            pretty(&value),
            "{\n  \"type\": [\"string\", \"null\"],\n  \"required\": [],\n  \"properties\": {},\n  \"say \\\"hi\\\"\\n\": [\n    \"a\\\"b\",\n    \"c\"\n  ]\n}"
        );
    }

    #[test]
    fn long_lists_stay_in_a_column() {
        let names: Vec<Value> = (0..10).map(|index| text(&format!("property{index}"))).collect();

        assert!(pretty(&Value::List(names)).starts_with("[\n  \"property0\","));
    }
}
