//! Unpredictable bytes from the operating system.

/// A hex string of `bytes` random bytes.
///
pub fn random_hex(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| format!("OS random source failed: {e}"))?;
    Ok(buf.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
        out
    }))
}

#[cfg(test)]
mod tests {
    use super::random_hex;

    #[test]
    fn hex_output_is_the_requested_length_and_does_not_repeat() {
        let a = random_hex(16).unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, random_hex(16).unwrap());
        assert_ne!(random_hex(8).unwrap(), random_hex(8).unwrap());
        assert_eq!(random_hex(0).unwrap(), "");
    }
}
