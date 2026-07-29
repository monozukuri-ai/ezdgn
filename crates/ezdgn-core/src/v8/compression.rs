use flate2::{Decompress, FlushDecompress, Status};

use crate::DgnError;

/// Inflate one zlib member while enforcing a hard output bound.
pub(crate) fn inflate_zlib_bounded(
    path: &str,
    bytes: &[u8],
    offset: usize,
    limit: usize,
) -> Result<Vec<u8>, DgnError> {
    let compressed = bytes
        .get(offset..)
        .ok_or_else(|| DgnError::InvalidV8CompressedStream {
            path: path.to_owned(),
            context: format!(
                "zlib offset {offset} is beyond the {}-byte stream",
                bytes.len()
            ),
        })?;
    if compressed.is_empty() {
        return Err(DgnError::InvalidV8CompressedStream {
            path: path.to_owned(),
            context: "missing zlib payload".to_owned(),
        });
    }

    let mut decoder = Decompress::new(true);
    let mut inflated = Vec::new();
    let mut input_offset = 0usize;
    let mut chunk = [0_u8; 8192];
    loop {
        // Leave room for one byte beyond the configured limit so expansion is
        // rejected without allocating an attacker-controlled output buffer.
        let output_capacity = limit
            .saturating_sub(inflated.len())
            .saturating_add(1)
            .min(chunk.len())
            .max(1);
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                &compressed[input_offset..],
                &mut chunk[..output_capacity],
                FlushDecompress::None,
            )
            .map_err(|error| DgnError::InvalidV8CompressedStream {
                path: path.to_owned(),
                context: error.to_string(),
            })?;
        let consumed = usize::try_from(decoder.total_in() - before_in).unwrap_or(usize::MAX);
        let produced = usize::try_from(decoder.total_out() - before_out).unwrap_or(usize::MAX);
        input_offset = input_offset.saturating_add(consumed);
        if produced > limit.saturating_sub(inflated.len()) {
            return Err(DgnError::V8InflatedSizeLimitExceeded {
                path: path.to_owned(),
                limit,
            });
        }
        inflated.extend_from_slice(&chunk[..produced]);

        if status == Status::StreamEnd {
            if input_offset != compressed.len() {
                return Err(DgnError::InvalidV8CompressedStream {
                    path: path.to_owned(),
                    context: format!(
                        "zlib member consumed {input_offset} of {} compressed bytes",
                        compressed.len()
                    ),
                });
            }
            return Ok(inflated);
        }
        if consumed == 0 && produced == 0 {
            return Err(DgnError::InvalidV8CompressedStream {
                path: path.to_owned(),
                context: "zlib payload ended before the stream checksum".to_owned(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{write::ZlibEncoder, Compression};

    use super::*;

    fn compressed(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn inflates_exactly_one_bounded_member() {
        let mut bytes = vec![1, 2, 3, 4];
        bytes.extend(compressed(b"hello"));
        assert_eq!(
            inflate_zlib_bounded("/test", &bytes, 4, 5).unwrap(),
            b"hello"
        );
        assert!(matches!(
            inflate_zlib_bounded("/test", &bytes, 4, 4),
            Err(DgnError::V8InflatedSizeLimitExceeded { .. })
        ));

        bytes.push(0);
        assert!(matches!(
            inflate_zlib_bounded("/test", &bytes, 4, 8),
            Err(DgnError::InvalidV8CompressedStream { .. })
        ));

        let truncated = &bytes[..bytes.len() - 2];
        assert!(matches!(
            inflate_zlib_bounded("/test", truncated, 4, 8),
            Err(DgnError::InvalidV8CompressedStream { .. })
        ));
    }
}
