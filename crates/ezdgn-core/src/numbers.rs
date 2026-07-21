//! Numeric encodings used by V7/ISFF records.

/// Decodes a 32-bit value stored in DGN's 16-bit-word-swapped byte order.
#[must_use]
pub const fn decode_middle_endian_u32(bytes: [u8; 4]) -> u32 {
    (bytes[2] as u32)
        | ((bytes[3] as u32) << 8)
        | ((bytes[0] as u32) << 16)
        | ((bytes[1] as u32) << 24)
}

/// Decodes a two's-complement signed 32-bit middle-endian value.
#[must_use]
pub const fn decode_middle_endian_i32(bytes: [u8; 4]) -> i32 {
    decode_middle_endian_u32(bytes) as i32
}

/// Decodes a middle-endian range coordinate stored in offset-binary form.
#[must_use]
pub const fn decode_offset_binary_i32(bytes: [u8; 4]) -> i32 {
    (decode_middle_endian_u32(bytes) ^ 0x8000_0000) as i32
}

/// Converts one VAX D-floating value to IEEE-754 `f64`.
///
/// VAX D-floating carries three more fraction bits than IEEE binary64. This
/// conversion follows GDAL's long-standing `CPLVaxToIEEEDouble` behavior and
/// preserves whether any discarded low bit was set in the resulting least
/// significant IEEE fraction bit.
#[must_use]
pub fn decode_vax_d_f64(bytes: [u8; 8]) -> f64 {
    let mut high = u32::from_le_bytes([bytes[2], bytes[3], bytes[0], bytes[1]]);
    let mut low = u32::from_le_bytes([bytes[6], bytes[7], bytes[4], bytes[5]]);

    let sign = high & 0x8000_0000;
    let vax_exponent = (high >> 23) & 0xff;
    let ieee_exponent = if vax_exponent == 0 {
        0
    } else {
        vax_exponent + (1023 - 129)
    };

    let discarded_bits = low & 0x7;
    low = ((low >> 3) & 0x1fff_ffff) | (high << 29);
    if discarded_bits != 0 {
        low |= 1;
    }

    high = ((high >> 3) & 0x000f_ffff) | (ieee_exponent << 20) | sign;
    f64::from_bits((u64::from(high) << 32) | u64::from(low))
}

/// Converts one finite IEEE-754 `f64` into the VAX D-floating byte layout
/// used by V7 DGN records.
///
/// Values outside the VAX exponent range are saturated or underflowed in the
/// same way as GDAL's `CPLIEEEToVaxDouble`. Writer-facing validation rejects
/// non-finite geometry before calling this primitive.
#[must_use]
pub fn encode_vax_d_f64(value: f64) -> [u8; 8] {
    let bits = value.to_bits();
    let ieee_high = (bits >> 32) as u32;
    let ieee_low = bits as u32;
    let sign = ieee_high & 0x8000_0000;
    let ieee_exponent = ((ieee_high >> 20) & 0x7ff) as i32;
    let vax_exponent = if ieee_exponent == 0 {
        0
    } else {
        ieee_exponent - 1023 + 129
    };

    if vax_exponent > 255 {
        return if sign == 0 {
            [0xff, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        } else {
            [0xff; 8]
        };
    }
    if vax_exponent < 0 || (vax_exponent == 0 && sign == 0) {
        return [0; 8];
    }

    let vax_high = (((ieee_high << 3) | (ieee_low >> 29)) & 0x007f_ffff)
        | ((vax_exponent as u32) << 23)
        | sign;
    let vax_low = ieee_low << 3;
    let high = vax_high.to_le_bytes();
    let low = vax_low.to_le_bytes();
    [
        high[2], high[3], high[0], high[1], low[2], low[3], low[0], low[1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_middle_endian_integer_variants() {
        assert_eq!(
            decode_middle_endian_u32([0x12, 0x34, 0x56, 0x78]),
            0x3412_7856
        );
        assert_eq!(decode_middle_endian_i32([0xff, 0xff, 0xff, 0xff]), -1);
        assert_eq!(decode_offset_binary_i32([0x00, 0x80, 0x00, 0x00]), 0);
        assert_eq!(decode_offset_binary_i32([0x00, 0x80, 0x01, 0x00]), 1);
        assert_eq!(decode_offset_binary_i32([0xff, 0x7f, 0xff, 0xff]), -1);
    }

    #[test]
    fn decodes_known_vax_d_floating_values() {
        assert_eq!(decode_vax_d_f64([0; 8]), 0.0);
        assert_eq!(decode_vax_d_f64([0x80, 0x40, 0, 0, 0, 0, 0, 0]), 1.0);
        assert_eq!(decode_vax_d_f64([0x80, 0xc0, 0, 0, 0, 0, 0, 0]), -1.0);
        assert_eq!(decode_vax_d_f64([0x00, 0x40, 0, 0, 0, 0, 0, 0]), 0.5);
        assert_eq!(decode_vax_d_f64([0x00, 0x41, 0, 0, 0, 0, 0, 0]), 2.0);
    }

    #[test]
    fn encodes_known_vax_d_floating_values() {
        assert_eq!(encode_vax_d_f64(0.0), [0; 8]);
        assert_eq!(encode_vax_d_f64(1.0), [0x80, 0x40, 0, 0, 0, 0, 0, 0]);
        assert_eq!(encode_vax_d_f64(-1.0), [0x80, 0xc0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(encode_vax_d_f64(0.5), [0x00, 0x40, 0, 0, 0, 0, 0, 0]);
        assert_eq!(encode_vax_d_f64(2.0), [0x00, 0x41, 0, 0, 0, 0, 0, 0]);

        for value in [
            -123_456.75,
            -0.125,
            0.25,
            1.0,
            12_345.678_901_234_5,
            f64::MIN_POSITIVE,
        ] {
            let decoded = decode_vax_d_f64(encode_vax_d_f64(value));
            if value.abs() < 1.0e-37 {
                assert_eq!(decoded, 0.0);
            } else {
                assert_eq!(decoded, value);
            }
        }
    }
}
