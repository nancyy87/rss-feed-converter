mod feed;

use feed::Feed;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

#[derive(Clone, Copy)]
enum Format {
    Rss,
    JsonFeed,
}

fn detect_format(input: &str) -> Option<Format> {
    for c in input.chars() {
        if c.is_whitespace() {
            continue;
        }
        return match c {
            '<' => Some(Format::Rss),
            '{' => Some(Format::JsonFeed),
            _ => None,
        };
    }
    None
}

fn parse_format_flag(name: &str) -> Option<Format> {
    match name {
        "rss" => Some(Format::Rss),
        "json" | "jsonfeed" => Some(Format::JsonFeed),
        _ => None,
    }
}

fn read_input(path: Option<&str>) -> io::Result<String> {
    match path {
        None | Some("-") => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
        Some(path) => fs::read_to_string(path),
    }
}

fn print_usage() {
    eprintln!("feedconv - convert between RSS 2.0 and JSON Feed 1.1");
    eprintln!();
    eprintln!("usage:");
    eprintln!("  feedconv [--to rss|json] [FILE]");
    eprintln!();
    eprintln!("  FILE defaults to '-', meaning read from stdin.");
    eprintln!("  --to picks the output format; if omitted, it's the opposite of the detected input.");
}

fn run() -> Result<(), String> {
    let mut to_format: Option<Format> = None;
    let mut path: Option<String> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--to" => {
                let value = args.next().ok_or("--to requires a value (rss or json)")?;
                to_format = Some(
                    parse_format_flag(&value).ok_or_else(|| format!("unknown format '{}'", value))?,
                );
            }
            "-h" | "--help" => {
                print_usage();
                return Ok(());
            }
            other => {
                if path.is_some() {
                    return Err(format!("unexpected argument '{}'", other));
                }
                path = Some(other.to_string());
            }
        }
    }

    let input = read_input(path.as_deref()).map_err(|e| format!("failed to read input: {}", e))?;

    let source_format =
        detect_format(&input).ok_or("could not detect input format (expected XML or JSON)")?;

    let target_format = to_format.unwrap_or(match source_format {
        Format::Rss => Format::JsonFeed,
        Format::JsonFeed => Format::Rss,
    });

    let parsed: Feed = match source_format {
        Format::Rss => feed::parse_rss(&input)?,
        Format::JsonFeed => feed::parse_json_feed(&input)?,
    };

    let output = match target_format {
        Format::Rss => feed::write_rss(&parsed),
        Format::JsonFeed => feed::write_json_feed(&parsed),
    };

    io::stdout()
        .write_all(output.as_bytes())
        .map_err(|e| format!("failed to write output: {}", e))
}

fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("feedconv: {}", e);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
