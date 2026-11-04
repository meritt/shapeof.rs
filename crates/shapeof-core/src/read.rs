use std::borrow::Cow;

use crate::format;
use crate::tree::{ARRAY, BOOLEAN, INTEGER, NULL, NUMBER, Node, OBJECT, STRING};

const DEPTH: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub offset: usize,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub reason: &'static str,
    pub line: usize,
    pub column: usize,
    pub source: String,
}

#[derive(Debug, Default)]
pub struct Examples {
    pub root: Node,
    pub documents: usize,
    pub depth: usize,
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    index: usize,
    serial: u32,
    depth: usize,
}

fn char_len(lead: u8) -> usize {
    match lead {
        0x00..0xC0 => 1,
        0xC0..0xE0 => 2,
        0xE0..0xF0 => 3,
        _ => 4,
    }
}

fn is_identifier_start(char: char) -> bool {
    char == '$' || char == '_' || unicode_ident::is_xid_start(char)
}

fn is_identifier_part(char: char) -> bool {
    matches!(char, '$' | '\u{200C}' | '\u{200D}') || unicode_ident::is_xid_continue(char)
}

fn is_js_space(char: char) -> bool {
    matches!(
        char,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

fn hex_value(text: &str, radix: u32) -> f64 {
    if text.is_empty() {
        return f64::NAN;
    }

    let mut exact: Option<u128> = Some(0);
    let mut approximate = 0.0_f64;

    for char in text.chars() {
        let Some(digit) = char.to_digit(radix) else {
            return f64::NAN;
        };

        exact = exact.and_then(|value| value.checked_mul(u128::from(radix))?.checked_add(u128::from(digit)));
        approximate = approximate * f64::from(radix) + f64::from(digit);
    }

    #[expect(clippy::cast_precision_loss, reason = "Number() rounds the same way")]
    exact.map_or(approximate, |value| value as f64)
}

fn numeric(raw: &str) -> f64 {
    let negative = raw.starts_with('-');
    let body = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    let cleaned: String = body.chars().filter(|char| *char != '_').collect();
    let digits = cleaned.strip_suffix('n').unwrap_or(&cleaned);
    let prefix = digits.get(..2).map(str::to_ascii_lowercase);
    let value = match prefix.as_deref() {
        Some("0x") => hex_value(&digits[2..], 16),
        Some("0o") => hex_value(&digits[2..], 8),
        Some("0b") => hex_value(&digits[2..], 2),
        _ if digits == "Infinity" => f64::INFINITY,
        _ if digits == "NaN" => f64::NAN,
        _ => digits.parse().unwrap_or(f64::NAN),
    };

    if negative { -value } else { value }
}

fn unescape(raw: &str) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(raw.len());
    let mut rest = raw;

    let push = |units: &mut Vec<u16>, text: &str| units.extend(text.encode_utf16());
    let push_code = |units: &mut Vec<u16>, code: u32| {
        if let Some(char) = char::from_u32(code) {
            let mut buffer = [0; 2];

            units.extend_from_slice(char.encode_utf16(&mut buffer));
        } else if code <= 0xFFFF {
            units.push(code as u16);
        }
    };
    let hex = |text: &str| u32::from_str_radix(text, 16).ok();

    while let Some(position) = rest.find('\\') {
        push(&mut units, &rest[..position]);
        rest = &rest[position + 1..];

        let Some(char) = rest.chars().next() else {
            units.push(u16::from(b'\\'));
            break;
        };
        let after = &rest[char.len_utf8()..];

        match char {
            'u' | 'U' if after.starts_with('{') => {
                let length = after[1..].bytes().take_while(u8::is_ascii_hexdigit).count();

                if length > 0 && after.as_bytes().get(length + 1) == Some(&b'}') {
                    let code = after[1..=length]
                        .chars()
                        .try_fold(0_u32, |code, digit| code.checked_mul(16)?.checked_add(digit.to_digit(16)?));

                    if let Some(code) = code.filter(|code| *code <= 0x10_FFFF) {
                        push_code(&mut units, code);
                    }

                    rest = &after[length + 2..];
                    continue;
                }

                units.push(char as u16);
                rest = after;
            }
            'u' | 'U' if after.len() >= 4 && after.as_bytes()[..4].iter().all(u8::is_ascii_hexdigit) => {
                push_code(&mut units, hex(&after[..4]).unwrap_or(0));
                rest = &after[4..];
            }
            'x' | 'X' if after.len() >= 2 && after.as_bytes()[..2].iter().all(u8::is_ascii_hexdigit) => {
                push_code(&mut units, hex(&after[..2]).unwrap_or(0));
                rest = &after[2..];
            }
            '\r' => rest = after.strip_prefix('\n').unwrap_or(after),
            '\n' | '\u{2028}' | '\u{2029}' => rest = after,
            _ => {
                let escaped = match char {
                    'b' => '\u{08}',
                    'f' => '\u{0C}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'v' => '\u{0B}',
                    '0' => '\0',
                    other => other,
                };

                push(&mut units, escaped.encode_utf8(&mut [0; 4]));
                rest = after;
            }
        }
    }

    push(&mut units, rest);
    String::from_utf16_lossy(&units)
}

fn js_string(value: f64) -> String {
    ryu_js::Buffer::new().format(value).to_owned()
}

impl<'a> Reader<'a> {
    fn new(text: &'a str, serial: u32) -> Self {
        Self { text, bytes: text.as_bytes(), index: 0, serial, depth: 0 }
    }

    fn byte(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn fail<T>(&self, reason: &'static str) -> Result<T, Failure> {
        Err(Failure { offset: self.index.min(self.bytes.len()), reason })
    }

    fn ended(&self, reason: &'static str) -> &'static str {
        if self.index >= self.bytes.len() {
            "пример обрывается раньше времени"
        } else {
            reason
        }
    }

    fn space(&self) -> usize {
        let rest = &self.bytes[self.index.min(self.bytes.len())..];

        match rest {
            [b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C, ..] => 1,
            [0xC2, 0xA0, ..] => 2,
            [0xEF, 0xBB, 0xBF, ..] | [0xE2, 0x80, 0xA8 | 0xA9, ..] => 3,
            _ => 0,
        }
    }

    fn skip(&mut self) -> Result<(), Failure> {
        loop {
            loop {
                let length = self.space();

                if length == 0 {
                    break;
                }

                self.index += length;
            }

            if self.byte() != Some(b'/') {
                return Ok(());
            }

            match self.bytes.get(self.index + 1) {
                Some(b'/') => {
                    self.index = self.text[self.index..].find('\n').map_or(self.bytes.len(), |end| self.index + end);
                }
                Some(b'*') => match self.text[self.index + 2..].find("*/") {
                    Some(end) => self.index += end + 4,
                    None => return self.fail("комментарий не закрыт"),
                },
                _ => return Ok(()),
            }
        }
    }

    fn string(&mut self) -> Result<Cow<'a, str>, Failure> {
        let quote = self.bytes[self.index];

        self.index += 1;

        let start = self.index;
        let mut escaped = false;

        while let Some(byte) = self.byte() {
            if byte == quote {
                let raw = &self.text[start..self.index];

                self.index += 1;
                return Ok(if escaped { Cow::Owned(unescape(raw)) } else { Cow::Borrowed(raw) });
            }

            if byte == b'\n' || byte == b'\r' {
                break;
            }

            if byte == b'\\' {
                escaped = true;
                self.index += if self.bytes[self.index + 1..].starts_with(b"\r\n") {
                    3
                } else {
                    1 + self.bytes.get(self.index + 1).map_or(1, |lead| char_len(*lead))
                };
            } else {
                self.index += 1;
            }
        }

        self.fail("строка не закрыта кавычкой")
    }

    fn digits(&self, mut at: usize, accept: impl Fn(u8) -> bool) -> usize {
        while self.bytes.get(at).is_some_and(|byte| accept(*byte)) {
            at += 1;
        }

        at
    }

    fn number(&self) -> Option<usize> {
        let bytes = self.bytes;
        let mut at = self.index;

        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }

        let rest = &bytes[at.min(bytes.len())..];
        let prefixed = |letters: &[u8], accept: fn(u8) -> bool| {
            rest.len() > 2 && rest[0] == b'0' && letters.contains(&rest[1]) && (accept(rest[2]) || rest[2] == b'_')
        };
        let decimal = |byte: u8| byte.is_ascii_digit() || byte == b'_';

        if rest.starts_with(b"Infinity") {
            at += 8;
        } else if rest.starts_with(b"NaN") {
            at += 3;
        } else if prefixed(b"xX", |byte| byte.is_ascii_hexdigit()) {
            at = self.digits(at + 2, |byte| byte.is_ascii_hexdigit() || byte == b'_');
        } else if prefixed(b"oO", |byte| (b'0'..=b'7').contains(&byte)) {
            at = self.digits(at + 2, |byte| (b'0'..=b'7').contains(&byte) || byte == b'_');
        } else if prefixed(b"bB", |byte| byte == b'0' || byte == b'1') {
            at = self.digits(at + 2, |byte| byte == b'0' || byte == b'1' || byte == b'_');
        } else {
            if rest.first().is_some_and(u8::is_ascii_digit) {
                at = self.digits(at + 1, decimal);

                if bytes.get(at) == Some(&b'.') {
                    at = self.digits(at + 1, decimal);
                }
            } else if rest.first() == Some(&b'.') && rest.get(1).is_some_and(u8::is_ascii_digit) {
                at = self.digits(at + 2, decimal);
            } else {
                return None;
            }

            if matches!(bytes.get(at), Some(b'e' | b'E')) {
                let sign = usize::from(matches!(bytes.get(at + 1), Some(b'+' | b'-')));

                if bytes.get(at + 1 + sign).is_some_and(u8::is_ascii_digit) {
                    at = self.digits(at + 1 + sign, |byte| byte.is_ascii_digit());
                }
            }
        }

        if bytes.get(at) == Some(&b'n') {
            at += 1;
        }

        Some(at)
    }

    fn identifier(&self) -> Option<usize> {
        let rest = &self.text[self.index..];
        let mut chars = rest.char_indices();
        let (_, first) = chars.next()?;

        if !is_identifier_start(first) {
            return None;
        }

        let end = chars.find(|(_, char)| !is_identifier_part(*char)).map_or(rest.len(), |(at, _)| at);

        Some(self.index + end)
    }

    fn key(&mut self) -> Result<Cow<'a, str>, Failure> {
        if matches!(self.byte(), Some(b'"' | b'\'')) {
            return self.string();
        }

        if let Some(end) = self.identifier() {
            let name = &self.text[self.index..end];

            self.index = end;
            return Ok(Cow::Borrowed(name));
        }

        if let Some(end) = self.number() {
            let name = js_string(numeric(&self.text[self.index..end]));

            self.index = end;
            return Ok(Cow::Owned(name));
        }

        self.fail(self.ended("ожидалось имя свойства"))
    }

    fn separator(&mut self, close: u8, reason: &'static str) -> Result<(), Failure> {
        self.skip()?;

        if self.byte() == Some(b',') {
            self.index += 1;
            self.skip()
        } else if self.byte() == Some(close) {
            Ok(())
        } else {
            self.fail(self.ended(reason))
        }
    }

    fn value(&mut self, node: &mut Node, depth: usize) -> Result<(), Failure> {
        if depth > DEPTH {
            return self.fail("слишком глубокая вложенность");
        }

        self.depth = self.depth.max(depth);

        match self.byte() {
            Some(b'{') => {
                self.serial += 1;

                let stamp = self.serial;

                node.types |= OBJECT;
                node.objects += 1;
                self.index += 1;
                self.skip()?;

                while self.byte() != Some(b'}') {
                    let name = self.key()?;

                    self.skip()?;

                    if self.byte() != Some(b':') {
                        return self.fail(self.ended("ожидалось двоеточие после имени свойства"));
                    }

                    self.index += 1;
                    self.skip()?;
                    self.value(&mut node.prop(&name, stamp).node, depth + 1)?;
                    self.separator(b'}', "ожидалась запятая или «}»")?;
                }

                self.index += 1;
            }
            Some(b'[') => {
                node.types |= ARRAY;

                let items = node.items.get_or_insert_default();

                self.index += 1;
                self.skip()?;

                while self.byte() != Some(b']') {
                    self.value(items, depth + 1)?;
                    self.separator(b']', "ожидалась запятая или «]»")?;
                }

                self.index += 1;
            }
            Some(b'"' | b'\'') => {
                let text = self.string()?;

                node.types |= STRING;

                if node.wants_format() {
                    node.add_format(format::of(&text));
                }
            }
            _ => {
                if let Some(end) = self.number() {
                    let value = numeric(&self.text[self.index..end]);
                    let integer = value.is_finite() && value.fract() == 0.0;

                    node.types |= if integer { INTEGER } else { NUMBER };
                    self.index = end;
                    return Ok(());
                }

                let kind = self.identifier().and_then(|end| {
                    let kind = match &self.text[self.index..end] {
                        "true" | "false" => BOOLEAN,
                        "null" | "undefined" => NULL,
                        _ => return None,
                    };

                    Some((kind, end))
                });

                let Some((kind, end)) = kind else {
                    return self
                        .fail(self.ended("ожидалось значение: строка, число, объект, массив, true, false или null"));
                };

                node.types |= kind;
                self.index = end;
            }
        }

        Ok(())
    }

    fn document(&mut self, node: &mut Node) -> Result<(), Failure> {
        self.skip()?;
        self.value(node, 0)?;
        self.skip()?;

        if self.byte() == Some(b';') {
            self.index += 1;
            self.skip()?;
        }

        if self.index < self.bytes.len() {
            return self.fail("после данных остался лишний текст");
        }

        Ok(())
    }
}

fn position(text: &str, offset: usize, reason: &'static str) -> Error {
    let before = &text[..offset];
    let start = before.rfind('\n').map_or(0, |at| at + 1);
    let end = text[start..].find('\n').map_or(text.len(), |at| start + at);

    Error {
        reason,
        line: before.bytes().filter(|byte| *byte == b'\n').count() + 1,
        column: text[start..offset].chars().count() + 1,
        source: text[start..end].to_owned(),
    }
}

fn lines(text: &str) -> Result<Option<Examples>, Error> {
    let mut examples = Examples::default();
    let mut serial = 0;
    let mut offset = 0;

    for line in text.split('\n') {
        let start = offset;

        offset += line.len() + 1;

        if js_trim(line).is_empty() {
            continue;
        }

        let mut reader = Reader::new(line, serial);

        if let Err(failure) = reader.document(&mut examples.root) {
            return if examples.documents > 0 {
                Err(position(text, start + failure.offset, failure.reason))
            } else {
                Ok(None)
            };
        }

        serial = reader.serial;
        examples.documents += 1;
        examples.depth = examples.depth.max(reader.depth);
    }

    Ok(Some(examples))
}

pub fn read(text: &str) -> Result<Examples, Error> {
    let mut root = Node::default();
    let mut reader = Reader::new(text, 0);

    let failure = match reader.document(&mut root) {
        Ok(()) => return Ok(Examples { root, documents: 1, depth: reader.depth }),
        Err(failure) => failure,
    };

    if js_trim(text).contains('\n')
        && let Some(examples) = lines(text)?
    {
        return Ok(examples);
    }

    Err(position(text, failure.offset, failure.reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(text: &str) -> (usize, usize, &'static str) {
        let error = read(text).unwrap_err();

        (error.line, error.column, error.reason)
    }

    #[test]
    fn numbers_as_javascript() {
        assert_eq!(numeric("0x1F"), 31.0);
        assert_eq!(numeric("-0o17"), -15.0);
        assert_eq!(numeric("0b101"), 5.0);
        assert_eq!(numeric("1_000n"), 1000.0);
        assert_eq!(numeric("1."), 1.0);
        assert_eq!(numeric(".5e1"), 5.0);
        assert!(numeric("NaN").is_nan());
        assert_eq!(numeric("-Infinity"), f64::NEG_INFINITY);
        assert_eq!(js_string(numeric("1e21")), "1e+21");
        assert_eq!(js_string(numeric("1.50")), "1.5");
        assert_eq!(js_string(numeric("-0")), "0");
        assert_eq!(js_string(numeric("0x10")), "16");
        assert_eq!(js_string(numeric("1e-7")), "1e-7");
    }

    #[test]
    fn escapes_as_javascript() {
        assert_eq!(unescape(r"a\nb\tc"), "a\nb\tc");
        assert_eq!(unescape(r"A\x42\u{1F600}"), "AB😀");
        assert_eq!(unescape(r"😀"), "😀");
        assert_eq!(unescape(r"\q\'\B"), "q'B");
        assert_eq!(unescape("a\\\r\nb"), "ab");
        assert_eq!(unescape(r"\u{110000}!"), "!");
        assert_eq!(unescape(r"\uZZ"), "uZZ");
    }

    #[test]
    fn loose_text() {
        let examples =
            read("// comment\n{ a: 1, 'b': [1, 2,], /* c */ 0x10: NaN, $d: undefined, e: +.5, ok: true, };").unwrap();
        let names: Vec<&str> = examples.root.props.iter().map(|prop| &*prop.name).collect();

        assert_eq!(names, ["a", "b", "16", "$d", "e", "ok"]);
        assert_eq!(examples.documents, 1);
    }

    #[test]
    fn json_lines() {
        let examples = read("{\"a\": 1}\n\n{\"a\": 2, \"b\": 3}\r\n").unwrap();

        assert_eq!(examples.documents, 2);
        assert_eq!(examples.root.objects, 2);
        assert_eq!(
            error("{\"a\": 1}\n{\"a\": }"),
            (2, 7, "ожидалось значение: строка, число, объект, массив, true, false или null")
        );
        assert_eq!(error("{\"a\": 1\n{\"b\": 2}"), (2, 1, "ожидалась запятая или «}»"));
    }

    #[test]
    fn errors_point_at_the_place() {
        assert_eq!(error("{\"a\" 1}"), (1, 6, "ожидалось двоеточие после имени свойства"));
        assert_eq!(error("{\"a\": \"б\nв\"}"), (1, 9, "строка не закрыта кавычкой"));
        assert_eq!(error("[1, 2"), (1, 6, "пример обрывается раньше времени"));
        assert_eq!(error("{} x"), (1, 4, "после данных остался лишний текст"));
        assert_eq!(error("/* x"), (1, 1, "комментарий не закрыт"));
        assert_eq!(error("{,}"), (1, 2, "ожидалось имя свойства"));
        assert_eq!(error("[1 2]"), (1, 4, "ожидалась запятая или «]»"));
        assert_eq!(
            error("[\"ж\", nope]"),
            (1, 7, "ожидалось значение: строка, число, объект, массив, true, false или null")
        );
        assert_eq!(error(&format!("{}{}", "[".repeat(1002), "]".repeat(1002))).2, "слишком глубокая вложенность");
    }
}
