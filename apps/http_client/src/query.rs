use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

const QUERY_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub(super) fn url_with_default_scheme(value: &str) -> Option<String> {
    let value = value.strip_prefix("//").unwrap_or(value);
    let authority = value.split(['/', '?', '#']).next()?;
    let port = if let Some(ip) = authority.strip_prefix('[') {
        let (ip, suffix) = ip.split_once(']')?;
        ip.parse::<std::net::Ipv6Addr>().ok()?;
        if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':')?)
        }
    } else {
        let (host, port) = authority
            .split_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)));
        let host = host.strip_suffix('.').unwrap_or(host);
        if !host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_'))
        }) {
            return None;
        }
        port
    };
    if let Some(port) = port {
        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        port.parse::<u16>().ok()?;
    }
    Some(format!("http://{value}"))
}

pub(super) fn split_url(url: &str) -> (&str, Option<&str>, &str) {
    let (url, fragment) = url.split_at(url.find('#').unwrap_or(url.len()));
    match url.split_once('?') {
        Some((base, query)) => (base, Some(query), fragment),
        None => (url, None, fragment),
    }
}

pub(super) fn with_query(url: &str, query: Option<&str>) -> String {
    let (base, _, fragment) = split_url(url);
    match query {
        Some(query) => format!("{base}?{query}{fragment}"),
        None => format!("{base}{fragment}"),
    }
}

pub(super) fn decode_pair(pair: &str) -> (String, Option<String>) {
    let decode = |text: &str| {
        percent_decode_str(&text.replace('+', " "))
            .decode_utf8_lossy()
            .into_owned()
    };
    match pair.split_once('=') {
        Some((key, value)) => (decode(key), Some(decode(value))),
        None => (decode(pair), None),
    }
}

pub(super) fn encode_pair(
    key: &str,
    value: Option<&str>,
    original: Option<&str>,
) -> Option<String> {
    // Keep the URL spelling of each untouched component, including + and existing escapes.
    let (raw_key, raw_value) = original.map_or(("", None), |pair| match pair.split_once('=') {
        Some((key, value)) => (key, Some(value)),
        None => (pair, None),
    });
    let decoded = original.map(decode_pair);
    let key = if decoded
        .as_ref()
        .is_some_and(|(original, _)| same_input_text(original, key))
    {
        raw_key.to_owned()
    } else {
        utf8_percent_encode(key, QUERY_COMPONENT).to_string()
    };
    match value {
        Some(value) => {
            let encoded = match raw_value {
                Some(raw)
                    if decoded
                        .as_ref()
                        .and_then(|(_, value)| value.as_deref())
                        .is_some_and(|original| same_input_text(original, value)) =>
                {
                    raw.to_owned()
                }
                _ => utf8_percent_encode(value, QUERY_COMPONENT).to_string(),
            };
            Some(format!("{key}={encoded}"))
        }
        None => (!key.is_empty()).then_some(key),
    }
}

fn same_input_text(decoded: &str, text: &str) -> bool {
    // Single-line inputs omit CR and LF. Preserve those bytes at rest in the URL.
    decoded
        .bytes()
        .filter(|byte| !matches!(byte, b'\r' | b'\n'))
        .eq(text.bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_http_to_hosts_without_a_scheme() {
        assert_eq!(
            url_with_default_scheme("localhost:8080/path?q=a#b").as_deref(),
            Some("http://localhost:8080/path?q=a#b")
        );
        assert_eq!(
            url_with_default_scheme("LOCALHOST").as_deref(),
            Some("http://LOCALHOST")
        );
        assert_eq!(
            url_with_default_scheme("//localhost/test").as_deref(),
            Some("http://localhost/test")
        );
        for url in [
            "example.com",
            "internal:9200/path",
            "127.0.0.1",
            "[::1]:8080",
        ] {
            assert_eq!(url_with_default_scheme(url), Some(format!("http://{url}")));
        }
        for url in [
            "https://localhost:8080/",
            "http://localhost/",
            "localhost:abc",
            "ftp://example.com/",
            "/relative",
            "host name",
            "host:65536",
        ] {
            assert_eq!(url_with_default_scheme(url), None);
        }
    }

    #[test]
    fn decodes_flags_empty_values_and_embedded_delimiters() {
        assert_eq!(decode_pair("flag"), ("flag".into(), None));
        assert_eq!(decode_pair("flag="), ("flag".into(), Some(String::new())));
        assert_eq!(
            decode_pair("a+b=%E2%9C%93%26x=y%2B"),
            ("a b".into(), Some("✓&x=y+".into()))
        );
        assert_eq!(
            encode_pair("a+b", Some("✓&x=y "), None).as_deref(),
            Some("a%2Bb=%E2%9C%93%26x%3Dy%20")
        );
        assert_eq!(
            encode_pair("flag", None, Some("flag=")).as_deref(),
            Some("flag")
        );
        assert_eq!(
            encode_pair("flag", Some(""), Some("flag")).as_deref(),
            Some("flag=")
        );
        assert_eq!(encode_pair("", None, None), None);
        assert_eq!(encode_pair("", Some(""), None).as_deref(), Some("="));
    }

    #[test]
    fn changes_only_the_query_and_preserves_unchanged_component_spelling() {
        assert_eq!(
            encode_pair("key", Some("ab"), Some("key=a%0Ab")).as_deref(),
            Some("key=a%0Ab")
        );
        assert_eq!(
            split_url("host/path?a=1&a=2#frag?still-fragment"),
            ("host/path", Some("a=1&a=2"), "#frag?still-fragment")
        );
        assert_eq!(
            with_query("host/path?a=1#frag?x", Some("flag")),
            "host/path?flag#frag?x"
        );
        assert_eq!(
            with_query("host/path?flag#frag?x", None),
            "host/path#frag?x"
        );
        assert_eq!(
            encode_pair("a b", Some("new&"), Some("a+b=old")).as_deref(),
            Some("a+b=new%26")
        );
        assert_eq!(
            encode_pair("new", Some("%bad"), Some("a=%25bad")).as_deref(),
            Some("new=%25bad")
        );
    }
}
