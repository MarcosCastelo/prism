use sha2::{Digest, Sha256};

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_hash() {
        // SHA-256("") = e3b0c44298fc1c149afb...
        let result = sha256(b"");
        assert_eq!(result[0], 0xe3);
        assert_eq!(result[1], 0xb0);
    }

    #[test]
    fn deterministic() {
        assert_eq!(sha256(b"prism"), sha256(b"prism"));
    }

    #[test]
    fn different_inputs_differ() {
        assert_ne!(sha256(b"a"), sha256(b"b"));
    }
}
