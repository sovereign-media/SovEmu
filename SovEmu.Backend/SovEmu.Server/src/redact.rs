//! Redaction for settings that are shown: the startup log, and the admin pages that list the
//! configuration. A value that can carry a credential without being a secret outright (a URL, a
//! list of ICE servers, a header list) is shown with the credential masked.

/// What a masked credential reads as.
pub(crate) const MASK: &str = "***";

/// A URL with any userinfo and query masked: `https://user:token@host/p?key=x` reads
/// `https://***@host/p?***`. Something that does not parse as a URL is masked whole, since it
/// cannot be told apart from a credential.
pub(crate) fn url(raw: &str) -> String {
    let Ok(parsed) = ::url::Url::parse(raw.trim()) else {
        return MASK.to_owned();
    };
    let mut out = format!("{}://", parsed.scheme());
    if !parsed.username().is_empty() || parsed.password().is_some() {
        out.push_str(MASK);
        out.push('@');
    }
    if let Some(host) = parsed.host_str() {
        out.push_str(host);
    }
    if let Some(port) = parsed.port() {
        out.push_str(&format!(":{port}"));
    }
    out.push_str(parsed.path());
    if parsed.query().is_some() {
        out.push('?');
        out.push_str(MASK);
    }
    out
}

/// The names in an `OTEL_EXPORTER_OTLP_HEADERS`-style list (`key1=value1,key2=value2`), without
/// their values, which are usually tokens.
pub(crate) fn header_names(raw: &str) -> Vec<String> {
    raw.split(',')
        .filter_map(|pair| {
            let name = pair.split_once('=').map_or(pair, |(name, _)| name).trim();
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_lose_their_credentials() {
        assert_eq!(url("http://collector:4318"), "http://collector:4318/");
        assert_eq!(url("https://collector.example/otlp"), "https://collector.example/otlp");
        assert_eq!(url("https://user:token@collector:4318/x"), "https://***@collector:4318/x");
        assert_eq!(url("https://token@collector/"), "https://***@collector/");
        assert_eq!(url("https://collector/v1?api-key=abc&x=1"), "https://collector/v1?***");
        assert_eq!(url("https://u:p@collector/?k=v#frag"), "https://***@collector/?***");
        assert_eq!(url("not a url, maybe a secret"), MASK);
        let shown = url("https://u:secret@h/?api-key=abc");
        assert!(!shown.contains("secret") && !shown.contains("abc"), "{shown}");
    }

    #[test]
    fn header_lists_show_names_only() {
        assert_eq!(header_names("authorization=Bearer x, x-team = 7"), ["authorization", "x-team"]);
        assert_eq!(header_names(""), Vec::<String>::new());
        assert_eq!(header_names("bare"), ["bare"]);
    }
}
