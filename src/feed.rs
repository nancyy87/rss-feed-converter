// Shared feed model plus hand-rolled XML and JSON handling. No third-party
// crates, so both formats get just enough of a parser to round-trip the
// fields RSS 2.0 and JSON Feed 1.1 actually share.

pub struct Feed {
    pub title: String,
    pub link: String,
    pub description: String,
    pub items: Vec<Item>,
}

pub struct Item {
    pub id: String,
    pub title: String,
    pub link: String,
    pub content: String,
    pub pub_date: String,
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

/// Finds the first `<tag ...>...</tag>` in `s` and returns its decoded inner text.
fn extract_tag(s: &str, tag: &str) -> Option<String> {
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
        return Some(strip_cdata(&s[content_start..content_end]));
    }
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

pub fn parse_rss(xml: &str) -> Result<Feed, String> {
    let channel = extract_tag(xml, "channel").ok_or("no <channel> element found")?;
    let mut feed = Feed {
        title: extract_tag(&channel, "title").unwrap_or_default(),
        link: extract_tag(&channel, "link").unwrap_or_default(),
        description: extract_tag(&channel, "description").unwrap_or_default(),
        items: Vec::new(),
    };
    for raw_item in extract_all_tag(&channel, "item") {
        feed.items.push(Item {
            id: extract_tag(&raw_item, "guid").unwrap_or_default(),
            title: extract_tag(&raw_item, "title").unwrap_or_default(),
            link: extract_tag(&raw_item, "link").unwrap_or_default(),
            content: extract_tag(&raw_item, "description").unwrap_or_default(),
            pub_date: extract_tag(&raw_item, "pubDate").unwrap_or_default(),
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

pub fn write_rss(feed: &Feed) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<rss version=\"2.0\">\n<channel>\n");
    out.push_str(&format!("<title>{}</title>\n", escape_xml(&feed.title)));
    out.push_str(&format!("<link>{}</link>\n", escape_xml(&feed.link)));
    out.push_str(&format!(
        "<description>{}</description>\n",
        escape_xml(&feed.description)
    ));
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
        out.push_str("</item>\n");
    }
    out.push_str("</channel>\n</rss>\n");
    out
}

// ---------- minimal JSON reading/writing ----------

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

pub fn parse_json_feed(input: &str) -> Result<Feed, String> {
    let value = parse_json(input)?;
    let mut feed = Feed {
        title: value.get("title").and_then(JsonValue::as_str).unwrap_or("").to_string(),
        link: value
            .get("home_page_url")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_string(),
        description: value
            .get("description")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_string(),
        items: Vec::new(),
    };
    if let Some(items) = value.get("items").and_then(JsonValue::as_array) {
        for entry in items {
            let content = entry
                .get("content_text")
                .or_else(|| entry.get("content_html"))
                .or_else(|| entry.get("summary"))
                .and_then(JsonValue::as_str)
                .unwrap_or("")
                .to_string();
            feed.items.push(Item {
                id: entry.get("id").and_then(JsonValue::as_str).unwrap_or("").to_string(),
                title: entry.get("title").and_then(JsonValue::as_str).unwrap_or("").to_string(),
                link: entry.get("url").and_then(JsonValue::as_str).unwrap_or("").to_string(),
                content,
                pub_date: entry
                    .get("date_published")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }
    Ok(feed)
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
                "      \"date_published\": \"{}\"\n",
                json_escape(&crate::date::to_iso8601(&item.pub_date))
            ));
        } else {
            out.push('\n');
        }
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
