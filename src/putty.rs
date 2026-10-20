//! PuTTY private keys (.ppk, versions 2 and 3) turned into OpenSSH private keys, for the system ssh,
//! which only reads its own format. A key protected by a passphrase stays protected by it: the copy is
//! encrypted again the way ssh-keygen does (bcrypt, AES-256-CTR), and ssh asks for it as usual.
//!
//! Format: https://the.earth.li/~sgtatham/putty/0.83/htmldoc/AppendixC.html

use aes::cipher::{BlockDecryptMut, KeyIvInit, StreamCipher};
use base64::Engine;
use chacha20poly1305::aead::rand_core::RngCore;
use hmac::Mac;
use sha1::Digest;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a PuTTY key, or a damaged one.
    Format,
    /// A key type or file version ssh can't use (Ed448, PuTTY's first format...).
    Unsupported(String),
    /// The key is protected and no passphrase was given.
    NeedsPassphrase,
    WrongPassphrase,
}

/// The parts of a .ppk file.
struct Ppk {
    version: u8,
    algorithm: String,
    encryption: String,
    comment: String,
    public: Vec<u8>,
    private: Vec<u8>,
    mac: Vec<u8>,
    /// Version 3 with a passphrase: the Argon2 settings.
    argon2: Option<(argon2::Algorithm, u32, u32, u32, Vec<u8>)>,
}

/// Whether the key is protected by a passphrase (None: not a PuTTY key).
pub fn is_encrypted(text: &str) -> Option<bool> {
    parse(text).ok().map(|ppk| ppk.encryption != "none")
}

/// The OpenSSH private key ("-----BEGIN OPENSSH PRIVATE KEY-----…") for a .ppk file's text; a protected
/// key needs its passphrase, and is encrypted again with it.
pub fn to_openssh(text: &str, passphrase: Option<&str>) -> Result<String, Error> {
    let ppk = parse(text)?;
    let passphrase = match ppk.encryption.as_str() {
        "none" => None,
        "aes256-cbc" => Some(passphrase.filter(|p| !p.is_empty()).ok_or(Error::NeedsPassphrase)?),
        other => return Err(Error::Unsupported(other.to_owned())),
    };
    let private = unlock(&ppk, passphrase)?;
    let (public, private) = openssh_parts(&ppk.algorithm, &ppk.public, &private)?;
    Ok(openssh_file(&public, &private, &ppk.comment, passphrase))
}

fn parse(text: &str) -> Result<Ppk, Error> {
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let (head, algorithm) = lines.next().and_then(|l| l.split_once(": ")).ok_or(Error::Format)?;
    let version = match head {
        "PuTTY-User-Key-File-2" => 2,
        "PuTTY-User-Key-File-3" => 3,
        "PuTTY-User-Key-File-1" | "PuTTY-User-Key-File" => return Err(Error::Unsupported("PuTTY 1".to_owned())),
        _ => return Err(Error::Format),
    };
    let mut fields = std::collections::HashMap::new();
    let (mut public, mut private) = (None, None);
    while let Some(line) = lines.next() {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(": ").ok_or(Error::Format)?;
        if let Some(kind) = name.strip_suffix("-Lines") {
            // The base64 lines that follow.
            let count: usize = value.trim().parse().map_err(|_| Error::Format)?;
            let joined: String = lines.by_ref().take(count).collect();
            let bytes = base64::engine::general_purpose::STANDARD.decode(joined.trim()).map_err(|_| Error::Format)?;
            match kind {
                "Public" => public = Some(bytes),
                "Private" => private = Some(bytes),
                _ => {}
            }
        } else {
            fields.insert(name.to_owned(), value.trim().to_owned());
        }
    }
    let field = |name: &str| fields.get(name).cloned().ok_or(Error::Format);
    let encryption = field("Encryption")?;
    let argon2 = if version == 3 && encryption != "none" {
        let algorithm = match field("Key-Derivation")?.as_str() {
            "Argon2id" => argon2::Algorithm::Argon2id,
            "Argon2i" => argon2::Algorithm::Argon2i,
            "Argon2d" => argon2::Algorithm::Argon2d,
            other => return Err(Error::Unsupported(other.to_owned())),
        };
        let number = |name: &str| field(name)?.parse::<u32>().map_err(|_| Error::Format);
        let salt = from_hex(&field("Argon2-Salt")?).ok_or(Error::Format)?;
        Some((algorithm, number("Argon2-Memory")?, number("Argon2-Passes")?, number("Argon2-Parallelism")?, salt))
    } else {
        None
    };
    Ok(Ppk {
        version,
        algorithm: algorithm.to_owned(),
        encryption,
        comment: fields.get("Comment").cloned().unwrap_or_default(),
        public: public.ok_or(Error::Format)?,
        private: private.ok_or(Error::Format)?,
        mac: from_hex(&field("Private-MAC")?).ok_or(Error::Format)?,
        argon2,
    })
}

/// The private blob, decrypted if needed, once the file's MAC proves the passphrase right.
fn unlock(ppk: &Ppk, passphrase: Option<&str>) -> Result<Vec<u8>, Error> {
    let pass = passphrase.unwrap_or("").as_bytes();
    // Cipher key and IV, and MAC key.
    let (cipher, mac_key): (Option<([u8; 32], [u8; 16])>, Vec<u8>) = match (ppk.version, &ppk.argon2) {
        (3, Some((algorithm, memory, passes, parallelism, salt))) => {
            let params = argon2::Params::new(*memory, *passes, *parallelism, Some(80)).map_err(|_| Error::Format)?;
            let mut out = [0u8; 80];
            argon2::Argon2::new(*algorithm, argon2::Version::V0x13, params).hash_password_into(pass, salt, &mut out).map_err(|_| Error::Format)?;
            (Some((out[..32].try_into().unwrap(), out[32..48].try_into().unwrap())), out[48..].to_vec())
        }
        (3, None) => (None, Vec::new()),
        _ => {
            let cipher = passphrase.map(|_| {
                let mut key = [0u8; 32];
                key[..20].copy_from_slice(&sha1::Sha1::new().chain_update([0, 0, 0, 0]).chain_update(pass).finalize());
                key[20..].copy_from_slice(&sha1::Sha1::new().chain_update([0, 0, 0, 1]).chain_update(pass).finalize()[..12]);
                (key, [0u8; 16])
            });
            let mac_key = sha1::Sha1::new().chain_update(b"putty-private-key-file-mac-key").chain_update(pass).finalize().to_vec();
            (cipher, mac_key)
        }
    };
    let mut private = ppk.private.clone();
    if let Some((key, iv)) = cipher {
        if private.len() % 16 != 0 {
            return Err(Error::Format);
        }
        let mut decryptor = cbc::Decryptor::<aes::Aes256>::new(&key.into(), &iv.into());
        for block in private.chunks_exact_mut(16) {
            decryptor.decrypt_block_mut(block.into());
        }
    }
    let mut data = Vec::new();
    for part in [ppk.algorithm.as_bytes(), ppk.encryption.as_bytes(), ppk.comment.as_bytes(), &ppk.public, &private] {
        put_string(&mut data, part);
    }
    let right = if ppk.version == 3 {
        let mut mac = <hmac::Hmac<sha2::Sha256> as hmac::KeyInit>::new_from_slice(&mac_key).map_err(|_| Error::Format)?;
        mac.update(&data);
        mac.verify_slice(&ppk.mac).is_ok()
    } else {
        let mut mac = <hmac::Hmac<sha1::Sha1> as hmac::KeyInit>::new_from_slice(&mac_key).map_err(|_| Error::Format)?;
        mac.update(&data);
        mac.verify_slice(&ppk.mac).is_ok()
    };
    match (right, passphrase) {
        (true, _) => Ok(private),
        (false, Some(_)) => Err(Error::WrongPassphrase),
        (false, None) => Err(Error::Format),
    }
}

/// The public blob, and the key's fields as OpenSSH writes them in its private section.
fn openssh_parts(algorithm: &str, public: &[u8], private: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let mut pub_in = Reader(public);
    let mut priv_in = Reader(private);
    if pub_in.string()? != algorithm.as_bytes() {
        return Err(Error::Format);
    }
    let mut out = Vec::new();
    put_string(&mut out, algorithm.as_bytes());
    match algorithm {
        "ssh-rsa" => {
            let (e, n) = (pub_in.string()?, pub_in.string()?);
            let (d, p, q, iqmp) = (priv_in.string()?, priv_in.string()?, priv_in.string()?, priv_in.string()?);
            for part in [n, e, d, iqmp, p, q] {
                put_string(&mut out, part);
            }
        }
        "ssh-dss" => {
            for _ in 0..4 {
                put_string(&mut out, pub_in.string()?);
            }
            put_string(&mut out, priv_in.string()?);
        }
        "ecdsa-sha2-nistp256" | "ecdsa-sha2-nistp384" | "ecdsa-sha2-nistp521" => {
            put_string(&mut out, pub_in.string()?);
            put_string(&mut out, pub_in.string()?);
            put_string(&mut out, priv_in.string()?);
        }
        "ssh-ed25519" => {
            let pk = pub_in.string()?;
            // PuTTY writes the secret as a little-endian number: its top zero bytes may be missing.
            let seed = priv_in.string()?;
            if pk.len() != 32 || seed.len() > 32 {
                return Err(Error::Format);
            }
            let mut sk = seed.to_vec();
            sk.resize(32, 0);
            sk.extend_from_slice(pk);
            put_string(&mut out, pk);
            put_string(&mut out, &sk);
        }
        other => return Err(Error::Unsupported(other.to_owned())),
    }
    Ok((public.to_vec(), out))
}

/// An "openssh-key-v1" file, encrypted with the passphrase if one is given (as ssh-keygen does:
/// bcrypt with 16 rounds, AES-256-CTR).
fn openssh_file(public: &[u8], private: &[u8], comment: &str, passphrase: Option<&str>) -> String {
    let mut rng = chacha20poly1305::aead::OsRng;
    let check = rng.next_u32();
    let mut section = Vec::new();
    section.extend_from_slice(&check.to_be_bytes());
    section.extend_from_slice(&check.to_be_bytes());
    section.extend_from_slice(private);
    put_string(&mut section, comment.as_bytes());
    let block = if passphrase.is_some() { 16 } else { 8 };
    let mut pad = 1u8;
    while section.len() % block != 0 {
        section.push(pad);
        pad += 1;
    }

    let mut out = b"openssh-key-v1\0".to_vec();
    match passphrase {
        Some(pass) => {
            const ROUNDS: u32 = 16;
            let mut salt = [0u8; 16];
            rng.fill_bytes(&mut salt);
            let mut key_iv = [0u8; 48];
            bcrypt_pbkdf::bcrypt_pbkdf(pass.as_bytes(), &salt, ROUNDS, &mut key_iv).expect("valid bcrypt parameters");
            let mut cipher = ctr::Ctr128BE::<aes::Aes256>::new(key_iv[..32].into(), key_iv[32..].into());
            cipher.apply_keystream(&mut section);
            put_string(&mut out, b"aes256-ctr");
            put_string(&mut out, b"bcrypt");
            let mut options = Vec::new();
            put_string(&mut options, &salt);
            options.extend_from_slice(&ROUNDS.to_be_bytes());
            put_string(&mut out, &options);
        }
        None => {
            put_string(&mut out, b"none");
            put_string(&mut out, b"none");
            put_string(&mut out, b"");
        }
    }
    out.extend_from_slice(&1u32.to_be_bytes());
    put_string(&mut out, public);
    put_string(&mut out, &section);

    let encoded = base64::engine::general_purpose::STANDARD.encode(&out);
    let mut text = String::from("-----BEGIN OPENSSH PRIVATE KEY-----\n");
    for line in encoded.as_bytes().chunks(70) {
        text.push_str(std::str::from_utf8(line).unwrap());
        text.push('\n');
    }
    text.push_str("-----END OPENSSH PRIVATE KEY-----\n");
    text
}

/// SSH wire format: a big-endian length, then the bytes.
fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn string(&mut self) -> Result<&'a [u8], Error> {
        let (len, rest) = self.0.split_at_checked(4).ok_or(Error::Format)?;
        let len = u32::from_be_bytes(len.try_into().unwrap()) as usize;
        let (bytes, rest) = rest.split_at_checked(len).ok_or(Error::Format)?;
        self.0 = rest;
        Ok(bytes)
    }
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test keys made by PuTTYgen, from SshAgentLib (MIT license): https://github.com/dlech/SshAgentLib
    const PASS: &str = "correct horse battery staple";

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/ppk/{name}.ppk", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    /// The public key ssh-keygen reads from the converted file, without its comment (None when
    /// ssh-keygen isn't installed).
    fn ssh_keygen_public(openssh: &str, passphrase: Option<&str>) -> Option<String> {
        let dir = std::env::temp_dir().join(format!("ronnie-ppk-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("key");
        std::fs::write(&path, openssh).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let out = std::process::Command::new("ssh-keygen").args(["-y", "-P", passphrase.unwrap_or(""), "-f"]).arg(&path).output();
        let _ = std::fs::remove_dir_all(&dir);
        let out = out.ok()?;
        assert!(out.status.success(), "ssh-keygen: {}", String::from_utf8_lossy(&out.stderr));
        Some(String::from_utf8(out.stdout).unwrap().split_whitespace().take(2).collect::<Vec<_>>().join(" "))
    }

    fn public_line(text: &str) -> String {
        let ppk = parse(text).unwrap();
        format!("{} {}", ppk.algorithm, base64::engine::general_purpose::STANDARD.encode(&ppk.public))
    }

    #[test]
    fn converts_every_supported_key() {
        for kind in ["rsa", "ecdsa-nistp256", "ecdsa-nistp384", "ecdsa-nistp521", "eddsa-ed25519"] {
            let kdf = match kind {
                "rsa" | "eddsa-ed25519" => "argon2id",
                _ => "argon2d",
            };
            let plain = [format!("{kind}-v2-none"), format!("{kind}-v3-none-none")];
            let locked = [format!("{kind}-v2-aes256cbc"), format!("{kind}-v3-aes256cbc-{kdf}")];
            for name in plain.iter().chain(&locked) {
                let text = fixture(name);
                let passphrase = locked.contains(name).then_some(PASS);
                assert_eq!(is_encrypted(&text), Some(passphrase.is_some()), "{name}");
                let openssh = to_openssh(&text, passphrase).unwrap_or_else(|e| panic!("{name}: {e:?}"));
                assert!(openssh.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----\n"));
                if let Some(public) = ssh_keygen_public(&openssh, passphrase) {
                    assert_eq!(public, public_line(&text), "{name}");
                }
            }
        }
    }

    #[test]
    fn passphrase_is_checked() {
        let text = fixture("rsa-v3-aes256cbc-argon2id");
        assert_eq!(to_openssh(&text, None), Err(Error::NeedsPassphrase));
        assert_eq!(to_openssh(&text, Some("nope")), Err(Error::WrongPassphrase));
        assert_eq!(to_openssh(&fixture("eddsa-ed25519-v2-aes256cbc"), Some("nope")), Err(Error::WrongPassphrase));
    }

    #[test]
    fn refuses_what_ssh_cant_use() {
        assert!(matches!(to_openssh(&fixture("eddsa-ed448-v3-none-none"), None), Err(Error::Unsupported(_))));
        assert_eq!(to_openssh("-----BEGIN OPENSSH PRIVATE KEY-----", None).err(), Some(Error::Format));
        // A changed byte: the MAC no longer matches.
        let text = fixture("eddsa-ed25519-v3-none-none").replacen("Comment: ", "Comment: x", 1);
        assert_eq!(to_openssh(&text, None).err(), Some(Error::Format));
    }
}
