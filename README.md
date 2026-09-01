# rss-feed-converter

Most feed readers and static site generators only speak one of RSS 2.0,
Atom, or JSON Feed. Every time I've needed a different one I've ended up
hand-rolling a throwaway script. `feedconv` is that script, done properly: a
small command line tool that reads one format and writes another.

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

If `--to` is omitted, RSS and Atom input convert to JSON Feed, and JSON Feed
input converts to RSS. Input format is detected by sniffing the input
itself rather than by file extension - a `{...}` document is JSON Feed, and
an XML document is Atom if its root element is `<feed>` and RSS otherwise -
which is what makes stdin input work in the first place.

Build a standalone binary the normal way:

```
cargo build --release
./target/release/feedconv --to json < feed.xml > feed.json
```

## What gets converted

Only the fields all three formats agree on: feed title, link/home page
URL/alternate link, description/subtitle, and per-item title, link/URL,
description-or-content/summary, guid/id, and publish date. RSS uses
`pubDate`, JSON Feed uses `date_published`, and Atom uses `published` (with
`updated` as a fallback on read and mirrored on write) - all three date
formats get reparsed and reformatted for whichever format is being written;
a date that fails to parse is passed through unchanged instead of being
dropped.

Atom's `<link>` is attribute-based rather than text content, and a feed or
entry can carry several of them (`alternate`, `self`, `enclosure`, ...); the
converter picks the `rel="alternate"` one (or the first link with no `rel`
at all, per the Atom default) and ignores the rest. Writing Atom synthesizes
the `<id>` and `<updated>` elements the spec requires but the shared feed
model doesn't otherwise track, falling back to the link or title when
nothing better is available.

## Known limitations

- No support for enclosures, categories, or other extension fields - they're
  silently dropped on conversion.
- The XML reader is a small hand-written scanner built for the shape of RSS
  2.0 and Atom specifically, not a general-purpose XML parser. Atom
  `<content type="xhtml">` bodies (inline XML rather than escaped text)
  round-trip as raw markup rather than being reserialized.

## Roadmap

- Carry over enclosures / attachments.
- Unit tests for the XML and JSON parsing helpers.
- Preserve unknown extension fields instead of dropping them.

## License

MIT, see [LICENSE](LICENSE).
