mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use support::{load_streams, sha256_hex};

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
    let (before_path, after_path) = parse_args()?;
    let before = load_streams(&before_path)?;
    let after = load_streams(&after_path)?;
    let before_streams = before
        .streams
        .iter()
        .map(|stream| (stream.path.as_str(), stream.as_bytes()))
        .collect::<BTreeMap<_, _>>();
    let after_streams = after
        .streams
        .iter()
        .map(|stream| (stream.path.as_str(), stream.as_bytes()))
        .collect::<BTreeMap<_, _>>();
    let paths = before_streams
        .keys()
        .chain(after_streams.keys())
        .copied()
        .collect::<BTreeSet<_>>();

    println!("before={}", before_path.display());
    println!("after={}", after_path.display());
    println!("before_stream_count={}", before_streams.len());
    println!("after_stream_count={}", after_streams.len());
    println!("before_total_stream_bytes={}", before.total_size());
    println!("after_total_stream_bytes={}", after.total_size());
    println!(
        "\nstatus\tbefore_size\tafter_size\tchanged_bytes\tfirst_difference\tbefore_sha256\tafter_sha256\tpath"
    );

    let mut changed_streams = 0;
    for path in paths {
        match (before_streams.get(path), after_streams.get(path)) {
            (Some(left), Some(right)) if left == right => println!(
                "same\t{}\t{}\t0\t-\t{}\t{}\t{}",
                left.len(),
                right.len(),
                sha256_hex(left),
                sha256_hex(right),
                path.escape_default()
            ),
            (Some(left), Some(right)) => {
                changed_streams += 1;
                let first = first_difference(left, right)
                    .map_or_else(|| "-".to_owned(), |offset| format!("0x{offset:08x}"));
                println!(
                    "changed\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    left.len(),
                    right.len(),
                    differing_byte_count(left, right),
                    first,
                    sha256_hex(left),
                    sha256_hex(right),
                    path.escape_default()
                );
            }
            (Some(left), None) => {
                changed_streams += 1;
                println!(
                    "removed\t{}\t-\t{}\t0x00000000\t{}\t-\t{}",
                    left.len(),
                    left.len(),
                    sha256_hex(left),
                    path.escape_default()
                );
            }
            (None, Some(right)) => {
                changed_streams += 1;
                println!(
                    "added\t-\t{}\t{}\t0x00000000\t-\t{}\t{}",
                    right.len(),
                    right.len(),
                    sha256_hex(right),
                    path.escape_default()
                );
            }
            (None, None) => unreachable!("path came from one of the two maps"),
        }
    }
    println!("\nchanged_stream_count={changed_streams}");
    Ok(())
}

fn parse_args() -> Result<(PathBuf, PathBuf), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .iter()
        .any(|argument| argument == "-h" || argument == "--help")
    {
        return Err(usage());
    }
    if arguments.len() != 2 {
        return Err(usage());
    }
    Ok((PathBuf::from(&arguments[0]), PathBuf::from(&arguments[1])))
}

fn usage() -> String {
    "usage: v8_diff BEFORE.dgn AFTER.dgn".to_owned()
}

fn first_difference(left: &[u8], right: &[u8]) -> Option<usize> {
    left.iter()
        .zip(right)
        .position(|(left_byte, right_byte)| left_byte != right_byte)
        .or_else(|| (left.len() != right.len()).then(|| left.len().min(right.len())))
}

fn differing_byte_count(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .filter(|(left_byte, right_byte)| left_byte != right_byte)
        .count()
        + left.len().abs_diff(right.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_content_and_length_differences() {
        assert_eq!(first_difference(b"same", b"same"), None);
        assert_eq!(differing_byte_count(b"same", b"same"), 0);
        assert_eq!(first_difference(b"abcd", b"abXd!"), Some(2));
        assert_eq!(differing_byte_count(b"abcd", b"abXd!"), 2);
        assert_eq!(first_difference(b"abc", b"abc!"), Some(3));
    }
}
