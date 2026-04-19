use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use zeroize::Zeroize;

use crate::hash::sha256;

pub struct Identity {
    signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
    pub node_id: [u8; 32],
}

impl Identity {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let node_id = sha256(verifying_key.as_bytes());
        Self { signing_key, verifying_key, node_id }
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(path)?;
            let mode = meta.permissions().mode();
            if mode & 0o044 != 0 {
                tracing::warn!(
                    path = %path.display(),
                    "key file is readable by group/others (mode {:o}) — consider chmod 600",
                    mode & 0o777
                );
            }
        }

        let bytes = std::fs::read(path)
            .map_err(|e| anyhow::anyhow!("cannot read key file {}: {}", path.display(), e))?;

        if bytes.len() != 32 {
            anyhow::bail!(
                "key file {} has wrong size: expected 32 bytes, got {}",
                path.display(),
                bytes.len()
            );
        }

        let key_bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("key file conversion failed"))?;

        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();
        let node_id = sha256(verifying_key.as_bytes());
        Ok(Self { signing_key, verifying_key, node_id })
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let bytes = self.signing_key.to_bytes();
        std::fs::write(path, bytes)
            .map_err(|e| anyhow::anyhow!("cannot write key file {}: {}", path.display(), e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms).map_err(|e| {
                anyhow::anyhow!("failed to set 0600 on {}: {}", path.display(), e)
            })?;
        }

        #[cfg(windows)]
        {
            // On Windows set the file to read-only as a minimal restriction
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_readonly(true);
            std::fs::set_permissions(path, perms).map_err(|e| {
                anyhow::anyhow!("failed to set readonly on {}: {}", path.display(), e)
            })?;
        }

        Ok(())
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }

    pub fn verify(data: &[u8], sig: &Signature, pubkey: &VerifyingKey) -> bool {
        pubkey.verify(data, sig).is_ok()
    }

    pub fn pubkey_hex(&self) -> String {
        hex::encode(self.verifying_key.as_bytes())
    }
}

impl Drop for Identity {
    fn drop(&mut self) {
        let mut bytes = self.signing_key.to_bytes();
        bytes.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_roundtrip() {
        let id = Identity::generate();
        let sig = id.sign(b"hello prism");
        assert!(Identity::verify(b"hello prism", &sig, &id.verifying_key));
    }

    #[test]
    fn verify_rejects_tampered_data() {
        let id = Identity::generate();
        let sig = id.sign(b"original");
        assert!(!Identity::verify(b"tampered", &sig, &id.verifying_key));
    }

    #[test]
    fn verify_rejects_wrong_key() {
        let id_a = Identity::generate();
        let id_b = Identity::generate();
        let sig = id_a.sign(b"data");
        assert!(!Identity::verify(b"data", &sig, &id_b.verifying_key));
    }

    #[test]
    fn save_load_preserves_node_id() {
        let id = Identity::generate();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        id.save(tmp.path()).unwrap();
        let loaded = Identity::load(tmp.path()).unwrap();
        assert_eq!(id.node_id, loaded.node_id);
        assert_eq!(id.pubkey_hex(), loaded.pubkey_hex());
    }

    #[cfg(unix)]
    #[test]
    fn save_applies_restricted_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let id = Identity::generate();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        id.save(tmp.path()).unwrap();
        let mode = std::fs::metadata(tmp.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "file must be 0600");
    }

    #[test]
    fn node_id_must_equal_sha256_of_pubkey() {
        let id = Identity::generate();
        assert_eq!(id.node_id, sha256(id.verifying_key.as_bytes()));
    }
}
