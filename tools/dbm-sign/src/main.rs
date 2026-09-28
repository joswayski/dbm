//! Signs release artifacts for the DBM updaters.
//!
//!   DBM_UPDATE_SIGNING_KEY=… DBM_UPDATE_SIGNING_KEY_PASSWORD=… dbm-sign FILE...
//!
//! Writes `FILE.sig` next to each file: base64 of a minisign signature, the
//! format `crates/dbm-update` verifies and the one `tauri signer sign`
//! produced, so installed Tauri builds accept these files as updates too. The
//! key is the release updater key: base64 of an encrypted minisign secret key.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;

fn main() {
    if let Err(error) = run() {
        eprintln!("dbm-sign: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        return Err("usage: dbm-sign FILE...".into());
    }
    let key = std::env::var("DBM_UPDATE_SIGNING_KEY")
        .map_err(|_| "DBM_UPDATE_SIGNING_KEY is not set".to_owned())?;
    let password = std::env::var("DBM_UPDATE_SIGNING_KEY_PASSWORD").unwrap_or_default();
    let secret_key = secret_key(&key, &password)?;
    for file in files {
        let signature = sign(&secret_key, Path::new(&file))?;
        std::fs::write(format!("{file}.sig"), &signature)
            .map_err(|error| format!("couldn't write {file}.sig: {error}"))?;
        println!("Signed {file}");
    }
    Ok(())
}

fn secret_key(encoded: &str, password: &str) -> Result<minisign::SecretKey, String> {
    let text = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or("the signing key is not base64 text")?;
    minisign::SecretKeyBox::from_string(&text)
        .and_then(|secret| secret.into_secret_key(Some(password.to_owned())))
        .map_err(|error| format!("couldn't open the signing key: {error}"))
}

/// Base64 of the minisign signature file for `path`.
fn sign(secret_key: &minisign::SecretKey, path: &Path) -> Result<String, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let reader = File::open(path)
        .map(BufReader::new)
        .map_err(|error| format!("couldn't read {}: {error}", path.display()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let trusted = format!("timestamp:{timestamp}\tfile:{name}");
    let signature = minisign::sign(
        None,
        secret_key,
        reader,
        Some(&trusted),
        Some("signature from dbm secret key"),
    )
    .map_err(|error| format!("couldn't sign {}: {error}", path.display()))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(signature.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_verify_with_the_updater_check() {
        let minisign::KeyPair { pk, sk } =
            minisign::KeyPair::generate_encrypted_keypair(Some("pw".into())).unwrap();
        let secret_text = sk.to_box(None).unwrap().to_string();
        let encoded = base64::engine::general_purpose::STANDARD.encode(secret_text);
        let secret_key = secret_key(&encoded, "pw").unwrap();

        let dir = std::env::temp_dir().join(format!("dbm-sign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("artifact.bin");
        std::fs::write(&file, b"release bytes").unwrap();
        let encoded_signature = sign(&secret_key, &file).unwrap();

        let signature_text = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(encoded_signature)
                .unwrap(),
        )
        .unwrap();
        let public_key =
            minisign_verify::PublicKey::decode(&pk.to_box().unwrap().to_string()).unwrap();
        let signature = minisign_verify::Signature::decode(&signature_text).unwrap();
        public_key
            .verify(b"release bytes", &signature, false)
            .unwrap();
        assert!(
            public_key
                .verify(b"other bytes", &signature, false)
                .is_err()
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
