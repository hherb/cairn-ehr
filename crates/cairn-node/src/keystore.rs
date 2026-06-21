use std::path::Path;
use cairn_event::{generate_key, SigningKey};

#[derive(thiserror::Error, Debug)]
pub enum KeystoreError {
    #[error("io: {0}")] Io(#[from] std::io::Error),
    #[error("key material: {0}")] Key(String),
}

// HONEST GAP (ADR-0026): v1 has NO recovery-secret escrow and NO passphrase KDF
// hardening beyond a raw at-rest file. A lost key file = a lost node identity,
// recoverable only by re-provisioning and a future `supersede`. `passphrase` is
// accepted but, for v1, the file is written with 0600 perms and no encryption;
// wiring a real KDF/seal is the ADR-0026 follow-on. This is surfaced in `status`.
pub fn generate_and_seal(path: &Path, _passphrase: Option<&str>) -> Result<(SigningKey, String), KeystoreError> {
    let (sk, kid) = generate_key().map_err(|e| KeystoreError::Key(e.to_string()))?;
    write_key_file(path, &sk.to_bytes())?;
    Ok((sk, kid))
}

pub fn load(path: &Path, _passphrase: Option<&str>) -> Result<SigningKey, KeystoreError> {
    let bytes = std::fs::read(path)?;
    let seed: [u8; 32] = bytes.as_slice().try_into().map_err(|_| KeystoreError::Key("not 32 bytes".into()))?;
    Ok(SigningKey::from_bytes(&seed))
}

#[cfg(unix)]
fn write_key_file(path: &Path, bytes: &[u8]) -> Result<(), KeystoreError> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(bytes)?;
    Ok(())
}
#[cfg(not(unix))]
fn write_key_file(path: &Path, bytes: &[u8]) -> Result<(), KeystoreError> {
    std::fs::write(path, bytes)?; Ok(())
}
