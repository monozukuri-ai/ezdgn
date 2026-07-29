mod support;

use std::env;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use ezdgn_core::{V8CfbEntryKind, V8Stream};
use flate2::read::ZlibDecoder;

use support::{load_streams, sha256_hex};

const DEFAULT_MAX_BYTES: usize = 4096;
const DEFAULT_MAX_INFLATED_BYTES: usize = 64 * 1024 * 1024;
const MIN_TEXT_CHARS: usize = 4;
const MAX_TEXT_CANDIDATES: usize = 200;
const MAX_FIND_MATCHES: usize = 100;
const MAX_ZLIB_CANDIDATES: usize = 1024;

#[derive(Debug)]
struct Args {
    path: PathBuf,
    stream: Option<String>,
    max_bytes: usize,
    inflate: bool,
    text_only: bool,
    max_inflated_bytes: usize,
    find: Vec<String>,
}

#[derive(Debug)]
struct TextCandidate {
    offset: usize,
    encoding: &'static str,
    value: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let stream_set = load_streams(&args.path)?;
    let storage_count = stream_set
        .container
        .entries
        .iter()
        .filter(|entry| entry.kind == V8CfbEntryKind::Storage)
        .count();

    println!("file={}", args.path.display());
    println!("cfb_version={}", stream_set.container.cfb_version);
    println!("entry_count={}", stream_set.container.entries.len());
    println!("storage_count={storage_count}");
    println!("stream_count={}", stream_set.streams.len());
    println!("total_stream_bytes={}", stream_set.total_size());
    let model_storages = stream_set
        .container
        .model_storage_paths
        .iter()
        .map(|path| path.escape_default().to_string())
        .collect::<Vec<_>>()
        .join(",");
    println!("model_storages={model_storages}");
    println!("\nsize\tsha256\tprefix\tformat_hint\tpath");

    let mut streams = stream_set.streams.iter().collect::<Vec<_>>();
    streams.sort_by(|left, right| left.path.cmp(&right.path));
    for stream in &streams {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            stream.len(),
            sha256_hex(stream.as_bytes()),
            hex_prefix(stream.as_bytes(), 8),
            zlib_header_hint(stream.as_bytes()),
            stream.path.escape_default()
        );
    }

    if let Some(selected_path) = args.stream.as_deref() {
        let selected = stream_set.get(selected_path).ok_or_else(|| {
            format!(
                "stream {selected_path:?} was not found; use the inventory above for valid paths"
            )
        })?;
        print_selected_stream(
            selected,
            args.max_bytes,
            args.inflate,
            args.text_only,
            args.max_inflated_bytes,
            &args.find,
        )?;
    } else if args.inflate {
        return Err("--inflate requires --stream".to_owned());
    }

    Ok(())
}

fn parse_args() -> Result<Args, String> {
    let mut path = None;
    let mut stream = None;
    let mut max_bytes = DEFAULT_MAX_BYTES;
    let mut inflate = false;
    let mut text_only = false;
    let mut max_inflated_bytes = DEFAULT_MAX_INFLATED_BYTES;
    let mut find = Vec::new();
    let mut arguments = env::args().skip(1);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Err(usage()),
            "--stream" => {
                stream = Some(
                    arguments
                        .next()
                        .ok_or_else(|| "--stream requires a CFB path".to_owned())?,
                );
            }
            "--max-bytes" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--max-bytes requires an integer".to_owned())?;
                max_bytes = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --max-bytes value {value:?}"))?;
            }
            "--inflate" => inflate = true,
            "--text-only" => text_only = true,
            "--find" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--find requires a non-empty string".to_owned())?;
                if value.is_empty() {
                    return Err("--find requires a non-empty string".to_owned());
                }
                find.push(value);
            }
            "--max-inflated-bytes" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--max-inflated-bytes requires an integer".to_owned())?;
                max_inflated_bytes = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --max-inflated-bytes value {value:?}"))?;
            }
            _ if argument.starts_with('-') => {
                return Err(format!("unknown option {argument:?}\n{}", usage()));
            }
            _ if path.is_some() => {
                return Err(format!("unexpected argument {argument:?}\n{}", usage()));
            }
            _ => path = Some(PathBuf::from(argument)),
        }
    }

    Ok(Args {
        path: path.ok_or_else(usage)?,
        stream,
        max_bytes,
        inflate,
        text_only,
        max_inflated_bytes,
        find,
    })
}

fn usage() -> String {
    format!(
        "usage: v8_stream_dump FILE [--stream /CFB/path] [--max-bytes N] \
         [--inflate] [--text-only] [--find TEXT] [--max-inflated-bytes N]\n\
         --stream prints a bounded hex/text preview (default {DEFAULT_MAX_BYTES} bytes)\n\
         --inflate explicitly probes a selected zlib payload (default output limit \
         {DEFAULT_MAX_INFLATED_BYTES} bytes)"
    )
}

fn print_selected_stream(
    stream: &V8Stream,
    max_bytes: usize,
    inflate: bool,
    text_only: bool,
    max_inflated_bytes: usize,
    find: &[String],
) -> Result<(), String> {
    println!("\nselected_stream={}", stream.path.escape_default());
    println!("selected_stream_bytes={}", stream.len());
    println!("selected_stream_sha256={}", sha256_hex(stream.as_bytes()));
    print_payload("raw", stream.as_bytes(), max_bytes, text_only, find);

    if inflate {
        let (inflated_offset, inflated) =
            inflate_zlib_bounded(stream.as_bytes(), max_inflated_bytes)?;
        println!("\ninflated_offset=0x{inflated_offset:08x}");
        println!("inflated_bytes={}", inflated.len());
        println!("inflated_sha256={}", sha256_hex(&inflated));
        print_payload("inflated", &inflated, max_bytes, text_only, find);
    }
    Ok(())
}

fn print_payload(label: &str, bytes: &[u8], max_bytes: usize, text_only: bool, find: &[String]) {
    let inspected_len = bytes.len().min(max_bytes);
    let inspected = &bytes[..inspected_len];
    println!("{label}_inspected_bytes={inspected_len}");
    if !text_only {
        println!("\n{label}_hexdump:");
        print_hexdump(inspected);
    }
    print_findings(label, inspected, find);
    println!("\n{label}_text_candidates:");

    let mut candidates = utf8_candidates(inspected);
    candidates.extend(utf16_candidates(inspected, true));
    candidates.extend(utf16_candidates(inspected, false));
    candidates.sort_by_key(|candidate| (candidate.offset, candidate.encoding));
    if candidates.is_empty() {
        println!("(none)");
        return;
    }

    let candidate_count = candidates.len();
    for candidate in candidates.into_iter().take(MAX_TEXT_CANDIDATES) {
        println!(
            "0x{:08x}\t{}\t{:?}",
            candidate.offset, candidate.encoding, candidate.value
        );
    }
    if candidate_count > MAX_TEXT_CANDIDATES {
        println!(
            "... {} additional candidates omitted",
            candidate_count - MAX_TEXT_CANDIDATES
        );
    }
}

fn print_findings(label: &str, bytes: &[u8], queries: &[String]) {
    if queries.is_empty() {
        return;
    }
    println!("\n{label}_findings:");
    for query in queries {
        let utf16le = query
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let utf16be = query
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let encodings = [
            ("utf-8", query.as_bytes()),
            ("utf-16le", utf16le.as_slice()),
            ("utf-16be", utf16be.as_slice()),
        ];
        let mut matches = 0;
        let mut truncated = false;
        'encodings: for (encoding, pattern) in encodings {
            for offset in find_offsets(bytes, pattern) {
                if matches == MAX_FIND_MATCHES {
                    truncated = true;
                    break 'encodings;
                }
                println!("{query:?}\t{encoding}\t0x{offset:08x}");
                matches += 1;
            }
        }
        if matches == 0 {
            println!("{query:?}\tnot-found\t-");
        } else if truncated {
            println!("{query:?}\t... additional matches omitted after {MAX_FIND_MATCHES} hits");
        }
    }
}

fn find_offsets<'a>(bytes: &'a [u8], pattern: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    bytes
        .windows(pattern.len())
        .enumerate()
        .filter_map(move |(offset, window)| (window == pattern).then_some(offset))
}

fn hex_prefix(bytes: &[u8], max_bytes: usize) -> String {
    bytes
        .iter()
        .take(max_bytes)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn zlib_header_hint(bytes: &[u8]) -> String {
    zlib_header_offsets(bytes).next().map_or_else(
        || "-".to_owned(),
        |offset| format!("zlib-header@0x{offset:08x}"),
    )
}

fn zlib_header_offsets(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    (0..bytes.len().saturating_sub(1)).filter(|offset| has_zlib_header(&bytes[*offset..]))
}

fn has_zlib_header(bytes: &[u8]) -> bool {
    let Some((&compression, rest)) = bytes.split_first() else {
        return false;
    };
    let Some(&flags) = rest.first() else {
        return false;
    };
    compression & 0x0f == 8
        && compression >> 4 <= 7
        && (u16::from(compression) * 256 + u16::from(flags)) % 31 == 0
}

fn inflate_zlib_bounded(bytes: &[u8], limit: usize) -> Result<(usize, Vec<u8>), String> {
    let offsets = zlib_header_offsets(bytes)
        .take(MAX_ZLIB_CANDIDATES + 1)
        .collect::<Vec<_>>();
    if offsets.len() > MAX_ZLIB_CANDIDATES {
        return Err(format!(
            "selected stream contains more than {MAX_ZLIB_CANDIDATES} zlib header candidates"
        ));
    }
    if offsets.is_empty() {
        return Err("selected stream does not contain a valid zlib header".to_owned());
    }

    let read_limit = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut last_error = None;
    for offset in offsets.iter().copied() {
        let mut decoder = ZlibDecoder::new(&bytes[offset..]).take(read_limit);
        let mut inflated = Vec::new();
        match decoder.read_to_end(&mut inflated) {
            Ok(_) if inflated.len() <= limit => return Ok((offset, inflated)),
            Ok(_) => {
                return Err(format!(
                    "inflated data exceeds configured limit {limit} bytes"
                ));
            }
            Err(error) => last_error = Some(error),
        }
    }

    Err(format!(
        "none of {} zlib header candidates could be inflated: {}",
        offsets.len(),
        last_error.map_or_else(|| "unknown error".to_owned(), |error| error.to_string())
    ))
}

fn print_hexdump(bytes: &[u8]) {
    for (line_index, chunk) in bytes.chunks(16).enumerate() {
        let offset = line_index * 16;
        print!("{offset:08x}  ");
        for index in 0..16 {
            if let Some(byte) = chunk.get(index) {
                print!("{byte:02x} ");
            } else {
                print!("   ");
            }
            if index == 7 {
                print!(" ");
            }
        }
        print!(" |");
        for byte in chunk {
            let character = if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '.'
            };
            print!("{character}");
        }
        println!("|");
    }
}

fn utf8_candidates(bytes: &[u8]) -> Vec<TextCandidate> {
    let mut candidates = Vec::new();
    let mut start = None;
    let mut value = String::new();
    let mut offset = 0;

    while offset < bytes.len() {
        let decoded = decode_utf8_at(bytes, offset).filter(|(character, _)| is_text(*character));
        if let Some((character, width)) = decoded {
            start.get_or_insert(offset);
            value.push(character);
            offset += width;
        } else {
            finish_candidate(&mut candidates, &mut start, &mut value, "utf-8");
            offset += 1;
        }
    }
    finish_candidate(&mut candidates, &mut start, &mut value, "utf-8");
    candidates
}

fn decode_utf8_at(bytes: &[u8], offset: usize) -> Option<(char, usize)> {
    let first = *bytes.get(offset)?;
    let width = match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let end = offset.checked_add(width)?;
    let decoded = std::str::from_utf8(bytes.get(offset..end)?).ok()?;
    Some((decoded.chars().next()?, width))
}

fn utf16_candidates(bytes: &[u8], little_endian: bool) -> Vec<TextCandidate> {
    let mut candidates = Vec::new();
    let encoding = if little_endian {
        "utf-16le"
    } else {
        "utf-16be"
    };

    for alignment in 0..2 {
        let mut start = None;
        let mut value = String::new();
        let mut offset = alignment;
        while offset + 1 < bytes.len() {
            let pair = [bytes[offset], bytes[offset + 1]];
            let code_unit = if little_endian {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            };
            let decoded =
                char::from_u32(u32::from(code_unit)).filter(|character| is_text(*character));
            if let Some(character) = decoded {
                start.get_or_insert(offset);
                value.push(character);
            } else {
                finish_candidate(&mut candidates, &mut start, &mut value, encoding);
            }
            offset += 2;
        }
        finish_candidate(&mut candidates, &mut start, &mut value, encoding);
    }
    candidates
}

fn is_text(character: char) -> bool {
    matches!(
        character,
        ' '..='~'
            | '\u{00a0}'..='\u{024f}'
            | '\u{0370}'..='\u{052f}'
            | '\u{2000}'..='\u{206f}'
            | '\u{3000}'..='\u{30ff}'
            | '\u{3400}'..='\u{9fff}'
            | '\u{ac00}'..='\u{d7af}'
            | '\u{ff00}'..='\u{ffef}'
    )
}

fn finish_candidate(
    candidates: &mut Vec<TextCandidate>,
    start: &mut Option<usize>,
    value: &mut String,
    encoding: &'static str,
) {
    if value.chars().count() >= MIN_TEXT_CHARS {
        candidates.push(TextCandidate {
            offset: start.unwrap_or_default(),
            encoding,
            value: std::mem::take(value),
        });
    } else {
        value.clear();
    }
    *start = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_inflates_an_embedded_zlib_payload_with_a_limit() {
        let bytes = [
            0xaa, 0xbb, 0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02,
            0x15,
        ];
        assert_eq!(zlib_header_offsets(&bytes).collect::<Vec<_>>(), [2]);
        assert_eq!(
            inflate_zlib_bounded(&bytes, 5).unwrap(),
            (2, b"hello".to_vec())
        );
        assert!(inflate_zlib_bounded(&bytes, 4).is_err());

        let too_many_candidates = [0x78, 0x9c].repeat(MAX_ZLIB_CANDIDATES + 1);
        assert!(inflate_zlib_bounded(&too_many_candidates, 5)
            .unwrap_err()
            .contains("more than"));
    }

    #[test]
    fn finds_unicode_queries_in_each_supported_encoding() {
        let query = "myTéxt";
        let utf16le = query
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            find_offsets(query.as_bytes(), query.as_bytes()).collect::<Vec<_>>(),
            [0]
        );
        assert_eq!(find_offsets(&utf16le, &utf16le).collect::<Vec<_>>(), [0]);
    }
}
