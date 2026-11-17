use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct PairedDevice {
    pub device_id: String,
    pub name: String,
    pub noise_public_key: Vec<u8>,
    pub fingerprint: String,
    pub paired_at: i64,
}

#[derive(Serialize, Deserialize)]
struct KeyBundle {
    noise_private_key: Vec<u8>,
    noise_public_key: Vec<u8>,
    device_id: String,
    peer_id: u64,
    paired_devices: Vec<PairedDevice>,
}

pub struct DeviceIdentity {
    pub device_id: String,
    pub peer_id: u64,
    pub noise_private_key: Vec<u8>,
    pub noise_public_key: Vec<u8>,
    pub fingerprint: String,
    pub paired_devices: Vec<PairedDevice>,
}

impl DeviceIdentity {
    pub fn fingerprint_from_public_key(public_key: &[u8]) -> String {
        let hash = Sha256::digest(public_key);
        hex_encode(&hash[..8])
    }
}

fn sync_dir(app_data: &Path) -> PathBuf {
    app_data.join("sync")
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn generate_identity(app_data: &Path) -> Result<DeviceIdentity, String> {
    let dir = sync_dir(app_data);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create sync dir: {}", e))?;

    let mut rng = rand::thread_rng();

    // Generate device_id: 16 random bytes → 32 hex chars
    let mut id_bytes = [0u8; 16];
    rng.fill(&mut id_bytes);
    let device_id = hex_encode(&id_bytes);

    // Generate peer_id for Loro
    let peer_id: u64 = rng.gen();

    // Generate Noise keypair
    let builder = snow::Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse().unwrap());
    let keypair = builder.generate_keypair().map_err(|e| format!("Keypair generation failed: {}", e))?;

    let fingerprint = DeviceIdentity::fingerprint_from_public_key(&keypair.public);

    let identity = DeviceIdentity {
        device_id,
        peer_id,
        noise_private_key: keypair.private.to_vec(),
        noise_public_key: keypair.public.to_vec(),
        fingerprint,
        paired_devices: Vec::new(),
    };

    save_identity(app_data, &identity)?;
    Ok(identity)
}

pub fn save_identity(app_data: &Path, identity: &DeviceIdentity) -> Result<(), String> {
    let dir = sync_dir(app_data);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create sync dir: {}", e))?;

    // Write plaintext device_id and peer_id
    fs::write(dir.join("device_id"), &identity.device_id)
        .map_err(|e| format!("Failed to write device_id: {}", e))?;
    fs::write(dir.join("peer_id"), identity.peer_id.to_string())
        .map_err(|e| format!("Failed to write peer_id: {}", e))?;

    // Bundle secrets for DPAPI encryption
    let bundle = KeyBundle {
        noise_private_key: identity.noise_private_key.clone(),
        noise_public_key: identity.noise_public_key.clone(),
        device_id: identity.device_id.clone(),
        peer_id: identity.peer_id,
        paired_devices: identity.paired_devices.clone(),
    };

    let plaintext = rmp_serde::to_vec(&bundle)
        .map_err(|e| format!("Failed to serialize key bundle: {}", e))?;

    let encrypted = dpapi_encrypt(&plaintext)?;
    fs::write(dir.join("keys.dat"), encrypted)
        .map_err(|e| format!("Failed to write keys.dat: {}", e))?;

    Ok(())
}

pub fn load_identity(app_data: &Path) -> Result<Option<DeviceIdentity>, String> {
    let dir = sync_dir(app_data);
    let keys_path = dir.join("keys.dat");

    if !keys_path.exists() {
        return Ok(None);
    }

    let encrypted = fs::read(&keys_path)
        .map_err(|e| format!("Failed to read keys.dat: {}", e))?;

    let plaintext = dpapi_decrypt(&encrypted)?;

    let bundle: KeyBundle = rmp_serde::from_slice(&plaintext)
        .map_err(|e| format!("Failed to deserialize key bundle: {}", e))?;

    let fingerprint = DeviceIdentity::fingerprint_from_public_key(&bundle.noise_public_key);

    Ok(Some(DeviceIdentity {
        device_id: bundle.device_id,
        peer_id: bundle.peer_id,
        noise_private_key: bundle.noise_private_key,
        noise_public_key: bundle.noise_public_key,
        fingerprint,
        paired_devices: bundle.paired_devices,
    }))
}

// --- DPAPI wrappers ---

#[cfg(windows)]
pub(crate) fn dpapi_encrypt(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPT_INTEGER_BLOB,
    };

    extern "system" {
        fn LocalFree(hmem: *mut u8) -> *mut u8;
    }

    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };

        let result = CryptProtectData(
            &mut input,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut output,
        );

        if result == 0 {
            return Err("DPAPI CryptProtectData failed".into());
        }

        let encrypted = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData);

        Ok(encrypted)
    }
}

#[cfg(windows)]
pub(crate) fn dpapi_decrypt(encrypted: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    extern "system" {
        fn LocalFree(hmem: *mut u8) -> *mut u8;
    }

    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: encrypted.len() as u32,
            pbData: encrypted.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };

        let result = CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut output,
        );

        if result == 0 {
            return Err("DPAPI CryptUnprotectData failed".into());
        }

        let decrypted = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData);

        Ok(decrypted)
    }
}

// Versioned, authenticated at-rest encryption. The 256-bit wrapping key is
// stored by the user's desktop Secret Service, not alongside keys.dat/vault.db.
// This backend needs libsecret's secret-tool and an unlocked user keyring.
// There is deliberately no plaintext or passwordless filesystem fallback.
#[cfg(target_os = "linux")]
const LINUX_ENVELOPE: &[u8] = b"BSH-LINUX-V1\0";

#[cfg(target_os = "linux")]
fn linux_seal_with_key(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    use chacha20poly1305::{aead::{Aead, KeyInit}, ChaCha20Poly1305, Nonce};
    use rand::RngCore;
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| "Invalid encryption key length".to_string())?;
    let mut nonce = [0_u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|_| "Linux secret encryption failed".to_string())?;
    let mut envelope = Vec::with_capacity(LINUX_ENVELOPE.len() + 12 + ciphertext.len());
    envelope.extend_from_slice(LINUX_ENVELOPE);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

#[cfg(target_os = "linux")]
fn linux_open_with_key(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, String> {
    use chacha20poly1305::{aead::{Aead, KeyInit}, ChaCha20Poly1305, Nonce};
    if !blob.starts_with(LINUX_ENVELOPE) || blob.len() < LINUX_ENVELOPE.len() + 12 + 16 {
        // This also rejects historical non-Windows plaintext keys.dat blobs.
        return Err("Unknown or unauthenticated Linux secret envelope".into());
    }
    let rest = &blob[LINUX_ENVELOPE.len()..];
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| "Invalid decryption key length".to_string())?;
    cipher.decrypt(Nonce::from_slice(&rest[..12]), &rest[12..])
        .map_err(|_| "Linux secret authentication failed".into())
}

#[cfg(target_os = "linux")]
fn linux_keyring_lookup() -> Result<Option<zeroize::Zeroizing<[u8; 32]>>, String> {
    use std::process::{Command, Stdio};
    let output = Command::new("secret-tool")
        .args(["lookup", "application", "bushido-browser", "purpose", "linux-wrap-v1"])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "Cannot access secret-tool; install libsecret-tools and unlock the desktop keyring".to_string())?;
    // secret-tool has no distinct 'not found' result code across distributions.
    // Accept an empty result only when it has no diagnostic stderr.
    if output.stdout.is_empty() && output.stderr.is_empty() {
        return Ok(None);
    }
    if !output.status.success() {
        return Err("Desktop Secret Service lookup failed or keyring is locked".into());
    }
    let hex = zeroize::Zeroizing::new(
        String::from_utf8(output.stdout)
            .map_err(|_| "Secret Service key is not UTF-8".to_string())?
    );
    let hex = hex.trim_end_matches(&['\r', '\n'][..]);
    if hex.len() != 64 || !hex.is_ascii() {
        return Err("Desktop Secret Service key has invalid encoding".into());
    }
    let mut key = zeroize::Zeroizing::new([0_u8; 32]);
    for (i, dest) in key.iter_mut().enumerate() {
        *dest = u8::from_str_radix(&hex[i*2..i*2+2], 16)
            .map_err(|_| "Desktop Secret Service key is not hexadecimal".to_string())?;
    }
    Ok(Some(key))
}

#[cfg(target_os = "linux")]
fn linux_keyring_key(create: bool) -> Result<zeroize::Zeroizing<[u8; 32]>, String> {
    use std::process::{Command, Stdio};
    use std::io::Write;
    if let Some(key) = linux_keyring_lookup()? { return Ok(key); }
    if !create {
        return Err("Linux Secret Service wrapping key is missing; refusing decryption".into());
    }
    let mut candidate = zeroize::Zeroizing::new([0_u8; 32]);
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut candidate[..]);
    let value = zeroize::Zeroizing::new(hex_encode(&candidate[..]));
    let mut child = Command::new("secret-tool")
        .args(["store", "--label=Bushido local encryption key",
               "application", "bushido-browser", "purpose", "linux-wrap-v1"])
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Unable to start Secret Service key storage".to_string())?;
    {
        let mut stdin = child.stdin.take().ok_or("Cannot open Secret Service input")?;
        stdin.write_all(value.as_bytes())
            .map_err(|_| "Secret Service refused encryption key input".to_string())?;
    }
    if !child.wait().map_err(|_| "Secret Service storage did not complete")?.success() {
        return Err("Secret Service could not save a wrapping key".into());
    }
    // Re-read the authoritative stored key to avoid first-use races across
    // independent Bushido processes. Never use an unconfirmed candidate key.
    linux_keyring_lookup()?.ok_or_else(|| "Secret Service did not retain the wrapping key".into())
}

#[cfg(target_os = "linux")]
pub(crate) fn dpapi_encrypt(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let key = linux_keyring_key(true)?;
    linux_seal_with_key(&key, plaintext)
}

#[cfg(target_os = "linux")]
pub(crate) fn dpapi_decrypt(encrypted: &[u8]) -> Result<Vec<u8>, String> {
    // Reject invalid/legacy blobs before opening or prompting for a keyring.
    if !encrypted.starts_with(LINUX_ENVELOPE) {
        return Err("Unknown or unauthenticated Linux secret envelope".into());
    }
    let key = linux_keyring_key(false)?;
    linux_open_with_key(&key, encrypted)
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub(crate) fn dpapi_encrypt(_plaintext: &[u8]) -> Result<Vec<u8>, String> {
    Err("Secure key wrapping is not implemented on this platform".into())
}

#[cfg(all(not(windows), not(target_os = "linux")))]
pub(crate) fn dpapi_decrypt(_encrypted: &[u8]) -> Result<Vec<u8>, String> {
    Err("Secure key unwrapping is not implemented on this platform".into())
}

#[cfg(all(test, target_os = "linux"))]
mod linux_key_storage_tests {
    use super::*;

    #[test]
    fn envelope_roundtrip_requires_correct_key() {
        let key = [0x35_u8; 32];
        let plain = b"noise-private-key-and-vault-entry";
        let ciphertext = linux_seal_with_key(&key, plain).expect("seal");
        assert!(ciphertext.starts_with(LINUX_ENVELOPE));
        assert!(!ciphertext.windows(plain.len()).any(|part| part == plain));
        assert_eq!(linux_open_with_key(&key, &ciphertext).expect("open"), plain);
        assert!(linux_open_with_key(&[0x36_u8; 32], &ciphertext).is_err());
    }

    #[test]
    fn tampering_and_legacy_plaintext_are_rejected() {
        let key = [0x55_u8; 32];
        let mut ciphertext = linux_seal_with_key(&key, b"important").expect("seal");
        *ciphertext.last_mut().unwrap() ^= 0x01;
        assert!(linux_open_with_key(&key, &ciphertext).is_err());
        assert!(linux_open_with_key(&key, b"important").is_err());
        assert!(linux_open_with_key(&key, LINUX_ENVELOPE).is_err());
        assert!(linux_open_with_key(&key, b"BSH-LINUX-V0\0bad").is_err());
    }

    #[test]
    fn nonce_is_unique_for_repeated_encryption() {
        let key = [0x77_u8; 32];
        let left = linux_seal_with_key(&key, b"same").expect("seal");
        let right = linux_seal_with_key(&key, b"same").expect("seal");
        assert_ne!(left, right);
    }
}

#[cfg(all(test, not(windows), not(target_os = "linux")))]
mod unsupported_platform_tests {
    use super::*;
    #[test]
    fn unsupported_platform_cannot_write_plaintext() {
        assert!(dpapi_encrypt(b"secret").is_err());
        assert!(dpapi_decrypt(b"secret").is_err());
    }
}

