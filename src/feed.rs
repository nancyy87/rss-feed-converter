// Shared feed model plus hand-rolled XML and JSON handling. No third-party
// crates, so each format gets just enough of a parser to round-trip the
// fields RSS 2.0, Atom, and JSON Feed 1.1 actually share.

pub struct Feed {
    pub title: String,
    pub link: String,
    pub description: String,
    pub items: Vec<Item>,
    // JSON Feed fields we don't model directly (icon, language, custom
    // "_foo" extensions, ...), kept so a JSON-Feed-to-JSON-Feed pass
    // doesn't silently drop them. Empty for RSS/Atom input.
    pub extensions: Vec<(String, JsonValue)>,
    // Namespaced elements from an RSS/Atom source we don't understand
    // (media:content, dc:creator, atom:link, ...), kept as raw markup so an
    // RSS/Atom-to-RSS/Atom pass doesn't silently drop them. Empty for JSON
    // Feed input, since JSON Feed has no equivalent extension mechanism.
    pub xml_extensions: Vec<String>,
    // xmlns:prefix declarations pulled off the source root element, needed
    // so the prefixes used in xml_extensions stay bound when we write them
    // back out. Empty for JSON Feed input.
    pub xml_namespaces: Vec<(String, String)>,
}

pub struct Item {
    pub id: String,
    pub title: String,
    pub link: String,
    pub content: String,
    pub pub_date: String,
    pub enclosures: Vec<Enclosure>,
    pub extensions: Vec<(String, JsonValue)>,
    pub xml_extensions: Vec<String>,
}

pub struct Enclosure {
    pub url: String,
    pub mime_type: String,
    pub length: Option<u64>,
}

// ---------- minimal XML reading ----------

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        let mut entity = String::new();
        let mut closed = false;
        while let Some(&next) = chars.peek() {
            if next == ';' {
                chars.next();
                closed = true;
                break;
            }
            if entity.len() > 10 {
                break;
            }
            entity.push(next);
            chars.next();
        }
        if !closed {
            out.push('&');
            out.push_str(&entity);
            continue;
        }
        match entity.as_str() {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                if let Ok(code) = u32::from_str_radix(&entity[2..], 16) {
                    if let Some(ch) = char::from_u32(code) {
                        out.push(ch);
                    }
                }
            }
            _ if entity.starts_with('#') => {
                if let Ok(code) = entity[1..].parse::<u32>() {
                    if let Some(ch) = char::from_u32(code) {
                        out.push(ch);
                    }
                }
            }
            _ => {
                out.push('&');
                out.push_str(&entity);
                out.push(';');
            }
        }
    }
    out
}

fn strip_cdata(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("<![CDATA[") {
        if let Some(inner) = rest.strip_suffix("]]>") {
            return inner.to_string();
        }
    }
    decode_entities(trimmed)
}

fn is_tag_boundary(c: char) -> bool {
    c == '>' || c == '/' || c.is_whitespace()
}

/// Finds the first `<tag ...>...</tag>` in `s` and returns its raw
/// (undecoded, CDATA-untouched) inner text, or `Some("")` for a
/// self-closing `<tag/>`.
fn extract_tag_raw(s: &str, tag: &str) -> Option<String> {
    let open_needle = format!("<{}", tag);
    let mut search_from = 0;
    loop {
        let rel = s[search_from..].find(&open_needle)?;
        let start = search_from + rel;
        let after = start + open_needle.len();
        let next_char = s[after..].chars().next()?;
        if !is_tag_boundary(next_char) {
            search_from = after;
            continue;
        }
        let close_of_open = s[after..].find('>')? + after;
        if s.as_bytes()[close_of_open - 1] == b'/' {
            return Some(String::new());
        }
        let content_start = close_of_open + 1;
        let close_needle = format!("</{}>", tag);
        let rel_end = s[content_start..].find(&close_needle)?;
        let content_end = content_start + rel_end;
        return Some(s[content_start..content_end].to_string());
    }
}

/// Finds the first `<tag ...>...</tag>` in `s` and returns its decoded inner text.
fn extract_tag(s: &str, tag: &str) -> Option<String> {
    extract_tag_raw(s, tag).map(|raw| strip_cdata(&raw))
}

/// Finds every top-level `<tag ...>...</tag>` block in `s` and returns each
/// one's raw (undecoded) inner text, so callers can run extract_tag on it again.
fn extract_all_tag(s: &str, tag: &str) -> Vec<String> {
    let mut result = Vec::new();
    let open_needle = format!("<{}", tag);
    let close_needle = format!("</{}>", tag);
    let mut pos = 0;
    while let Some(rel) = s[pos..].find(&open_needle) {
        let start = pos + rel;
        let after = start + open_needle.len();
        let next_char = match s[after..].chars().next() {
            Some(c) => c,
            None => break,
        };
        if !is_tag_boundary(next_char) {
            pos = after;
            continue;
        }
        let close_of_open = match s[after..].find('>') {
            Some(i) => after + i,
            None => break,
        };
        if s.as_bytes()[close_of_open - 1] == b'/' {
            result.push(String::new());
            pos = close_of_open + 1;
            continue;
        }
        let content_start = close_of_open + 1;
        let rel_end = match s[content_start..].find(&close_needle) {
            Some(i) => i,
            None => break,
        };
        let content_end = content_start + rel_end;
        result.push(s[content_start..content_end].to_string());
        pos = content_end + close_needle.len();
    }
    result
}

/// Finds every top-level `<tag ...>` opening tag in `s` (self-closing or
/// not) and returns each one's raw attribute text, so callers can pull
/// attribute values out of elements like Atom's `<link href="...">`.
fn extract_all_tag_attrs(s: &str, tag: &str) -> Vec<String> {
    let mut result = Vec::new();
    let open_needle = format!("<{}", tag);
    let mut pos = 0;
    while let Some(rel) = s[pos..].find(&open_needle) {
        let start = pos + rel;
        let after = start + open_needle.len();
        let next_char = match s[after..].chars().next() {
            Some(c) => c,
            None => break,
        };
        if !is_tag_boundary(next_char) {
            pos = after;
            continue;
        }
        let close_of_open = match s[after..].find('>') {
            Some(i) => after + i,
            None => break,
        };
        result.push(s[after..close_of_open].to_string());
        pos = close_of_open + 1;
    }
    result
}

/// Reads `attr="value"` (or `attr='value'`) out of a tag's raw attribute
/// text, as produced by `extract_all_tag_attrs`.
fn extract_attr_value(tag_attrs: &str, attr: &str) -> Option<String> {
    let needle = format!("{}=", attr);
    let mut idx = 0;
    while let Some(rel) = tag_attrs[idx..].find(&needle) {
        let pos = idx + rel;
        if pos > 0 && !tag_attrs.as_bytes()[pos - 1].is_ascii_whitespace() {
            idx = pos + needle.len();
            continue;
        }
        let after_eq = pos + needle.len();
        let quote = tag_attrs[after_eq..].chars().next()?;
        if quote != '"' && quote != '\'' {
            idx = after_eq;
            continue;
        }
        let value_start = after_eq + quote.len_utf8();
        let rel_end = tag_attrs[value_start..].find(quote)?;
        return Some(decode_entities(&tag_attrs[value_start..value_start + rel_end]));
    }
    None
}

/// Walks the top-level elements of `s` and returns each one's tag name
/// alongside its raw full markup (open tag through matching close tag, or
/// the bare self-closing tag). Like `extract_all_tag`, matching is by tag
/// name rather than real nesting depth, which is fine for well-formed feed
/// markup.
fn top_level_elements(s: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut pos = 0;
    while pos < s.len() {
        let rel = match s[pos..].find('<') {
            Some(r) => r,
            None => break,
        };
        let start = pos + rel;
        if s[start..].starts_with("<!--") {
            pos = match s[start..].find("-->") {
                Some(i) => start + i + 3,
                None => break,
            };
            continue;
        }
        if s[start..].starts_with("<!") || s[start..].starts_with("<?") {
            pos = match s[start..].find('>') {
                Some(i) => start + i + 1,
                None => break,
            };
            continue;
        }
        let after = start + 1;
        let name_end = match s[after..].find(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            Some(i) => after + i,
            None => break,
        };
        let name = s[after..name_end].to_string();
        let close_of_open = match s[after..].find('>') {
            Some(i) => after + i,
            None => break,
        };
        if bytes[close_of_open - 1] == b'/' {
            result.push((name, s[start..close_of_open + 1].to_string()));
            pos = close_of_open + 1;
            continue;
        }
        let content_start = close_of_open + 1;
        let close_needle = format!("</{}>", name);
        let content_end = match s[content_start..].find(&close_needle) {
            Some(i) => content_start + i + close_needle.len(),
            None => break,
        };
        result.push((name, s[start..content_end].to_string()));
        pos = content_end;
    }
    result
}

/// Returns the raw markup of every top-level namespaced element (tag names
/// containing a `:`) in `s` - the RSS/Atom extension elements we don't parse
/// into our own model, kept so they round-trip instead of getting dropped.
fn xml_extensions(s: &str) -> Vec<String> {
    top_level_elements(s)
        .into_iter()
        .filter(|(name, _)| name.contains(':'))
        .map(|(_, raw)| raw)
        .collect()
}

/// Reads every `xmlns:prefix="uri"` declaration out of a tag's raw
/// attribute text, so a preserved extension element's namespace prefix
/// (e.g. `media:` in `<media:content>`) stays bound when re-emitted.
fn extract_xmlns_decls(tag_attrs: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let needle = "xmlns:";
    let mut idx = 0;
    while let Some(rel) = tag_attrs[idx..].find(needle) {
        let pos = idx + rel;
        if pos > 0 && !tag_attrs.as_bytes()[pos - 1].is_ascii_whitespace() {
            idx = pos + needle.len();
            continue;
        }
        let name_start = pos + needle.len();
        let name_end = tag_attrs[name_start..]
            .find('=')
            .map(|i| name_start + i)
            .unwrap_or(tag_attrs.len());
        let prefix = tag_attrs[name_start..name_end].trim().to_string();
        let attr_name = format!("xmlns:{}", prefix);
        if let Some(value) = extract_attr_value(tag_attrs, &attr_name) {
            result.push((prefix, value));
        }
        idx = name_end;
    }
    result
}

/// Picks the href of an Atom `<link>` element, preferring `rel="alternate"`
/// (the default per the spec when `rel` is omitted) over other relations
/// like `self` or `enclosure`.
fn atom_link_href(s: &str) -> String {
    let mut fallback = None;
    for attrs in extract_all_tag_attrs(s, "link") {
        let rel = extract_attr_value(&attrs, "rel");
        if let Some(href) = extract_attr_value(&attrs, "href") {
            if rel.is_none() || rel.as_deref() == Some("alternate") {
                return href;
            }
            fallback.get_or_insert(href);
        }
    }
    fallback.unwrap_or_default()
}

/// Finds the root element name of an XML document, skipping the `<?xml?>`
/// declaration, comments, and doctype. Used to tell an RSS `<rss>` document
/// apart from an Atom `<feed>` document before picking a parser.
pub fn xml_root_tag(s: &str) -> Option<String> {
    let mut idx = 0;
    loop {
        while idx < s.len() && s.as_bytes()[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= s.len() || s.as_bytes()[idx] != b'<' {
            return None;
        }
        let rest = &s[idx..];
        if rest.starts_with("<?") {
            idx += rest.find("?>")? + 2;
            continue;
        }
        if rest.starts_with("<!--") {
            idx += rest.find("-->")? + 3;
            continue;
        }
        if rest.starts_with("<!") {
            idx += rest.find('>')? + 1;
            continue;
        }
        let after = idx + 1;
        let name_end = s[after..].find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
        return Some(s[after..after + name_end].to_string());
    }
}

/// Reads every top-level `<enclosure url="..." type="..." length="...">` in
/// `s`, skipping any that lack a `url` (the only attribute RSS requires).
fn parse_enclosures_rss(s: &str) -> Vec<Enclosure> {
    extract_all_tag_attrs(s, "enclosure")
        .iter()
        .filter_map(|attrs| {
            let url = extract_attr_value(attrs, "url")?;
            let mime_type = extract_attr_value(attrs, "type").unwrap_or_default();
            let length = extract_attr_value(attrs, "length").and_then(|s| s.parse().ok());
            Some(Enclosure { url, mime_type, length })
        })
        .collect()
}

pub fn parse_rss(xml: &str) -> Result<Feed, String> {
    let channel = extract_tag(xml, "channel").ok_or("no <channel> element found")?;
    let xml_namespaces = extract_all_tag_attrs(xml, "rss")
        .first()
        .map(|attrs| extract_xmlns_decls(attrs))
        .unwrap_or_default();
    let mut feed = Feed {
        title: extract_tag(&channel, "title").unwrap_or_default(),
        link: extract_tag(&channel, "link").unwrap_or_default(),
        description: extract_tag(&channel, "description").unwrap_or_default(),
        items: Vec::new(),
        extensions: Vec::new(),
        xml_extensions: xml_extensions(&channel),
        xml_namespaces,
    };
    for raw_item in extract_all_tag(&channel, "item") {
        feed.items.push(Item {
            id: extract_tag(&raw_item, "guid").unwrap_or_default(),
            title: extract_tag(&raw_item, "title").unwrap_or_default(),
            link: extract_tag(&raw_item, "link").unwrap_or_default(),
            content: extract_tag(&raw_item, "description").unwrap_or_default(),
            pub_date: extract_tag(&raw_item, "pubDate").unwrap_or_default(),
            enclosures: parse_enclosures_rss(&raw_item),
            extensions: Vec::new(),
            xml_extensions: xml_extensions(&raw_item),
        });
    }
    Ok(feed)
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_xml_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn write_rss(feed: &Feed) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<rss version=\"2.0\"");
    for (prefix, uri) in &feed.xml_namespaces {
        out.push_str(&format!(" xmlns:{}=\"{}\"", prefix, escape_xml_attr(uri)));
    }
    out.push_str(">\n<channel>\n");
    out.push_str(&format!("<title>{}</title>\n", escape_xml(&feed.title)));
    out.push_str(&format!("<link>{}</link>\n", escape_xml(&feed.link)));
    out.push_str(&format!(
        "<description>{}</description>\n",
        escape_xml(&feed.description)
    ));
    for extension in &feed.xml_extensions {
        out.push_str(extension);
        out.push('\n');
    }
    for item in &feed.items {
        out.push_str("<item>\n");
        out.push_str(&format!("<title>{}</title>\n", escape_xml(&item.title)));
        out.push_str(&format!("<link>{}</link>\n", escape_xml(&item.link)));
        out.push_str(&format!(
            "<description>{}</description>\n",
            escape_xml(&item.content)
        ));
        if !item.id.is_empty() {
            out.push_str(&format!("<guid>{}</guid>\n", escape_xml(&item.id)));
        }
        if !item.pub_date.is_empty() {
            let pub_date = crate::date::to_rfc822(&item.pub_date);
            out.push_str(&format!("<pubDate>{}</pubDate>\n", escape_xml(&pub_date)));
        }
        for enclosure in &item.enclosures {
            out.push_str(&format!(
                "<enclosure url=\"{}\"{}{}/>\n",
                escape_xml_attr(&enclosure.url),
                if enclosure.mime_type.is_empty() {
                    String::new()
                } else {
                    format!(" type=\"{}\"", escape_xml_attr(&enclosure.mime_type))
                },
                enclosure.length.map(|l| format!(" length=\"{}\"", l)).unwrap_or_default()
            ));
        }
        for extension in &item.xml_extensions {
            out.push_str(extension);
            out.push('\n');
        }
        out.push_str("</item>\n");
    }
    out.push_str("</channel>\n</rss>\n");
    out
}

// ---------- Atom ----------

/// Reads every `<link rel="enclosure" href="..." type="..." length="...">`
/// in `s`. Atom marks enclosures via the link relation rather than a
/// dedicated element, unlike RSS.
fn parse_enclosures_atom(s: &str) -> Vec<Enclosure> {
    extract_all_tag_attrs(s, "link")
        .iter()
        .filter(|attrs| extract_attr_value(attrs, "rel").as_deref() == Some("enclosure"))
        .filter_map(|attrs| {
            let url = extract_attr_value(attrs, "href")?;
            let mime_type = extract_attr_value(attrs, "type").unwrap_or_default();
            let length = extract_attr_value(attrs, "length").and_then(|s| s.parse().ok());
            Some(Enclosure { url, mime_type, length })
        })
        .collect()
}

/// Reads an Atom text construct (`<content>` or `<summary>`) as plain HTML
/// markup. Per the Atom spec these can carry `type="xhtml"`, which wraps
/// the actual markup in an inline `<div xmlns="...">` rather than escaping
/// it as text the way `type="html"` does. Unwrap that div so `Item::content`
/// always ends up holding the same kind of raw HTML string regardless of
/// which content type the source used, instead of leaving the xhtml div
/// wrapper (and its xmlns attribute) sitting in the content body.
fn atom_text_construct(entry: &str, tag: &str) -> Option<String> {
    let content_type = extract_all_tag_attrs(entry, tag)
        .into_iter()
        .next()
        .and_then(|attrs| extract_attr_value(&attrs, "type"));
    let raw = extract_tag_raw(entry, tag)?;
    if content_type.as_deref() == Some("xhtml") {
        Some(extract_tag(&raw, "div").unwrap_or_else(|| strip_cdata(&raw)))
    } else {
        Some(strip_cdata(&raw))
    }
}

pub fn parse_atom(xml: &str) -> Result<Feed, String> {
    let root = extract_tag(xml, "feed").ok_or("no <feed> element found")?;
    let xml_namespaces = extract_all_tag_attrs(xml, "feed")
        .first()
        .map(|attrs| extract_xmlns_decls(attrs))
        .unwrap_or_default();
    let mut feed = Feed {
        title: extract_tag(&root, "title").unwrap_or_default(),
        link: atom_link_href(&root),
        description: extract_tag(&root, "subtitle").unwrap_or_default(),
        items: Vec::new(),
        extensions: Vec::new(),
        xml_extensions: xml_extensions(&root),
        xml_namespaces,
    };
    for raw_entry in extract_all_tag(&root, "entry") {
        let content = atom_text_construct(&raw_entry, "content")
            .or_else(|| atom_text_construct(&raw_entry, "summary"))
            .unwrap_or_default();
        let pub_date = extract_tag(&raw_entry, "published")
            .or_else(|| extract_tag(&raw_entry, "updated"))
            .unwrap_or_default();
        feed.items.push(Item {
            id: extract_tag(&raw_entry, "id").unwrap_or_default(),
            title: extract_tag(&raw_entry, "title").unwrap_or_default(),
            link: atom_link_href(&raw_entry),
            content,
            pub_date,
            enclosures: parse_enclosures_atom(&raw_entry),
            extensions: Vec::new(),
            xml_extensions: xml_extensions(&raw_entry),
        });
    }
    Ok(feed)
}

pub fn write_atom(feed: &Feed) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<feed xmlns=\"http://www.w3.org/2005/Atom\"");
    for (prefix, uri) in &feed.xml_namespaces {
        out.push_str(&format!(" xmlns:{}=\"{}\"", prefix, escape_xml_attr(uri)));
    }
    out.push_str(">\n");
    out.push_str(&format!("<title>{}</title>\n", escape_xml(&feed.title)));
    // Atom requires a feed <id>; fall back to the title when there's no link
    // to use, since the source feed may not carry anything id-shaped at all.
    let feed_id = if !feed.link.is_empty() { &feed.link } else { &feed.title };
    out.push_str(&format!("<id>{}</id>\n", escape_xml(feed_id)));
    if !feed.link.is_empty() {
        out.push_str(&format!(
            "<link href=\"{}\" rel=\"alternate\"/>\n",
            escape_xml_attr(&feed.link)
        ));
    }
    if !feed.description.is_empty() {
        out.push_str(&format!("<subtitle>{}</subtitle>\n", escape_xml(&feed.description)));
    }
    // Atom also requires feed <updated>; use the newest item date we have.
    let feed_updated = feed
        .items
        .iter()
        .map(|item| item.pub_date.as_str())
        .find(|d| !d.is_empty())
        .map(crate::date::to_iso8601)
        .unwrap_or_default();
    if !feed_updated.is_empty() {
        out.push_str(&format!("<updated>{}</updated>\n", escape_xml(&feed_updated)));
    }
    for extension in &feed.xml_extensions {
        out.push_str(extension);
        out.push('\n');
    }
    for item in &feed.items {
        out.push_str("<entry>\n");
        out.push_str(&format!("<title>{}</title>\n", escape_xml(&item.title)));
        let entry_id = if !item.id.is_empty() { &item.id } else { &item.link };
        if !entry_id.is_empty() {
            out.push_str(&format!("<id>{}</id>\n", escape_xml(entry_id)));
        }
        if !item.link.is_empty() {
            out.push_str(&format!(
                "<link href=\"{}\" rel=\"alternate\"/>\n",
                escape_xml_attr(&item.link)
            ));
        }
        if !item.content.is_empty() {
            out.push_str(&format!(
                "<content type=\"html\">{}</content>\n",
                escape_xml(&item.content)
            ));
        }
        if !item.pub_date.is_empty() {
            let updated = crate::date::to_iso8601(&item.pub_date);
            out.push_str(&format!("<updated>{}</updated>\n", escape_xml(&updated)));
            out.push_str(&format!("<published>{}</published>\n", escape_xml(&updated)));
        }
        for enclosure in &item.enclosures {
            out.push_str(&format!(
                "<link rel=\"enclosure\" href=\"{}\"{}{}/>\n",
                escape_xml_attr(&enclosure.url),
                if enclosure.mime_type.is_empty() {
                    String::new()
                } else {
                    format!(" type=\"{}\"", escape_xml_attr(&enclosure.mime_type))
                },
                enclosure.length.map(|l| format!(" length=\"{}\"", l)).unwrap_or_default()
            ));
        }
        for extension in &item.xml_extensions {
            out.push_str(extension);
            out.push('\n');
        }
        out.push_str("</entry>\n");
    }
    out.push_str("</feed>\n");
    out
}

// ---------- minimal JSON reading/writing ----------

#[derive(Clone)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Vec<(String, JsonValue)>> {
        match self {
            JsonValue::Object(pairs) => Some(pairs),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Number(n) if *n >= 0.0 => Some(*n as u64),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a str) -> Self {
        JsonParser {
            chars: input.chars().peekable(),
        }
    }

    fn skip_ws(&mut self) {
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() {
                self.chars.next();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        match self.chars.next() {
            Some(c) if c == expected => Ok(()),
            other => Err(format!("expected '{}', found {:?}", expected, other)),
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        match self.chars.peek() {
            Some('"') => self.parse_string().map(JsonValue::String),
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('t') | Some('f') => self.parse_bool(),
            Some('n') => self.parse_null(),
            Some(c) if c.is_ascii_digit() || *c == '-' => self.parse_number(),
            other => Err(format!("unexpected character in JSON: {:?}", other)),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.chars.next() {
                Some('"') => return Ok(out),
                Some('\\') => match self.chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('/') => out.push('/'),
                    Some('b') => out.push('\u{8}'),
                    Some('f') => out.push('\u{c}'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('u') => {
                        let mut code = String::new();
                        for _ in 0..4 {
                            code.push(self.chars.next().ok_or("truncated unicode escape")?);
                        }
                        let value = u32::from_str_radix(&code, 16).map_err(|e| e.to_string())?;
                        if let Some(ch) = char::from_u32(value) {
                            out.push(ch);
                        }
                    }
                    other => return Err(format!("invalid escape: {:?}", other)),
                },
                Some(c) => out.push(c),
                None => return Err("unterminated string".to_string()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect('{')?;
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.chars.peek() == Some(&'}') {
            self.chars.next();
            return Ok(JsonValue::Object(pairs));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(':')?;
            let value = self.parse_value()?;
            pairs.push((key, value));
            self.skip_ws();
            match self.chars.next() {
                Some(',') => continue,
                Some('}') => break,
                other => return Err(format!("expected ',' or '}}', found {:?}", other)),
            }
        }
        Ok(JsonValue::Object(pairs))
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.chars.peek() == Some(&']') {
            self.chars.next();
            return Ok(JsonValue::Array(items));
        }
        loop {
            let value = self.parse_value()?;
            items.push(value);
            self.skip_ws();
            match self.chars.next() {
                Some(',') => continue,
                Some(']') => break,
                other => return Err(format!("expected ',' or ']', found {:?}", other)),
            }
        }
        Ok(JsonValue::Array(items))
    }

    fn consume_literal(&mut self, literal: &str) -> bool {
        for expected in literal.chars() {
            if self.chars.peek() != Some(&expected) {
                return false;
            }
            self.chars.next();
        }
        true
    }

    fn parse_bool(&mut self) -> Result<JsonValue, String> {
        if self.consume_literal("true") {
            Ok(JsonValue::Bool(true))
        } else if self.consume_literal("false") {
            Ok(JsonValue::Bool(false))
        } else {
            Err("invalid literal".to_string())
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, String> {
        if self.consume_literal("null") {
            Ok(JsonValue::Null)
        } else {
            Err("invalid literal".to_string())
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let mut raw = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' {
                raw.push(c);
                self.chars.next();
            } else {
                break;
            }
        }
        raw.parse::<f64>().map(JsonValue::Number).map_err(|e| e.to_string())
    }
}

pub fn parse_json(input: &str) -> Result<JsonValue, String> {
    JsonParser::new(input).parse_value()
}

// Fields our model reads explicitly; anything else on a feed or item object
// is kept in `extensions` and written back out verbatim rather than dropped.
const KNOWN_FEED_FIELDS: [&str; 5] = ["version", "title", "home_page_url", "description", "items"];
const KNOWN_ITEM_FIELDS: [&str; 7] = [
    "id",
    "url",
    "title",
    "content_text",
    "content_html",
    "summary",
    "date_published",
];

fn parse_json_feed_item(entry: &JsonValue) -> Item {
    let content = entry
        .get("content_text")
        .or_else(|| entry.get("content_html"))
        .or_else(|| entry.get("summary"))
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_string();
    let enclosures = entry
        .get("attachments")
        .and_then(JsonValue::as_array)
        .map(|attachments| {
            attachments
                .iter()
                .filter_map(|a| {
                    let url = a.get("url").and_then(JsonValue::as_str)?.to_string();
                    let mime_type = a
                        .get("mime_type")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("")
                        .to_string();
                    let length = a.get("size_in_bytes").and_then(JsonValue::as_u64);
                    Some(Enclosure { url, mime_type, length })
                })
                .collect()
        })
        .unwrap_or_default();
    let extensions = entry
        .as_object()
        .map(|pairs| {
            pairs
                .iter()
                .filter(|(k, _)| !KNOWN_ITEM_FIELDS.contains(&k.as_str()) && k != "attachments")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    Item {
        id: entry.get("id").and_then(JsonValue::as_str).unwrap_or("").to_string(),
        title: entry.get("title").and_then(JsonValue::as_str).unwrap_or("").to_string(),
        link: entry.get("url").and_then(JsonValue::as_str).unwrap_or("").to_string(),
        content,
        pub_date: entry
            .get("date_published")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_string(),
        enclosures,
        extensions,
        xml_extensions: Vec::new(),
    }
}

fn field<'a>(pairs: &'a [(String, JsonValue)], key: &str) -> Option<&'a JsonValue> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

pub fn parse_json_feed(input: &str) -> Result<Feed, String> {
    let pairs = match parse_json(input)? {
        JsonValue::Object(pairs) => pairs,
        _ => return Err("top-level JSON Feed value must be an object".to_string()),
    };

    let mut items = Vec::new();
    if let Some(raw_items) = field(&pairs, "items").and_then(JsonValue::as_array) {
        for entry in raw_items {
            items.push(parse_json_feed_item(entry));
        }
    }

    let title = field(&pairs, "title").and_then(JsonValue::as_str).unwrap_or("").to_string();
    let link = field(&pairs, "home_page_url")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_string();
    let description = field(&pairs, "description")
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_string();

    let extensions = pairs
        .into_iter()
        .filter(|(k, _)| !KNOWN_FEED_FIELDS.contains(&k.as_str()))
        .collect();

    Ok(Feed {
        title,
        link,
        description,
        items,
        extensions,
        xml_extensions: Vec::new(),
        xml_namespaces: Vec::new(),
    })
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn write_json_value(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => format!("\"{}\"", json_escape(s)),
        JsonValue::Array(items) => {
            let parts: Vec<String> = items.iter().map(write_json_value).collect();
            format!("[{}]", parts.join(","))
        }
        JsonValue::Object(pairs) => {
            let parts: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", json_escape(k), write_json_value(v)))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

pub fn write_json_feed(feed: &Feed) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"version\": \"https://jsonfeed.org/version/1.1\",\n");
    out.push_str(&format!("  \"title\": \"{}\",\n", json_escape(&feed.title)));
    out.push_str(&format!("  \"home_page_url\": \"{}\",\n", json_escape(&feed.link)));
    out.push_str(&format!(
        "  \"description\": \"{}\",\n",
        json_escape(&feed.description)
    ));
    for (key, value) in &feed.extensions {
        out.push_str(&format!("  \"{}\": {},\n", json_escape(key), write_json_value(value)));
    }
    out.push_str("  \"items\": [\n");
    for (i, item) in feed.items.iter().enumerate() {
        let id = if item.id.is_empty() { &item.link } else { &item.id };
        out.push_str("    {\n");
        out.push_str(&format!("      \"id\": \"{}\",\n", json_escape(id)));
        out.push_str(&format!("      \"url\": \"{}\",\n", json_escape(&item.link)));
        out.push_str(&format!("      \"title\": \"{}\",\n", json_escape(&item.title)));
        out.push_str(&format!(
            "      \"content_text\": \"{}\"",
            json_escape(&item.content)
        ));
        if !item.pub_date.is_empty() {
            out.push_str(",\n");
            out.push_str(&format!(
                "      \"date_published\": \"{}\"",
                json_escape(&crate::date::to_iso8601(&item.pub_date))
            ));
        }
        if !item.enclosures.is_empty() {
            out.push_str(",\n      \"attachments\": [\n");
            for (j, enclosure) in item.enclosures.iter().enumerate() {
                out.push_str("        {\n");
                out.push_str(&format!("          \"url\": \"{}\",\n", json_escape(&enclosure.url)));
                out.push_str(&format!(
                    "          \"mime_type\": \"{}\"",
                    json_escape(&enclosure.mime_type)
                ));
                if let Some(length) = enclosure.length {
                    out.push_str(&format!(",\n          \"size_in_bytes\": {}\n", length));
                } else {
                    out.push('\n');
                }
                out.push_str("        }");
                if j + 1 < item.enclosures.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("      ]");
        }
        for (key, value) in &item.extensions {
            out.push_str(&format!(
                ",\n      \"{}\": {}",
                json_escape(key),
                write_json_value(value)
            ));
        }
        out.push('\n');
        out.push_str("    }");
        if i + 1 < feed.items.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_entities_handles_named_and_numeric() {
        assert_eq!(decode_entities("&amp;&lt;&gt;&quot;&apos;"), "&<>\"'");
        assert_eq!(decode_entities("&#65;&#x41;"), "AA");
    }

    #[test]
    fn decode_entities_passes_through_unknown_and_unterminated() {
        assert_eq!(decode_entities("&unknown;"), "&unknown;");
        assert_eq!(decode_entities("100% & rising"), "100% & rising");
    }

    #[test]
    fn strip_cdata_keeps_markup_raw() {
        assert_eq!(strip_cdata("  <![CDATA[<b>bold</b>]]>  "), "<b>bold</b>");
        assert_eq!(strip_cdata("hello &amp; world"), "hello & world");
    }

    #[test]
    fn extract_tag_reads_simple_content() {
        assert_eq!(
            extract_tag("<title>Hello</title>", "title"),
            Some("Hello".to_string())
        );
    }

    #[test]
    fn extract_tag_treats_self_closing_as_empty() {
        assert_eq!(extract_tag("<link/>", "link"), Some(String::new()));
    }

    #[test]
    fn extract_tag_does_not_match_longer_tag_names() {
        // A naive substring search would stop at <titles>; the boundary
        // check must skip it and find the real <title> that follows.
        let xml = "<titles>wrong</titles><title>right</title>";
        assert_eq!(extract_tag(xml, "title"), Some("right".to_string()));
    }

    #[test]
    fn extract_all_tag_finds_every_block() {
        let xml = "<item>a</item><item>b</item>";
        assert_eq!(extract_all_tag(xml, "item"), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn extract_attr_value_reads_both_quote_styles() {
        let attrs = " href=\"a\" rel='self'";
        assert_eq!(extract_attr_value(attrs, "href"), Some("a".to_string()));
        assert_eq!(extract_attr_value(attrs, "rel"), Some("self".to_string()));
    }

    #[test]
    fn extract_attr_value_ignores_attribute_name_substrings() {
        // "xhref" contains "href" but is a different attribute entirely.
        assert_eq!(extract_attr_value(" xhref=\"a\"", "href"), None);
    }

    #[test]
    fn atom_link_href_prefers_alternate_over_other_rels() {
        let xml = "<link rel=\"self\" href=\"http://self\"/><link href=\"http://alt\"/>";
        assert_eq!(atom_link_href(xml), "http://alt");
    }

    #[test]
    fn atom_link_href_falls_back_when_no_alternate_present() {
        let xml = "<link rel=\"self\" href=\"http://self\"/>";
        assert_eq!(atom_link_href(xml), "http://self");
    }

    #[test]
    fn atom_text_construct_unwraps_xhtml_div() {
        let entry = r#"<entry><content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>Hello <b>world</b></p></div></content></entry>"#;
        assert_eq!(
            atom_text_construct(entry, "content"),
            Some("<p>Hello <b>world</b></p>".to_string())
        );
    }

    #[test]
    fn atom_text_construct_decodes_html_type_content() {
        let entry = r#"<entry><content type="html">&lt;p&gt;Hi&lt;/p&gt;</content></entry>"#;
        assert_eq!(atom_text_construct(entry, "content"), Some("<p>Hi</p>".to_string()));
    }

    #[test]
    fn parse_atom_reads_xhtml_entry_content() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>Example</title>
<entry>
<title>Post</title>
<content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>Hello <b>world</b></p></div></content>
</entry>
</feed>"#;
        let feed = parse_atom(xml).unwrap();
        assert_eq!(feed.items[0].content, "<p>Hello <b>world</b></p>");
    }

    #[test]
    fn xml_root_tag_skips_declaration_and_comments() {
        let xml = "<?xml version=\"1.0\"?><!-- note --><rss version=\"2.0\"><channel/></rss>";
        assert_eq!(xml_root_tag(xml), Some("rss".to_string()));
    }

    #[test]
    fn xml_root_tag_detects_atom_feed() {
        let xml = "<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\"></feed>";
        assert_eq!(xml_root_tag(xml), Some("feed".to_string()));
    }

    #[test]
    fn top_level_elements_matches_by_tag_name_not_nesting() {
        let xml = "<a>x</a><media:content url=\"http://x\"/><b><a>nested</a></b>";
        let names: Vec<String> = top_level_elements(xml).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["a".to_string(), "media:content".to_string(), "b".to_string()]);
    }

    #[test]
    fn xml_extensions_keeps_only_namespaced_top_level_tags() {
        let xml = r#"<title>t</title><media:content url="http://x/y.mp3"/><dc:creator>Jane</dc:creator>"#;
        let extensions = xml_extensions(xml);
        assert_eq!(
            extensions,
            vec![
                "<media:content url=\"http://x/y.mp3\"/>".to_string(),
                "<dc:creator>Jane</dc:creator>".to_string(),
            ]
        );
    }

    #[test]
    fn extract_xmlns_decls_reads_prefixed_declarations_only() {
        let attrs = " version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"";
        let decls = extract_xmlns_decls(attrs);
        assert_eq!(
            decls,
            vec![
                ("atom".to_string(), "http://www.w3.org/2005/Atom".to_string()),
                ("dc".to_string(), "http://purl.org/dc/elements/1.1/".to_string()),
            ]
        );
    }

    #[test]
    fn parse_rss_keeps_namespaced_extensions_and_declarations() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
<channel>
<title>Example</title>
<link>http://example.com</link>
<description>desc</description>
<atom:link href="http://example.com/feed" rel="self"/>
<item>
<title>Post</title>
<dc:creator>Jane</dc:creator>
</item>
</channel>
</rss>"#;
        let feed = parse_rss(xml).unwrap();
        assert_eq!(
            feed.xml_namespaces,
            vec![("dc".to_string(), "http://purl.org/dc/elements/1.1/".to_string())]
        );
        assert_eq!(feed.xml_extensions, vec!["<atom:link href=\"http://example.com/feed\" rel=\"self\"/>".to_string()]);
        assert_eq!(feed.items[0].xml_extensions, vec!["<dc:creator>Jane</dc:creator>".to_string()]);
    }

    #[test]
    fn write_rss_round_trips_namespaced_extensions() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
<channel>
<title>Example</title>
<link>http://example.com</link>
<description>desc</description>
<item>
<title>Post</title>
<dc:creator>Jane</dc:creator>
</item>
</channel>
</rss>"#;
        let feed = parse_rss(xml).unwrap();
        let output = write_rss(&feed);
        assert!(output.contains("xmlns:dc=\"http://purl.org/dc/elements/1.1/\""));
        assert!(output.contains("<dc:creator>Jane</dc:creator>"));
        let reparsed = parse_rss(&output).unwrap();
        assert_eq!(reparsed.items[0].xml_extensions, vec!["<dc:creator>Jane</dc:creator>".to_string()]);
    }

    #[test]
    fn parse_enclosures_rss_skips_entries_missing_url() {
        let xml = r#"<enclosure url="http://a/mp3" type="audio/mpeg" length="100"/><enclosure type="audio/mpeg"/>"#;
        let enclosures = parse_enclosures_rss(xml);
        assert_eq!(enclosures.len(), 1);
        assert_eq!(enclosures[0].url, "http://a/mp3");
        assert_eq!(enclosures[0].mime_type, "audio/mpeg");
        assert_eq!(enclosures[0].length, Some(100));
    }

    #[test]
    fn escape_xml_escapes_reserved_characters_only() {
        assert_eq!(escape_xml("a < b & c > d \"e\""), "a &lt; b &amp; c &gt; d \"e\"");
    }

    #[test]
    fn escape_xml_attr_also_escapes_quotes() {
        assert_eq!(escape_xml_attr("a \"quoted\" & b"), "a &quot;quoted&quot; &amp; b");
    }

    #[test]
    fn parse_json_reads_nested_structures() {
        let value = parse_json(r#"{"a": 1, "b": [true, false, null], "c": {"d": "x\ny"}}"#).unwrap();
        assert_eq!(value.get("a").and_then(JsonValue::as_u64), Some(1));
        let b = value.get("b").and_then(JsonValue::as_array).unwrap();
        assert_eq!(b.len(), 3);
        assert!(matches!(b[0], JsonValue::Bool(true)));
        assert!(matches!(b[1], JsonValue::Bool(false)));
        assert!(matches!(b[2], JsonValue::Null));
        let d = value.get("c").and_then(|c| c.get("d")).and_then(JsonValue::as_str);
        assert_eq!(d, Some("x\ny"));
    }

    #[test]
    fn parse_json_reads_unicode_escapes() {
        let value = parse_json("\"A\\u00e9\"").unwrap();
        assert_eq!(value.as_str(), Some("A\u{e9}"));
    }

    #[test]
    fn json_escape_escapes_control_characters() {
        assert_eq!(json_escape("a\nb\tc\"d\\e"), "a\\nb\\tc\\\"d\\\\e");
        assert_eq!(json_escape("\u{1}"), "\\u0001");
    }

    #[test]
    fn parse_json_feed_reads_items_and_attachments() {
        let json = r#"{
            "title": "Example",
            "home_page_url": "http://example.com",
            "items": [
                {
                    "id": "1",
                    "url": "http://example.com/1",
                    "title": "First",
                    "content_text": "hello",
                    "date_published": "2020-01-02T03:04:05Z",
                    "attachments": [
                        {"url": "http://example.com/a.mp3", "mime_type": "audio/mpeg", "size_in_bytes": 42}
                    ]
                }
            ]
        }"#;
        let feed = parse_json_feed(json).unwrap();
        assert_eq!(feed.title, "Example");
        assert_eq!(feed.link, "http://example.com");
        assert_eq!(feed.items.len(), 1);
        let item = &feed.items[0];
        assert_eq!(item.id, "1");
        assert_eq!(item.content, "hello");
        assert_eq!(item.enclosures.len(), 1);
        assert_eq!(item.enclosures[0].length, Some(42));
    }

    #[test]
    fn parse_json_feed_keeps_unknown_feed_and_item_fields_as_extensions() {
        let json = r#"{
            "title": "Example",
            "language": "en-US",
            "_custom_feed": {"a": 1},
            "items": [
                {
                    "id": "1",
                    "content_text": "hello",
                    "_custom_item": [true, "x"]
                }
            ]
        }"#;
        let feed = parse_json_feed(json).unwrap();
        assert_eq!(feed.extensions.len(), 2);
        assert!(feed.extensions.iter().any(|(k, v)| k == "language" && v.as_str() == Some("en-US")));
        assert!(feed.extensions.iter().any(|(k, _)| k == "_custom_feed"));
        let item = &feed.items[0];
        assert_eq!(item.extensions.len(), 1);
        assert_eq!(item.extensions[0].0, "_custom_item");
    }

    #[test]
    fn write_json_feed_round_trips_extensions() {
        let json = r#"{
            "title": "Example",
            "language": "en-US",
            "items": [
                {"id": "1", "content_text": "hi", "_custom_item": 7}
            ]
        }"#;
        let feed = parse_json_feed(json).unwrap();
        let output = write_json_feed(&feed);
        assert!(output.contains("\"language\": \"en-US\""));
        assert!(output.contains("\"_custom_item\": 7"));
        // Extensions must round-trip through a second parse too.
        let reparsed = parse_json_feed(&output).unwrap();
        assert_eq!(reparsed.extensions.len(), 1);
        assert_eq!(reparsed.items[0].extensions.len(), 1);
    }
}
