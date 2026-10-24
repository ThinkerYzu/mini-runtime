// The cryptographic primitives behind SubtleCrypto (RustCrypto), in the shapes NSS gives them to
// Gecko's WebCryptoTask: whole buffers in, whole buffers out, and Err(()) where an NSS call fails
// (the caller turns that into the DOMException Gecko gives for that call).

use aes::cipher::{BlockDecryptMut, BlockEncrypt, BlockEncryptMut, KeyInit, KeyIvInit, block_padding::Pkcs7};
use ghash::GHash;
use ghash::universal_hash::UniversalHash;
use hmac::{Hmac, Mac};
use rand::RngCore;
use rand::rngs::OsRng;
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, RsaPrivateKey, RsaPublicKey};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

pub type R<T> = Result<T, ()>;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    /// The hash a WebCrypto name stands for (MapHashAlgorithmNameToOID); None for any other name.
    pub fn from_name(name: &str) -> Option<Hash> {
        match name {
            "SHA-1" => Some(Hash::Sha1),
            "SHA-256" => Some(Hash::Sha256),
            "SHA-384" => Some(Hash::Sha384),
            "SHA-512" => Some(Hash::Sha512),
            _ => None,
        }
    }

    /// HMAC's default key length in bits (MapHashAlgorithmNameToBlockSize).
    pub fn block_bits(self) -> usize {
        match self {
            Hash::Sha1 | Hash::Sha256 => 512,
            Hash::Sha384 | Hash::Sha512 => 1024,
        }
    }

    pub fn len(self) -> usize {
        match self {
            Hash::Sha1 => 20,
            Hash::Sha256 => 32,
            Hash::Sha384 => 48,
            Hash::Sha512 => 64,
        }
    }

    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Hash::Sha1 => Sha1::digest(data).to_vec(),
            Hash::Sha256 => Sha256::digest(data).to_vec(),
            Hash::Sha384 => Sha384::digest(data).to_vec(),
            Hash::Sha512 => Sha512::digest(data).to_vec(),
        }
    }

    pub fn hmac(self, key: &[u8], data: &[u8]) -> Vec<u8> {
        macro_rules! mac {
            ($h:ty) => {{
                let mut m = <Hmac<$h> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
                m.update(data);
                m.finalize().into_bytes().to_vec()
            }};
        }
        match self {
            Hash::Sha1 => mac!(Sha1),
            Hash::Sha256 => mac!(Sha256),
            Hash::Sha384 => mac!(Sha384),
            Hash::Sha512 => mac!(Sha512),
        }
    }

    pub fn hkdf(self, ikm: &[u8], salt: &[u8], info: &[u8], len: usize) -> R<Vec<u8>> {
        let mut out = vec![0; len];
        macro_rules! expand {
            ($h:ty) => {
                hkdf::Hkdf::<$h>::new(Some(salt), ikm).expand(info, &mut out).map_err(|_| ())
            };
        }
        match self {
            Hash::Sha1 => expand!(Sha1),
            Hash::Sha256 => expand!(Sha256),
            Hash::Sha384 => expand!(Sha384),
            Hash::Sha512 => expand!(Sha512),
        }?;
        Ok(out)
    }

    pub fn pbkdf2(self, password: &[u8], salt: &[u8], iterations: u32, len: usize) -> Vec<u8> {
        let mut out = vec![0; len];
        match self {
            Hash::Sha1 => pbkdf2::pbkdf2_hmac::<Sha1>(password, salt, iterations, &mut out),
            Hash::Sha256 => pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, iterations, &mut out),
            Hash::Sha384 => pbkdf2::pbkdf2_hmac::<Sha384>(password, salt, iterations, &mut out),
            Hash::Sha512 => pbkdf2::pbkdf2_hmac::<Sha512>(password, salt, iterations, &mut out),
        }
        out
    }

    /// MGF1 (RFC 8017, B.2.1) of `seed` to `len` bytes.
    fn mgf1(self, seed: &[u8], len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len + self.len());
        let mut counter = 0u32;
        while out.len() < len {
            let mut block = seed.to_vec();
            block.extend_from_slice(&counter.to_be_bytes());
            out.extend(self.digest(&block));
            counter += 1;
        }
        out.truncate(len);
        out
    }
}

pub fn random(len: usize) -> Vec<u8> {
    let mut b = vec![0; len];
    OsRng.fill_bytes(&mut b);
    b
}

// ---- AES ----

enum Aes {
    A128(aes::Aes128),
    A192(aes::Aes192),
    A256(aes::Aes256),
}

impl Aes {
    fn new(key: &[u8]) -> R<Aes> {
        Ok(match key.len() {
            16 => Aes::A128(aes::Aes128::new_from_slice(key).map_err(|_| ())?),
            24 => Aes::A192(aes::Aes192::new_from_slice(key).map_err(|_| ())?),
            32 => Aes::A256(aes::Aes256::new_from_slice(key).map_err(|_| ())?),
            _ => return Err(()),
        })
    }

    fn block(&self, b: &[u8; 16]) -> [u8; 16] {
        let mut x = aes::Block::from(*b);
        match self {
            Aes::A128(c) => c.encrypt_block(&mut x),
            Aes::A192(c) => c.encrypt_block(&mut x),
            Aes::A256(c) => c.encrypt_block(&mut x),
        }
        x.into()
    }
}

/// AES-CBC with PKCS #7 padding (CKM_AES_CBC_PAD).
pub fn aes_cbc(key: &[u8], iv: &[u8], data: &[u8], encrypt: bool) -> R<Vec<u8>> {
    macro_rules! run {
        ($c:ty) => {
            if encrypt {
                Ok(cbc::Encryptor::<$c>::new_from_slices(key, iv).map_err(|_| ())?.encrypt_padded_vec_mut::<Pkcs7>(data))
            } else {
                cbc::Decryptor::<$c>::new_from_slices(key, iv).map_err(|_| ())?.decrypt_padded_vec_mut::<Pkcs7>(data).map_err(|_| ())
            }
        };
    }
    match key.len() {
        16 => run!(aes::Aes128),
        24 => run!(aes::Aes192),
        32 => run!(aes::Aes256),
        _ => Err(()),
    }
}

/// AES-CTR: the counter block's rightmost `bits` bits are the counter, wrapping within them
/// (CKM_AES_CTR with ulCounterBits; NSS refuses 0 and more than 128).
pub fn aes_ctr(key: &[u8], counter: &[u8], bits: u32, data: &[u8]) -> R<Vec<u8>> {
    if bits == 0 || bits > 128 || counter.len() != 16 {
        return Err(());
    }
    let aes = Aes::new(key)?;
    let mask: u128 = if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 };
    let mut ctr = u128::from_be_bytes(counter.try_into().unwrap());
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let ks = aes.block(&ctr.to_be_bytes());
        out.extend(chunk.iter().zip(ks).map(|(a, b)| a ^ b));
        ctr = (ctr & !mask) | (ctr.wrapping_add(1) & mask);
    }
    Ok(out)
}

/// AES-GCM (NIST SP 800-38D) with any IV length and a tag of `tag_bytes`; the tag follows the
/// ciphertext. Decryption fails on a tag that does not match.
pub fn aes_gcm(key: &[u8], iv: &[u8], aad: &[u8], tag_bytes: usize, data: &[u8], encrypt: bool) -> R<Vec<u8>> {
    if iv.is_empty() {
        return Err(());
    }
    let aes = Aes::new(key)?;
    let h = aes.block(&[0; 16]);
    let ghash = |a: &[u8], c: &[u8]| -> [u8; 16] {
        let mut g = GHash::new(&h.into());
        g.update_padded(a);
        g.update_padded(c);
        let mut lens = [0u8; 16];
        lens[..8].copy_from_slice(&((a.len() as u64) * 8).to_be_bytes());
        lens[8..].copy_from_slice(&((c.len() as u64) * 8).to_be_bytes());
        g.update(&[lens.into()]);
        g.finalize().into()
    };
    let j0: [u8; 16] = if iv.len() == 12 {
        let mut j = [0u8; 16];
        j[..12].copy_from_slice(iv);
        j[15] = 1;
        j
    } else {
        ghash(&[], iv)
    };
    let ctr = |input: &[u8]| -> Vec<u8> {
        let mut cb = j0;
        let mut out = Vec::with_capacity(input.len());
        for chunk in input.chunks(16) {
            let n = u32::from_be_bytes(cb[12..].try_into().unwrap()).wrapping_add(1);
            cb[12..].copy_from_slice(&n.to_be_bytes());
            let ks = aes.block(&cb);
            out.extend(chunk.iter().zip(ks).map(|(a, b)| a ^ b));
        }
        out
    };
    let tag = |c: &[u8]| -> Vec<u8> {
        let s = ghash(aad, c);
        let e = aes.block(&j0);
        s.iter().zip(e).map(|(a, b)| a ^ b).take(tag_bytes).collect()
    };
    if encrypt {
        let mut c = ctr(data);
        let t = tag(&c);
        c.extend(t);
        Ok(c)
    } else {
        if data.len() < tag_bytes {
            return Err(());
        }
        let (c, t) = data.split_at(data.len() - tag_bytes);
        let expected = tag(c);
        if expected.iter().zip(t).fold(0u8, |acc, (a, b)| acc | (a ^ b)) != 0 {
            return Err(());
        }
        Ok(ctr(c))
    }
}

/// AES key wrap (RFC 3394, CKM_NSS_AES_KEY_WRAP).
pub fn aes_kw(key: &[u8], data: &[u8], wrap: bool) -> R<Vec<u8>> {
    macro_rules! run {
        ($k:ty) => {{
            let kek = <$k>::try_from(key).map_err(|_| ())?;
            if wrap { kek.wrap_vec(data).map_err(|_| ()) } else { kek.unwrap_vec(data).map_err(|_| ()) }
        }};
    }
    match key.len() {
        16 => run!(aes_kw::KekAes128),
        24 => run!(aes_kw::KekAes192),
        32 => run!(aes_kw::KekAes256),
        _ => Err(()),
    }
}

// ---- RSA ----

/// The modulus's length in bytes, as NSS reports it (SECKEY_PublicKeyStrength).
pub fn rsa_len(n: &BigUint) -> usize {
    n.bits().div_ceil(8) as usize
}

fn left_pad(b: Vec<u8>, len: usize) -> Vec<u8> {
    if b.len() >= len {
        return b;
    }
    let mut out = vec![0; len - b.len()];
    out.extend(b);
    out
}

/// RSAES-OAEP encryption (RFC 8017, 7.1.1) with MGF1 of the same hash; the label is any bytes.
pub fn rsa_oaep_encrypt(key: &RsaPublicKey, hash: Hash, label: &[u8], m: &[u8]) -> R<Vec<u8>> {
    let k = rsa_len(key.n());
    let h = hash.len();
    if k < 2 * h + 2 || m.len() > k - 2 * h - 2 {
        return Err(());
    }
    let mut db = hash.digest(label);
    db.resize(k - m.len() - h - 2, 0);
    db.push(1);
    db.extend_from_slice(m);
    let seed = random(h);
    let db_mask = hash.mgf1(&seed, k - h - 1);
    let masked_db: Vec<u8> = db.iter().zip(db_mask).map(|(a, b)| a ^ b).collect();
    let seed_mask = hash.mgf1(&masked_db, h);
    let mut em = vec![0u8];
    em.extend(seed.iter().zip(seed_mask).map(|(a, b)| a ^ b));
    em.extend(masked_db);
    let c = rsa::hazmat::rsa_encrypt(key, &BigUint::from_bytes_be(&em)).map_err(|_| ())?;
    Ok(left_pad(c.to_bytes_be(), k))
}

/// RSAES-OAEP decryption (RFC 8017, 7.1.2).
pub fn rsa_oaep_decrypt(key: &RsaPrivateKey, hash: Hash, label: &[u8], c: &[u8]) -> R<Vec<u8>> {
    let k = rsa_len(key.n());
    let h = hash.len();
    if c.len() != k || k < 2 * h + 2 {
        return Err(());
    }
    let ci = BigUint::from_bytes_be(c);
    if &ci >= key.n() {
        return Err(());
    }
    let m = rsa::hazmat::rsa_decrypt_and_check(key, Some(&mut OsRng), &ci).map_err(|_| ())?;
    let em = left_pad(m.to_bytes_be(), k);
    let (masked_seed, masked_db) = em[1..].split_at(h);
    let seed: Vec<u8> = masked_seed.iter().zip(hash.mgf1(masked_db, h)).map(|(a, b)| a ^ b).collect();
    let db: Vec<u8> = masked_db.iter().zip(hash.mgf1(&seed, k - h - 1)).map(|(a, b)| a ^ b).collect();
    if em[0] != 0 || db[..h] != hash.digest(label)[..] {
        return Err(());
    }
    let rest = &db[h..];
    let one = rest.iter().position(|&b| b != 0).ok_or(())?;
    if rest[one] != 1 {
        return Err(());
    }
    Ok(rest[one + 1..].to_vec())
}

macro_rules! with_digest {
    ($hash:expr, $d:ident => $body:expr) => {
        match $hash {
            Hash::Sha1 => { type $d = Sha1; $body }
            Hash::Sha256 => { type $d = Sha256; $body }
            Hash::Sha384 => { type $d = Sha384; $body }
            Hash::Sha512 => { type $d = Sha512; $body }
        }
    };
}

pub fn rsa_pkcs1_sign(key: &RsaPrivateKey, hash: Hash, data: &[u8]) -> R<Vec<u8>> {
    let digest = hash.digest(data);
    with_digest!(hash, D => key.sign(rsa::Pkcs1v15Sign::new::<D>(), &digest)).map_err(|_| ())
}

pub fn rsa_pkcs1_verify(key: &RsaPublicKey, hash: Hash, sig: &[u8], data: &[u8]) -> bool {
    let digest = hash.digest(data);
    with_digest!(hash, D => key.verify(rsa::Pkcs1v15Sign::new::<D>(), &digest, sig)).is_ok()
}

pub fn rsa_pss_sign(key: &RsaPrivateKey, hash: Hash, salt: usize, data: &[u8]) -> R<Vec<u8>> {
    let digest = hash.digest(data);
    // NSS refuses a salt that does not fit: emLen < hLen + sLen + 2
    if rsa_len(key.n()) < hash.len() + salt + 2 {
        return Err(());
    }
    with_digest!(hash, D => key.sign_with_rng(&mut OsRng, rsa::Pss::new_with_salt::<D>(salt), &digest)).map_err(|_| ())
}

pub fn rsa_pss_verify(key: &RsaPublicKey, hash: Hash, salt: usize, sig: &[u8], data: &[u8]) -> bool {
    let digest = hash.digest(data);
    with_digest!(hash, D => key.verify(rsa::Pss::new_with_salt::<D>(salt), &digest, sig)).is_ok()
}

/// A key pair of `bits` with public exponent `e` (CKM_RSA_PKCS_KEY_PAIR_GEN).
pub fn rsa_generate(bits: usize, e: u64) -> R<RsaPrivateKey> {
    // NSS's RSA_NewKey: at least 256 bits, a multiple of 16, an odd exponent above 2
    if bits < 256 || bits % 16 != 0 || bits > 16384 || e < 3 || e % 2 == 0 {
        return Err(());
    }
    RsaPrivateKey::new_with_exp(&mut OsRng, bits, &BigUint::from(e)).map_err(|_| ())
}

// ---- NIST curves ----

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Curve {
    P256,
    P384,
    P521,
}

impl Curve {
    pub fn from_name(name: &str) -> Option<Curve> {
        match name {
            "P-256" => Some(Curve::P256),
            "P-384" => Some(Curve::P384),
            "P-521" => Some(Curve::P521),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Curve::P256 => "P-256",
            Curve::P384 => "P-384",
            Curve::P521 => "P-521",
        }
    }

    /// A field element's length in bytes.
    pub fn flen(self) -> usize {
        match self {
            Curve::P256 => 32,
            Curve::P384 => 48,
            Curve::P521 => 66,
        }
    }

    pub fn oid(self) -> &'static str {
        match self {
            Curve::P256 => "1.2.840.10045.3.1.7",
            Curve::P384 => "1.3.132.0.34",
            Curve::P521 => "1.3.132.0.35",
        }
    }

    pub fn from_oid(oid: &str) -> Option<Curve> {
        [Curve::P256, Curve::P384, Curve::P521].into_iter().find(|c| c.oid() == oid)
    }
}

macro_rules! with_curve {
    ($curve:expr, $c:ident => $body:expr) => {
        match $curve {
            Curve::P256 => { use p256 as $c; $body }
            Curve::P384 => { use p384 as $c; $body }
            Curve::P521 => { use p521 as $c; $body }
        }
    };
}

/// A public point, compressed or not, checked to be on the curve; its uncompressed form.
pub fn ec_point(curve: Curve, sec1: &[u8]) -> R<Vec<u8>> {
    use elliptic_curve::sec1::ToEncodedPoint;
    with_curve!(curve, c => {
        let p = c::PublicKey::from_sec1_bytes(sec1).map_err(|_| ())?;
        Ok(p.to_encoded_point(false).as_bytes().to_vec())
    })
}

/// A private scalar's public point, uncompressed.
pub fn ec_public_of(curve: Curve, d: &[u8]) -> R<Vec<u8>> {
    use elliptic_curve::sec1::ToEncodedPoint;
    with_curve!(curve, c => {
        let k = c::SecretKey::from_slice(d).map_err(|_| ())?;
        Ok(k.public_key().to_encoded_point(false).as_bytes().to_vec())
    })
}

/// A new key pair: the private scalar (flen bytes) and the uncompressed public point.
pub fn ec_generate(curve: Curve) -> (Vec<u8>, Vec<u8>) {
    use elliptic_curve::sec1::ToEncodedPoint;
    with_curve!(curve, c => {
        let k = c::SecretKey::random(&mut OsRng);
        (k.to_bytes().to_vec(), k.public_key().to_encoded_point(false).as_bytes().to_vec())
    })
}

/// ECDSA over the digest of `data`, r and s of flen bytes each.
pub fn ecdsa_sign(curve: Curve, d: &[u8], hash: Hash, data: &[u8]) -> R<Vec<u8>> {
    use ecdsa::signature::hazmat::PrehashSigner;
    let digest = hash.digest(data);
    with_curve!(curve, c => {
        let k = c::ecdsa::SigningKey::from_slice(d).map_err(|_| ())?;
        let s: c::ecdsa::Signature = k.sign_prehash(&digest).map_err(|_| ())?;
        Ok(s.to_bytes().to_vec())
    })
}

pub fn ecdsa_verify(curve: Curve, point: &[u8], hash: Hash, sig: &[u8], data: &[u8]) -> bool {
    use ecdsa::signature::hazmat::PrehashVerifier;
    let digest = hash.digest(data);
    with_curve!(curve, c => {
        let (Ok(k), Ok(s)) = (c::ecdsa::VerifyingKey::from_sec1_bytes(point), c::ecdsa::Signature::from_slice(sig)) else {
            return false;
        };
        k.verify_prehash(&digest, &s).is_ok()
    })
}

/// ECDH's shared secret: the x coordinate, flen bytes.
pub fn ecdh(curve: Curve, d: &[u8], point: &[u8]) -> R<Vec<u8>> {
    with_curve!(curve, c => {
        let k = c::SecretKey::from_slice(d).map_err(|_| ())?;
        let p = c::PublicKey::from_sec1_bytes(point).map_err(|_| ())?;
        Ok(c::ecdh::diffie_hellman(k.to_nonzero_scalar(), p.as_affine()).raw_secret_bytes().to_vec())
    })
}

// ---- Ed25519 and X25519 ----

pub fn ed25519_public_of(seed: &[u8; 32]) -> [u8; 32] {
    ed25519_dalek::SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

pub fn ed25519_valid_point(p: &[u8; 32]) -> bool {
    ed25519_dalek::VerifyingKey::from_bytes(p).is_ok()
}

pub fn ed25519_sign(seed: &[u8; 32], data: &[u8]) -> Vec<u8> {
    use ed25519_dalek::Signer;
    ed25519_dalek::SigningKey::from_bytes(seed).sign(data).to_bytes().to_vec()
}

pub fn ed25519_verify(p: &[u8; 32], sig: &[u8], data: &[u8]) -> bool {
    let (Ok(k), Ok(s)) = (ed25519_dalek::VerifyingKey::from_bytes(p), ed25519_dalek::Signature::from_slice(sig)) else {
        return false;
    };
    k.verify_strict(data, &s).is_ok()
}

pub fn x25519_public_of(secret: &[u8; 32]) -> [u8; 32] {
    x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(*secret)).to_bytes()
}

/// X25519's shared secret; Err for the all-zero result (a small-order peer), as NSS refuses it.
pub fn x25519(secret: &[u8; 32], peer: &[u8; 32]) -> R<Vec<u8>> {
    let s = x25519_dalek::StaticSecret::from(*secret).diffie_hellman(&x25519_dalek::PublicKey::from(*peer));
    if s.as_bytes().iter().all(|&b| b == 0) { Err(()) } else { Ok(s.as_bytes().to_vec()) }
}
