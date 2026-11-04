use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    DateTime,
    Date,
    Time,
    Duration,
    Email,
    Uuid,
    Ipv4,
    Ipv6,
    Uri,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::DateTime => "date-time",
            Format::Date => "date",
            Format::Time => "time",
            Format::Duration => "duration",
            Format::Email => "email",
            Format::Uuid => "uuid",
            Format::Ipv4 => "ipv4",
            Format::Ipv6 => "ipv6",
            Format::Uri => "uri",
        }
    }
}

pub fn of(value: &str) -> Option<Format> {
    let bytes = value.as_bytes();

    if bytes.len() > 254 || bytes.is_empty() {
        return None;
    }

    if bytes[0].is_ascii_digit() {
        if date_time(bytes) {
            return Some(Format::DateTime);
        }

        if bytes.len() == 10 && date(bytes) {
            return Some(Format::Date);
        }

        if time(bytes) {
            return Some(Format::Time);
        }
    }

    if email(value) {
        return Some(Format::Email);
    }

    if uuid(bytes) {
        return Some(Format::Uuid);
    }

    if value.parse::<Ipv4Addr>().is_ok() {
        return Some(Format::Ipv4);
    }

    if value.contains(':') && value.parse::<Ipv6Addr>().is_ok() {
        return Some(Format::Ipv6);
    }

    if duration(bytes) {
        return Some(Format::Duration);
    }

    uri(value).then_some(Format::Uri)
}

fn digits(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }

    Some(bytes.iter().fold(0, |sum, digit| sum * 10 + u32::from(digit - b'0')))
}

fn date(bytes: &[u8]) -> bool {
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }

    let (Some(year), Some(month), Some(day)) = (digits(&bytes[..4]), digits(&bytes[5..7]), digits(&bytes[8..10]))
    else {
        return false;
    };

    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    (1..=12).contains(&month) && day >= 1 && day <= days[month as usize - 1]
}

fn time(bytes: &[u8]) -> bool {
    if bytes.len() < 9 || bytes[2] != b':' || bytes[5] != b':' {
        return false;
    }

    let (Some(hour), Some(minute), Some(second)) = (digits(&bytes[..2]), digits(&bytes[3..5]), digits(&bytes[6..8]))
    else {
        return false;
    };

    if hour > 23 || minute > 59 || second > 59 {
        return false;
    }

    let mut rest = &bytes[8..];

    if let Some(fraction) = rest.strip_prefix(b".") {
        let length = fraction.iter().take_while(|byte| byte.is_ascii_digit()).count();

        if length == 0 {
            return false;
        }

        rest = &fraction[length..];
    }

    match rest {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => {
            let (Some(hour), Some(minute)) = (digits(&[*h1, *h2]), digits(&[*m1, *m2])) else {
                return false;
            };

            hour <= 23 && minute <= 59
        }
        _ => false,
    }
}

fn date_time(bytes: &[u8]) -> bool {
    bytes.len() > 11 && matches!(bytes[10], b'T' | b't') && date(&bytes[..10]) && time(&bytes[11..])
}

fn unit(bytes: &[u8], letter: u8) -> Option<&[u8]> {
    let length = bytes.iter().take_while(|byte| byte.is_ascii_digit()).count();

    (length > 0 && bytes.get(length) == Some(&letter)).then(|| &bytes[length + 1..])
}

fn units<'a>(bytes: &'a [u8], letters: &[u8]) -> Option<&'a [u8]> {
    let (first, rest) = letters.split_first()?;

    let Some(mut after) = unit(bytes, *first) else {
        return units(bytes, rest);
    };

    for letter in rest {
        match unit(after, *letter) {
            Some(next) => after = next,
            None => break,
        }
    }

    Some(after)
}

fn duration(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(b"P") else {
        return false;
    };

    if let Some(after) = unit(rest, b'W') {
        return after.is_empty();
    }

    let rest = units(rest, b"YMD").unwrap_or(rest);

    if rest.len() == bytes.len() - 1 && !rest.starts_with(b"T") {
        return false;
    }

    match rest.strip_prefix(b"T") {
        Some(time) => units(time, b"HMS").is_some_and(<[u8]>::is_empty),
        None => rest.is_empty(),
    }
}

fn email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };

    let atom = |text: &str| {
        !text.is_empty()
            && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+/=?^_`{|}~-".contains(&byte))
    };
    let label = |text: &str| {
        let bytes = text.as_bytes();

        !bytes.is_empty()
            && bytes[0].is_ascii_alphanumeric()
            && bytes[bytes.len() - 1].is_ascii_alphanumeric()
            && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
    };

    local.len() <= 64 && local.split('.').all(atom) && domain.contains('.') && domain.split('.').all(label)
}

fn uuid(bytes: &[u8]) -> bool {
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn uri(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };

    let scheme_ok = scheme.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && scheme.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte));

    if !scheme_ok || rest.is_empty() || !uri_chars(rest) {
        return false;
    }

    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, host)| host);

    if host_port.contains('@') {
        return false;
    }

    let (host, port) = if let Some(literal) = host_port.strip_prefix('[') {
        let Some((address, after)) = literal.split_once(']') else {
            return false;
        };

        if address.parse::<Ipv6Addr>().is_err() {
            return false;
        }

        match after {
            "" => ("[]", None),
            _ => match after.strip_prefix(':') {
                Some(port) => ("[]", Some(port)),
                None => return false,
            },
        }
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };

    if host != "[]" && host.contains(['[', ']', ':']) {
        return false;
    }

    if let Some(port) = port
        && !port.is_empty()
        && digits(port.as_bytes()).is_none_or(|number| number > 65535 || port.len() > 5)
    {
        return false;
    }

    let special = ["http", "https", "ws", "wss", "ftp"].iter().any(|name| scheme.eq_ignore_ascii_case(name));

    !special || (!host.is_empty() && special_host(host))
}

fn uri_chars(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];

        if byte == b'%' {
            if !(bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
                && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit))
            {
                return false;
            }

            index += 3;
            continue;
        }

        if !(byte.is_ascii_alphanumeric() || b"_-.~!$&'()*+,;=:@/?#[]".contains(&byte)) {
            return false;
        }

        index += 1;
    }

    true
}

fn special_host(host: &str) -> bool {
    if host == "[]" {
        return true;
    }

    let labels: Vec<&str> = host.split('.').collect();
    let last = labels.iter().rev().find(|label| !label.is_empty()).copied().unwrap_or("");
    let numeric = !last.is_empty()
        && (last.bytes().all(|byte| byte.is_ascii_digit())
            || last.len() > 1
                && last[..2].eq_ignore_ascii_case("0x")
                && last[2..].bytes().all(|b| b.is_ascii_hexdigit()));

    !host.contains('%') && (!numeric || host.parse::<Ipv4Addr>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(value: &str) -> Option<&'static str> {
        of(value).map(Format::name)
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(check("2026-10-27T10:00:00Z"), Some("date-time"));
        assert_eq!(check("2026-10-27t10:00:00.125+03:00"), Some("date-time"));
        assert_eq!(check("2026-10-27 10:00:00Z"), None);
        assert_eq!(check("2026-10-27T23:59:60Z"), None);
        assert_eq!(check("2026-02-29T10:00:00Z"), None);
        assert_eq!(check("2024-02-29"), Some("date"));
        assert_eq!(check("2026-02-29"), None);
        assert_eq!(check("10:00:00Z"), Some("time"));
        assert_eq!(check("10:00:00.5+03:00"), Some("time"));
        assert_eq!(check("10:00:00"), None);
        assert_eq!(check("10:00"), None);
        assert_eq!(check("24:00:00Z"), None);
    }

    #[test]
    fn durations() {
        for value in ["P1D", "P1Y", "P1Y2M", "P1Y2M3D", "P1M3D", "PT1H", "PT1H30M", "PT5S", "P1DT2H", "P2W"] {
            assert_eq!(check(value), Some("duration"), "{value}");
        }

        for value in ["P", "PT", "P1", "P1Y3D", "PT1H5S", "P1DT", "P1W2D", "P1.5D", "p1d", "P-1D"] {
            assert_eq!(check(value), None, "{value}");
        }
    }

    #[test]
    fn emails() {
        assert_eq!(check("alexey@simonenko.dev"), Some("email"));
        assert_eq!(check("a.b+tag@mail.simonenko.xyz"), Some("email"));
        assert_eq!(check("a..b@simonenko.dev"), None);
        assert_eq!(check(".a@simonenko.dev"), None);
        assert_eq!(check("алексей@simonenko.dev"), None);
        assert_eq!(check("alexey@localhost"), None);
        assert_eq!(check("alexey@-simonenko.dev"), None);
        assert_eq!(check(&format!("{}@simonenko.dev", "a".repeat(65))), None);
    }

    #[test]
    fn identifiers_and_addresses() {
        assert_eq!(check("0b7c3f8e-2f4a-4c1e-9d3b-5a6e7f8a9b0c"), Some("uuid"));
        assert_eq!(check("0B7C3F8E-2F4A-4C1E-9D3B-5A6E7F8A9B0C"), Some("uuid"));
        assert_eq!(check("0b7c3f8e2f4a4c1e9d3b5a6e7f8a9b0c"), None);
        assert_eq!(check("192.168.0.1"), Some("ipv4"));
        assert_eq!(check("192.168.0.256"), None);
        assert_eq!(check("01.1.1.1"), None);
        assert_eq!(check("1.2.3"), None);
        assert_eq!(check("2001:db8::1"), Some("ipv6"));
        assert_eq!(check("::ffff:192.168.0.1"), Some("ipv6"));
        assert_eq!(check("fe80::1%eth0"), None);
    }

    #[test]
    fn uris() {
        assert_eq!(check("https://simonenko.xyz/blog/"), Some("uri"));
        assert_eq!(check("https://simonenko.xyz/%D0%B1%D0%BB%D0%BE%D0%B3/"), Some("uri"));
        assert_eq!(check("https://simonenko.xyz/блог/"), None);
        assert_eq!(check("https://simonenko.xyz/%zz"), None);
        assert_eq!(check("https://user:pass@simonenko.xyz:8080/?q=1#top"), Some("uri"));
        assert_eq!(check("https://[::1]:8080/"), Some("uri"));
        assert_eq!(check("https://[::1/"), None);
        assert_eq!(check("https://simonenko.xyz:99999/"), None);
        assert_eq!(check("https:///path"), None);
        assert_eq!(check("https://1.2.3.256/"), None);
        assert_eq!(check("file:///etc/hosts"), Some("uri"));
        assert_eq!(check("mailto:alexey@simonenko.dev"), None);
        assert_eq!(check("simonenko.xyz/blog"), None);
    }
}
