//! Decoding for Prusa's binary `.bgcode` format to plain ASCII gcode text,
//! so it can flow through the same text-based pipeline as `.gcode` files.

/// True if `bytes` looks like a `.bgcode` file: either the extension says so,
/// or the file starts with the format's "GCDE" magic (spec: prusa3d/libbgcode).
/// The magic check lets drag-dropped files with a misleading name still work.
pub fn is_bgcode(file_name: &str, bytes: &[u8]) -> bool {
    file_name.to_ascii_lowercase().ends_with(".bgcode") || bytes.starts_with(b"GCDE")
}

/// Decode a whole `.bgcode` file to ASCII gcode text.
pub fn decode(bytes: &[u8]) -> Result<String, String> {
    binarygcode::binary_to_ascii(bytes, false)
        .map(|s| s.into_string())
        .map_err(|err| format!("bgcode decode failed: {err:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_ascii_binary_ascii() {
        // binarygcode's ascii_to_binary only picks up the gcode body between
        // PrusaSlicer's "M73 P0" / "M73 P100 R0" progress markers; without
        // them it silently emits a file with no gcode block at all.
        let src = "M73 P0 R10\nG28\nG1 X10 Y10 Z0.2 E1.0 F1200\nG1 X20 Y10 E2.0\nM73 P100 R0\n";
        let bin = binarygcode::ascii_to_binary(src).expect("encode");
        assert!(is_bgcode("x.bgcode", &bin));
        assert!(bin.starts_with(b"GCDE"));

        let out = decode(&bin).expect("decode");
        assert!(out.contains("G1 X10 Y10"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"not a bgcode file").is_err());
    }

    #[test]
    fn plain_gcode_is_not_bgcode() {
        assert!(!is_bgcode("model.gcode", b"G28\n"));
    }
}
