use std::fmt;

use crate::DgnError;

const V7_2D_SIGNATURE: [u8; 4] = [0x08, 0x09, 0xfe, 0x02];
const V7_3D_SIGNATURE: [u8; 4] = [0xc8, 0x09, 0xfe, 0x02];
const CFB_SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// Dimensionality encoded by the first V7 TCB record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V7Dimension {
    Two,
    Three,
}

impl V7Dimension {
    /// Returns the conventional numeric dimension.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Two => 2,
            Self::Three => 3,
        }
    }
}

impl fmt::Display for V7Dimension {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}D", self.as_u8())
    }
}

/// File family identified from the leading bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DgnFormat {
    /// V7/ISFF sequential record stream.
    V7(V7Dimension),
    /// A Compound File Binary container. The signature alone cannot prove
    /// that the contained streams are DGN V8 streams, so this remains an
    /// explicit candidate classification until a V8 backend inspects them.
    V8Cfb,
}

impl DgnFormat {
    /// Stable short label used by language bindings.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::V7(_) => "V7",
            Self::V8Cfb => "V8_CFB",
        }
    }

    /// Returns the V7 dimension when the input is a V7 file.
    #[must_use]
    pub const fn dimension(self) -> Option<V7Dimension> {
        match self {
            Self::V7(dimension) => Some(dimension),
            Self::V8Cfb => None,
        }
    }
}

impl fmt::Display for DgnFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V7(dimension) => write!(formatter, "V7 DGN ({dimension})"),
            Self::V8Cfb => formatter.write_str("V8/CFB candidate"),
        }
    }
}

/// Identifies V7 2D, V7 3D, or a V8/CFB candidate from the file signature.
pub fn detect_format(input: &[u8]) -> Result<DgnFormat, DgnError> {
    if input.len() < V7_2D_SIGNATURE.len() {
        return Err(DgnError::InputTooShort {
            needed: V7_2D_SIGNATURE.len(),
            actual: input.len(),
        });
    }

    if input.starts_with(&V7_2D_SIGNATURE) {
        return Ok(DgnFormat::V7(V7Dimension::Two));
    }
    if input.starts_with(&V7_3D_SIGNATURE) {
        return Ok(DgnFormat::V7(V7Dimension::Three));
    }

    if input.len() < CFB_SIGNATURE.len() && CFB_SIGNATURE.starts_with(input) {
        return Err(DgnError::InputTooShort {
            needed: CFB_SIGNATURE.len(),
            actual: input.len(),
        });
    }
    if input.starts_with(&CFB_SIGNATURE) {
        return Ok(DgnFormat::V8Cfb);
    }

    Err(DgnError::UnrecognizedFormat {
        signature: format_signature(input),
    })
}

fn format_signature(input: &[u8]) -> String {
    input
        .iter()
        .take(CFB_SIGNATURE.len())
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_v7_dimensions() {
        assert_eq!(
            detect_format(&V7_2D_SIGNATURE),
            Ok(DgnFormat::V7(V7Dimension::Two))
        );
        assert_eq!(
            detect_format(&V7_3D_SIGNATURE),
            Ok(DgnFormat::V7(V7Dimension::Three))
        );
    }

    #[test]
    fn detects_cfb_as_v8_candidate() {
        assert_eq!(detect_format(&CFB_SIGNATURE), Ok(DgnFormat::V8Cfb));
    }

    #[test]
    fn partial_cfb_signature_reports_required_length() {
        assert!(matches!(
            detect_format(&CFB_SIGNATURE[..4]),
            Err(DgnError::InputTooShort {
                needed: 8,
                actual: 4
            })
        ));
    }

    #[test]
    fn rejects_unknown_signature() {
        assert!(matches!(
            detect_format(b"not a dgn"),
            Err(DgnError::UnrecognizedFormat { .. })
        ));
    }
}
