mod emit;
mod format;
mod json;
mod read;
mod tree;

use std::fmt::Write;

pub use emit::{Notes, Target};
pub use read::Error;

pub const DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Schema { text: String, notes: Notes },
    Invalid(Error),
    Deep,
}

#[must_use]
pub fn infer(text: &str, target: Target) -> Outcome {
    let examples = match read::read(text) {
        Ok(examples) => examples,
        Err(error) => return Outcome::Invalid(error),
    };

    let root = &examples.root;
    let items = root.items.as_deref().filter(|items| items.types == tree::OBJECT);
    let list = examples.documents == 1 && target != Target::Plain && root.types == tree::ARRAY;
    let (root, depth) = match items {
        Some(items) if list => (items, examples.depth - 1),
        _ => (root, examples.depth),
    };

    if depth > DEPTH {
        return Outcome::Deep;
    }

    let (schema, notes) = emit::emit(root, target);

    Outcome::Schema { text: json::pretty(&schema), notes }
}

#[must_use]
pub fn report(text: &str, target: Target) -> String {
    let mut out = String::new();

    match infer(text, target) {
        Outcome::Schema { text, notes } => {
            out.push_str("{\"schema\":");
            json::quote(&text, &mut out);
            out.push_str(",\"notes\":{\"empty\":[");

            for (index, path) in notes.empty.iter().enumerate() {
                out.push_str(if index > 0 { ",[" } else { "[" });

                for (at, part) in path.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }

                    json::quote(part, &mut out);
                }

                out.push(']');
            }

            let _ = write!(
                out,
                "],\"unions\":{},\"optional\":{},\"depth\":{},\"properties\":{},\"names\":{},\"rootObject\":{}}}}}",
                notes.unions, notes.optional, notes.depth, notes.properties, notes.names, notes.root_object
            );
        }
        Outcome::Invalid(error) => {
            out.push_str("{\"error\":{\"reason\":");
            json::quote(error.reason, &mut out);
            let _ = write!(out, ",\"line\":{},\"column\":{},\"source\":", error.line, error.column);
            json::quote(&error.source, &mut out);
            out.push_str("}}");
        }
        Outcome::Deep => out.push_str("{\"error\":\"depth\"}"),
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(text: &str, target: Target) -> String {
        match infer(text, target) {
            Outcome::Schema { text, .. } => text,
            other => panic!("{other:?}"),
        }
    }

    fn notes(text: &str, target: Target) -> Notes {
        match infer(text, target) {
            Outcome::Schema { notes, .. } => notes,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn openai_makes_every_property_required() {
        let text = r#"[{"id": 1, "name": "a", "tags": []}, {"id": 2.5, "tags": ["x"], "seen": "2026-10-27"}]"#;

        assert_eq!(
            schema(text, Target::OpenAi),
            r#"{
  "type": "object",
  "properties": {
    "id": {
      "type": "number"
    },
    "name": {
      "type": ["string", "null"]
    },
    "tags": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "seen": {
      "type": ["string", "null"],
      "format": "date"
    }
  },
  "required": ["id", "name", "tags", "seen"],
  "additionalProperties": false
}"#
        );
    }

    #[test]
    fn anthropic_keeps_optional_properties() {
        let text = "{\"a\": 1, \"b\": {\"c\": \"https://simonenko.xyz/\"}}\n{\"a\": null}";
        let notes = notes(text, Target::Anthropic);

        assert_eq!(notes.optional, 1);
        assert_eq!(notes.unions, 1);
        assert!(notes.root_object);
        assert!(schema(text, Target::Anthropic).contains("\"format\": \"uri\""));
        assert!(!schema(text, Target::OpenAi).contains("\"uri\""));
    }

    #[test]
    fn plain_keeps_a_list_root() {
        let text = r#"[{"a": 1}, {"a": 2}]"#;
        let plain = schema(text, Target::Plain);

        assert!(
            plain.starts_with(
                "{\n  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n  \"type\": \"array\""
            )
        );
        assert!(!plain.contains("additionalProperties"));
        assert!(!notes(text, Target::Plain).root_object);
        assert!(notes(text, Target::OpenAi).root_object);
    }

    #[test]
    fn empty_arrays_are_reported() {
        let found = notes(r#"{"a": {"b": []}, "c": [[]]}"#, Target::Anthropic);

        assert_eq!(found.empty, [vec!["a", "b", "[]"], vec!["c", "[]", "[]"]]);
        assert_eq!(found.depth, 2);
        assert_eq!(found.properties, 3);
    }

    #[test]
    fn mixed_types_become_any_of() {
        let text = r#"[{"v": 1}, {"v": "x"}, {"v": {"w": true}}, {"v": [1]}]"#;
        let found = schema(text, Target::Anthropic);

        assert!(found.contains("\"anyOf\": ["));
        assert!(found.contains("\"type\": [\"string\", \"integer\"]"));
    }

    #[test]
    fn depth_is_limited() {
        let deep = format!("{}{}", "[".repeat(65), "]".repeat(65));
        let deeper = format!("{}{}", "[".repeat(66), "]".repeat(66));
        let list = format!("[{{\"a\": {}{}}}]", "[".repeat(64), "]".repeat(64));

        assert!(matches!(infer(&deep, Target::Plain), Outcome::Schema { .. }));
        assert_eq!(infer(&deeper, Target::Plain), Outcome::Deep);
        assert!(matches!(infer(&list, Target::OpenAi), Outcome::Schema { .. }));
        assert_eq!(infer(&list, Target::Plain), Outcome::Deep);
    }

    #[test]
    fn reports_as_json() {
        assert_eq!(
            report("{\"a\" 1}", Target::OpenAi),
            "{\"error\":{\"reason\":\"ожидалось двоеточие после имени свойства\",\"line\":1,\"column\":6,\"source\":\"{\\\"a\\\" 1}\"}}"
        );
        assert!(report("{\"a\": []}", Target::OpenAi).ends_with(
            "\"notes\":{\"empty\":[[\"a\",\"[]\"]],\"unions\":0,\"optional\":0,\"depth\":1,\"properties\":1,\"names\":1,\"rootObject\":true}}"
        ));
    }
}
