# rss-feed-converter

Most feed readers and static site generators only speak one of RSS 2.0 or
JSON Feed. Every time I've needed the other one I've ended up hand-rolling a
throwaway script. `feedconv` is that script, done properly: a small command
line tool that reads one format and writes the other.

It has zero dependencies. Everything - XML reading, JSON reading, JSON
writing - is written against the standard library only, so there's nothing
to `cargo add` and no lockfile to think about.

## Usage

Convert an RSS file to JSON Feed and print it to stdout:

```
cargo run -- path/to/feed.xml
```

Convert the other direction, forcing the target format explicitly:

```
cargo run -- --to rss path/to/feed.json
```

Read from stdin (this is the point of the tool - most real feeds arrive over
a pipe, not as a file on disk):

```
curl -s https://example.com/feed.xml | cargo run -- --to json > feed.json
```

No file argument, or `-` as the file argument, both mean "read stdin":

```
cat feed.json | cargo run --
```

If `--to` is omitted, the output format is whichever one the input isn't:
feed a `<...>` XML document and you get JSON Feed back, feed a `{...}` JSON
document and you get RSS back. Input format is detected by sniffing the
first non-whitespace character, not by file extension, which is what makes
stdin input work in the first place.

Build a standalone binary the normal way:

```
cargo build --release
./target/release/feedconv --to json < feed.xml > feed.json
```

## What gets converted

Only the fields both formats agree on: feed title, link/home page URL,
description, and per-item title, link/URL, description/content, guid/id,
and publish date. `pubDate` and `date_published` use different date formats
(RFC 822 vs ISO 8601), so the publish date is reparsed and reformatted for
whichever format it's being written to; a date that fails to parse is passed
through unchanged instead of being dropped.

## Known limitations

- No Atom support, only RSS 2.0 and JSON Feed 1.1.
- No support for enclosures, categories, or other extension fields - they're
  silently dropped on conversion.
- The XML reader is a small hand-written scanner built for the shape of RSS
  2.0 specifically, not a general-purpose XML parser.

## Roadmap

- Read and write Atom as a third format.
- Carry over enclosures / attachments.
- Unit tests for the XML and JSON parsing helpers.

## License

MIT, see [LICENSE](LICENSE).
