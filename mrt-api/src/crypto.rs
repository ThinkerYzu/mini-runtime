// SubtleCrypto in the API process (SPEC decision 27; DESIGN "Phase 6"), in place of the bridge tab's
// Firefox (bench/crypto_bridge.js), speaking that script's protocol: a request names its operation
// (`call`), its arguments as the runtime encoded them (js.rs), the keys it uses (`keys`, by the
// runtime's ids) and the ids for the keys it makes (`newIds`). A key this process does not hold is
// answered `{missing}`, and the runtime sends the request again with its material (`materials`).
//
// What Firefox does after the WebIDL bindings is ported from Gecko's dom/crypto: the algorithm
// normalization of each task's constructor (WebCryptoTask.cpp), the key checks and their errors in
// Gecko's order, the keys' algorithm dictionaries and usages (CryptoKey.cpp, KeyAlgorithmProxy), and
// the JWK, SPKI and PKCS #8 forms. `early` says whether Firefox settles the promise at the call (a
// FailureTask, an error from a task's constructor, BeforeCrypto, or a symmetric key's import, done
// on the main thread) or a task later, from its crypto thread; the runtime settles it the same way.
// As in the bridge tab, a key made here (generateKey, deriveKey, unwrapKey) is made extractable and
// its JWK sent back as its material, so the runtime holds every key's material (SPEC decision 21).

use std::collections::HashMap;

use rsa::traits::{PrivateKeyParts, PublicKeyParts};
use rsa::{BigUint, RsaPrivateKey, RsaPublicKey};
use serde_json::{Map, Value, json};

use crate::js::{self, member};
use crate::prims::{self, Curve, Hash};

/// The errors a task can end with: Gecko's nsresult, each a DOMException but TYPE_MISMATCH, which
/// FailWithError turns into a TypeError.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rv {
    Syntax,
    NotSupported,
    InvalidAccess,
    Data,
    Operation,
    Unknown,
    TypeMismatch,
}

impl Rv {
    fn reject(self) -> Value {
        let (name, message) = match self {
            Rv::Syntax => ("SyntaxError", "An invalid or illegal string was specified"),
            Rv::NotSupported => ("NotSupportedError", "Operation is not supported"),
            Rv::InvalidAccess => ("InvalidAccessError", "A parameter or an operation is not supported by the underlying object"),
            Rv::Data => ("DataError", "Data provided to an operation does not meet requirements"),
            Rv::Operation => ("OperationError", "The operation failed for an operation-specific reason"),
            Rv::Unknown => ("UnknownError", "The operation failed for an unknown transient reason"),
            Rv::TypeMismatch => ("TypeError", "The operation could not be performed."),
        };
        json!({ "name": name, "message": message })
    }
}

// ---- keys ----

const ENCRYPT: u32 = 0x01;
const DECRYPT: u32 = 0x02;
const SIGN: u32 = 0x04;
const VERIFY: u32 = 0x08;
const DERIVEKEY: u32 = 0x10;
const DERIVEBITS: u32 = 0x20;
const WRAPKEY: u32 = 0x40;
const UNWRAPKEY: u32 = 0x80;
// in CryptoKey::GetUsages' order
const USAGES: [(&str, u32); 8] = [("encrypt", ENCRYPT), ("decrypt", DECRYPT), ("sign", SIGN), ("verify", VERIFY),
    ("deriveKey", DERIVEKEY), ("deriveBits", DERIVEBITS), ("wrapKey", WRAPKEY), ("unwrapKey", UNWRAPKEY)];

fn usage_bit(s: &str) -> Option<u32> {
    USAGES.iter().find(|(n, _)| *n == s).map(|(_, b)| *b)
}

fn usage_names(bits: u32) -> Vec<&'static str> {
    USAGES.iter().filter(|(_, b)| bits & b != 0).map(|(n, _)| *n).collect()
}

/// CryptoKey::GetAllowedUsagesForAlgorithm
fn allowed_usages(alg: &str) -> u32 {
    match alg {
        "AES-CTR" | "AES-CBC" | "AES-GCM" | "RSA-OAEP" => ENCRYPT | DECRYPT | WRAPKEY | UNWRAPKEY,
        "AES-KW" => WRAPKEY | UNWRAPKEY,
        "HMAC" | "RSASSA-PKCS1-v1_5" | "RSA-PSS" | "ECDSA" | "Ed25519" => SIGN | VERIFY,
        "ECDH" | "HKDF" | "PBKDF2" | "X25519" => DERIVEBITS | DERIVEKEY,
        _ => 0,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum KeyType {
    Secret,
    Public,
    Private,
}

/// KeyAlgorithmProxy: what `key.algorithm` reads.
#[derive(Clone)]
enum Alg {
    Aes { name: &'static str, length: u32 },
    Kdf { name: &'static str },
    Hmac { length: u32, hash: &'static str },
    Rsa { name: &'static str, modulus_length: u16, public_exponent: Vec<u8>, hash: &'static str },
    Ec { name: &'static str, curve: &'static str },
    Okp { name: &'static str },
}

impl Alg {
    fn name(&self) -> &'static str {
        match self {
            Alg::Aes { name, .. } | Alg::Kdf { name } | Alg::Rsa { name, .. } | Alg::Ec { name, .. } | Alg::Okp { name } => name,
            Alg::Hmac { .. } => "HMAC",
        }
    }

    /// The dictionary as the binding converts it: inherited members first, then by name.
    fn value(&self) -> Value {
        let hash = |h: &str| js::object(vec![("name", json!(h))]);
        match self {
            Alg::Aes { name, length } => js::object(vec![("name", json!(name)), ("length", json!(length))]),
            Alg::Kdf { name } | Alg::Okp { name } => js::object(vec![("name", json!(name))]),
            Alg::Hmac { length, hash: h } => js::object(vec![("name", json!("HMAC")), ("hash", hash(h)), ("length", json!(length))]),
            Alg::Rsa { name, modulus_length, public_exponent, hash: h } => js::object(vec![
                ("name", json!(name)),
                ("hash", hash(h)),
                ("modulusLength", json!(modulus_length)),
                ("publicExponent", js::encode_bytes(public_exponent, "Uint8Array")),
            ]),
            Alg::Ec { name, curve } => js::object(vec![("name", json!(name)), ("namedCurve", json!(curve))]),
        }
    }

    /// KeyAlgorithmProxy::JwkAlg; None where Gecko gives the empty string.
    fn jwk_alg(&self) -> Option<&'static str> {
        let by_length = |l: &u32, a: [&'static str; 3]| match l {
            128 => Some(a[0]),
            192 => Some(a[1]),
            256 => Some(a[2]),
            _ => None,
        };
        let by_hash = |h: &str, a: [&'static str; 4]| match h {
            "SHA-1" => Some(a[0]),
            "SHA-256" => Some(a[1]),
            "SHA-384" => Some(a[2]),
            "SHA-512" => Some(a[3]),
            _ => None,
        };
        match self {
            Alg::Aes { name: "AES-CBC", length } => by_length(length, ["A128CBC", "A192CBC", "A256CBC"]),
            Alg::Aes { name: "AES-CTR", length } => by_length(length, ["A128CTR", "A192CTR", "A256CTR"]),
            Alg::Aes { name: "AES-GCM", length } => by_length(length, ["A128GCM", "A192GCM", "A256GCM"]),
            Alg::Aes { name: "AES-KW", length } => by_length(length, ["A128KW", "A192KW", "A256KW"]),
            Alg::Hmac { hash, .. } => by_hash(hash, ["HS1", "HS256", "HS384", "HS512"]),
            Alg::Rsa { name: "RSASSA-PKCS1-v1_5", hash, .. } => by_hash(hash, ["RS1", "RS256", "RS384", "RS512"]),
            Alg::Rsa { name: "RSA-OAEP", hash, .. } => by_hash(hash, ["RSA-OAEP", "RSA-OAEP-256", "RSA-OAEP-384", "RSA-OAEP-512"]),
            Alg::Rsa { name: "RSA-PSS", hash, .. } => by_hash(hash, ["PS1", "PS256", "PS384", "PS512"]),
            Alg::Ec { name: "ECDSA", curve } => match *curve {
                "P-256" => Some("ES256"),
                "P-384" => Some("ES384"),
                "P-521" => Some("ES512"),
                _ => None,
            },
            Alg::Okp { name: "Ed25519" } => Some("Ed25519"),
            _ => None,
        }
    }
}

/// What the key holds, as Gecko's CryptoKey does: a secret, or one NSS key (a private key holds no
/// public key: GetPublicKey gives null for it).
#[derive(Clone)]
enum Mat {
    Sym(Vec<u8>),
    RsaPriv(RsaPrivateKey),
    RsaPub(RsaPublicKey),
    EcPriv { curve: Curve, d: Vec<u8>, point: Vec<u8> },
    EcPub { curve: Curve, point: Vec<u8> },
    EdPriv { seed: [u8; 32], public: [u8; 32] },
    EdPub([u8; 32]),
    XPriv { secret: [u8; 32], public: [u8; 32] },
    XPub([u8; 32]),
}

#[derive(Clone)]
pub struct Key {
    kind: KeyType,
    extractable: bool,
    alg: Alg,
    usages: u32,
    mat: Mat,
}

impl Key {
    fn sym(&self) -> &[u8] {
        if let Mat::Sym(b) = &self.mat { b } else { &[] }
    }
    fn has_private(&self) -> bool {
        matches!(self.mat, Mat::RsaPriv(_) | Mat::EcPriv { .. } | Mat::EdPriv { .. } | Mat::XPriv { .. })
    }
    fn has_public(&self) -> bool {
        matches!(self.mat, Mat::RsaPub(_) | Mat::EcPub { .. } | Mat::EdPub(_) | Mat::XPub(_))
    }
    fn has(&self, usage: u32) -> bool {
        self.usages & usage != 0
    }
    fn has_other_than(&self, usages: u32) -> bool {
        self.usages & !usages != 0
    }
    fn ec_curve(&self) -> &'static str {
        if let Alg::Ec { curve, .. } = self.alg { curve } else { "" }
    }
    fn rsa_hash(&self) -> &'static str {
        if let Alg::Rsa { hash, .. } = self.alg { hash } else { "" }
    }
}

/// The keys of one partition (site and top-level site), by the runtime's ids.
pub type Keys = HashMap<String, Key>;

// ---- algorithm names and dictionaries ----

const TOKENS: [&str; 23] = ["AES-CBC", "AES-CTR", "AES-GCM", "AES-KW", "SHA-1", "SHA-256", "SHA-384", "SHA-512", "HMAC", "HKDF",
    "PBKDF2", "RSASSA-PKCS1-v1_5", "RSA-OAEP", "RSA-PSS", "ECDH", "ECDSA", "Ed25519", "X25519", "P-256", "P-384", "P-521",
    "Ed25519", "X25519"];

/// NormalizeToken: an algorithm or curve name, matched ignoring ASCII case.
fn normalize(s: &str) -> Option<&'static str> {
    TOKENS.iter().copied().find(|t| t.eq_ignore_ascii_case(s))
}

/// GetAlgorithmName: a string, or the `name` of an Algorithm dictionary, normalized.
fn alg_name(v: &Value) -> Result<&'static str, Rv> {
    let name = if js::is_object(v) {
        js::to_string(member(v, "name").ok_or(Rv::TypeMismatch)?)
    } else {
        js::to_string(v)
    };
    normalize(&name).ok_or(Rv::NotSupported)
}

/// Coerce's first half: a string is no dictionary (SyntaxError); a dictionary derived from
/// Algorithm needs its `name`. The members are then read with the helpers below, each failing as
/// the dictionary's Init does (TYPE_MISMATCH).
fn dict(v: &Value, with_name: bool) -> Result<&Value, Rv> {
    if !js::is_object(v) {
        return Err(Rv::Syntax);
    }
    if with_name && member(v, "name").is_none() {
        return Err(Rv::TypeMismatch);
    }
    Ok(v)
}

fn buf_member(d: &Value, name: &str) -> Result<Option<Vec<u8>>, Rv> {
    match member(d, name) {
        None => Ok(None),
        Some(v) => js::bytes(v).map(|(b, _)| Some(b)).ok_or(Rv::TypeMismatch),
    }
}

fn req_buf(d: &Value, name: &str) -> Result<Vec<u8>, Rv> {
    buf_member(d, name)?.ok_or(Rv::TypeMismatch)
}

fn int_member(d: &Value, name: &str, max: f64) -> Result<Option<u64>, Rv> {
    match member(d, name) {
        None => Ok(None),
        Some(v) => js::enforce_range(v, max).map(Some).ok_or(Rv::TypeMismatch),
    }
}

fn req_int(d: &Value, name: &str, max: f64) -> Result<u64, Rv> {
    int_member(d, name, max)?.ok_or(Rv::TypeMismatch)
}

const OCTET: f64 = 255.0;
const ULONG: f64 = 4294967295.0;

fn req_member<'a>(d: &'a Value, name: &str) -> Result<&'a Value, Rv> {
    member(d, name).ok_or(Rv::TypeMismatch)
}

/// A member that is a CryptoKey: its id.
fn req_key(d: &Value, name: &str) -> Result<String, Rv> {
    let v = req_member(d, name)?;
    v.get("$k").and_then(Value::as_str).map(str::to_string).ok_or(Rv::TypeMismatch)
}

/// GetKeyLengthForAlgorithmIfSpecified: the length of the key a derivation makes.
fn derived_key_length(alg: &Value) -> Result<Option<usize>, Rv> {
    let name = alg_name(alg).map_err(|_| Rv::Syntax)?;
    match name {
        "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" => {
            let d = dict(alg, true).map_err(|_| Rv::Syntax)?;
            let length = req_int(d, "length", ULONG).map_err(|_| Rv::Syntax)?;
            if ![128, 192, 256].contains(&length) {
                return Err(Rv::Operation);
            }
            Ok(Some(length as usize))
        }
        "HMAC" => {
            let d = dict(alg, true).map_err(|_| Rv::Syntax)?;
            let hash = req_member(d, "hash").map_err(|_| Rv::Syntax)?;
            let length = int_member(d, "length", ULONG).map_err(|_| Rv::Syntax)?;
            if let Some(l) = length {
                return Ok(Some(l as usize));
            }
            let hash = alg_name(hash).map_err(|_| Rv::Syntax)?;
            Hash::from_name(hash).map(|h| Some(h.block_bits())).ok_or(Rv::Syntax)
        }
        _ => Ok(None),
    }
}

/// MapAlgorithmNameToMechanism's names: the ones Gecko knows a PKCS #11 mechanism for.
fn has_mechanism(name: &str) -> bool {
    matches!(name, "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" | "SHA-1" | "SHA-256" | "SHA-384" | "SHA-512" | "PBKDF2"
        | "RSASSA-PKCS1-v1_5" | "RSA-OAEP" | "RSA-PSS" | "ECDH")
}

// ---- JWK ----

/// A JsonWebKey dictionary.
#[derive(Default)]
struct Jwk {
    kty: String,
    use_: Option<String>,
    key_ops: Option<Vec<String>>,
    alg: Option<String>,
    ext: Option<bool>,
    m: HashMap<&'static str, String>,
}

const JWK_STRINGS: [&str; 12] = ["crv", "x", "y", "d", "n", "e", "p", "q", "dp", "dq", "qi", "k"];

impl Jwk {
    /// The dictionary's Init from an object the runtime encoded; None where it throws.
    fn from_value(v: &Value) -> Option<Jwk> {
        if !js::is_object(v) {
            return None;
        }
        let mut j = Jwk { kty: js::to_string(member(v, "kty")?), ..Default::default() };
        j.use_ = member(v, "use").map(js::to_string);
        j.alg = member(v, "alg").map(js::to_string);
        j.ext = member(v, "ext").map(js::truthy);
        if let Some(ops) = member(v, "key_ops") {
            j.key_ops = Some(js::string_sequence(ops)?);
        }
        for name in JWK_STRINGS {
            if let Some(x) = member(v, name) {
                j.m.insert(name, js::to_string(x));
            }
        }
        Some(j)
    }

    /// Init from JSON text (an unwrapped key)
    fn from_json(text: &str) -> Option<Jwk> {
        fn encode(v: Value) -> Value {
            match v {
                Value::Object(o) => js::object_owned(o.into_iter().map(|(k, x)| (k, encode(x))).collect()),
                Value::Array(a) => Value::Array(a.into_iter().map(encode).collect()),
                x => x,
            }
        }
        Jwk::from_value(&encode(serde_json::from_str(text).ok()?))
    }

    fn get(&self, name: &str) -> Option<&String> {
        self.m.get(name)
    }

    fn bytes(&self, name: &str) -> Option<Vec<u8>> {
        js::from_jwk_base64(self.get(name)?)
    }
}

/// ImportKeyTask::JwkCompatible
fn jwk_compatible(j: &Jwk, key: &Key, extractable: bool) -> bool {
    let skip_alg = j.kty == "OKP" || (j.kty == "EC" && key.alg.name() == "ECDH");
    if !skip_alg && j.alg.as_ref().is_some_and(|a| Some(a.as_str()) != key.alg.jwk_alg()) {
        return false;
    }
    if extractable && j.ext == Some(false) {
        return false;
    }
    if let Some(ops) = &j.key_ops {
        if usage_names(key.usages).iter().any(|u| !ops.iter().any(|o| o == u)) {
            return false;
        }
    }
    true
}

/// A key as a JWK (ExportKeyTask's jwk branch): its members in the dictionary's order.
fn export_jwk(key: &Key) -> Result<Map<String, Value>, Rv> {
    let mut m = Map::new();
    let b64 = |b: &[u8]| json!(js::jwk_base64(b));
    let big = |n: &BigUint| json!(js::jwk_base64(&n.to_bytes_be()));
    let ec = |m: &mut Map<String, Value>, curve: Curve, point: &[u8]| {
        let f = curve.flen();
        m.insert("kty".into(), json!("EC"));
        m.insert("crv".into(), json!(curve.name()));
        m.insert("x".into(), b64(&point[1..1 + f]));
        m.insert("y".into(), b64(&point[1 + f..]));
    };
    match &key.mat {
        Mat::Sym(k) => {
            m.insert("kty".into(), json!("oct"));
            m.insert("k".into(), b64(k));
        }
        Mat::RsaPub(k) => {
            m.insert("kty".into(), json!("RSA"));
            m.insert("n".into(), big(k.n()));
            m.insert("e".into(), big(k.e()));
        }
        Mat::RsaPriv(k) => {
            let mut k = k.clone();
            k.precompute().map_err(|_| Rv::Operation)?;
            let (p, q) = (&k.primes()[0], &k.primes()[1]);
            let one = BigUint::from(1u8);
            let qi = k.crt_coefficient().ok_or(Rv::Operation)?;
            m.insert("kty".into(), json!("RSA"));
            m.insert("n".into(), big(k.n()));
            m.insert("e".into(), big(k.e()));
            m.insert("d".into(), big(k.d()));
            m.insert("p".into(), big(p));
            m.insert("q".into(), big(q));
            m.insert("dp".into(), big(&(k.d() % (p - &one))));
            m.insert("dq".into(), big(&(k.d() % (q - &one))));
            m.insert("qi".into(), big(&qi));
        }
        Mat::EcPub { curve, point } => ec(&mut m, *curve, point),
        Mat::EcPriv { curve, d, point } => {
            ec(&mut m, *curve, point);
            m.insert("d".into(), b64(d));
        }
        Mat::EdPub(p) | Mat::XPub(p) => {
            m.insert("kty".into(), json!("OKP"));
            m.insert("crv".into(), json!(if matches!(key.mat, Mat::EdPub(_)) { "Ed25519" } else { "X25519" }));
            m.insert("x".into(), b64(p));
        }
        Mat::EdPriv { seed: s, public: p } | Mat::XPriv { secret: s, public: p } => {
            m.insert("kty".into(), json!("OKP"));
            m.insert("crv".into(), json!(if matches!(key.mat, Mat::EdPriv { .. }) { "Ed25519" } else { "X25519" }));
            m.insert("x".into(), b64(p));
            m.insert("d".into(), b64(s));
        }
    }
    if let Some(alg) = key.alg.jwk_alg() {
        m.insert("alg".into(), json!(alg));
    }
    m.insert("ext".into(), json!(key.extractable));
    m.insert("key_ops".into(), json!(usage_names(key.usages)));
    Ok(m)
}

/// A JWK as the page reads it: WebIDL dictionary members in sorted order.
/// IndexedDB enables preserve_order globally, so sort this dictionary explicitly.
fn jwk_value(m: &Map<String, Value>) -> Value {
    let mut members = m.clone();
    members.sort_keys();
    js::object_owned(members.into_iter().collect())
}

// ---- SPKI and PKCS #8 ----

const OID_RSA: &str = "1.2.840.113549.1.1.1";
const OID_EC: &str = "1.2.840.10045.2.1";
// id-ecDH, which old versions of the spec used and Gecko still reads (PublicKeyFromSpki)
const OID_ECDH: &str = "1.3.132.112";
const OID_ED25519: &str = "1.3.101.112";
const OID_X25519: &str = "1.3.101.110";

/// The key a SubjectPublicKeyInfo holds (PublicKeyFromSpki), or Err(Data); `Err(Operation)` and
/// `Err(NotSupported)` for EC parameters Gecko checks after it.
fn public_from_spki(der: &[u8]) -> Result<Mat, Rv> {
    use der::Decode;
    let spki = spki::SubjectPublicKeyInfoRef::from_der(der).map_err(|_| Rv::Data)?;
    let oid = spki.algorithm.oid.to_string();
    let bits = spki.subject_public_key.raw_bytes();
    match oid.as_str() {
        OID_RSA => rsa_public_from_pkcs1(bits).map(Mat::RsaPub).ok_or(Rv::Data),
        OID_EC | OID_ECDH => {
            let params = spki.algorithm.parameters.ok_or(Rv::Operation)?;
            let curve_oid = params.decode_as::<der::asn1::ObjectIdentifier>().map_err(|_| Rv::Operation)?;
            let curve = Curve::from_oid(&curve_oid.to_string()).ok_or(Rv::NotSupported)?;
            let point = prims::ec_point(curve, bits).map_err(|_| Rv::Data)?;
            Ok(Mat::EcPub { curve, point })
        }
        OID_ED25519 => {
            let p: [u8; 32] = bits.try_into().map_err(|_| Rv::Data)?;
            if !prims::ed25519_valid_point(&p) {
                return Err(Rv::Data);
            }
            Ok(Mat::EdPub(p))
        }
        OID_X25519 => Ok(Mat::XPub(bits.try_into().map_err(|_| Rv::Data)?)),
        _ => Err(Rv::Data),
    }
}

fn rsa_public_from_pkcs1(der: &[u8]) -> Option<RsaPublicKey> {
    use der::Decode;
    let k = rsa::pkcs1::RsaPublicKey::from_der(der).ok()?;
    let n = BigUint::from_bytes_be(k.modulus.as_bytes());
    let e = BigUint::from_bytes_be(k.public_exponent.as_bytes());
    RsaPublicKey::new_with_max_size(n, e, 16384).ok()
}

fn rsa_private_from_parts(n: &[u8], e: &[u8], d: &[u8], p: &[u8], q: &[u8]) -> Option<RsaPrivateKey> {
    let big = BigUint::from_bytes_be;
    RsaPrivateKey::from_components(big(n), big(e), big(d), vec![big(p), big(q)]).ok()
}

/// The key a PrivateKeyInfo holds; `Err(NotSupported)` for an EC key whose curve Gecko does not
/// support, Err(Data) for anything else it cannot read.
fn private_from_pkcs8(der: &[u8]) -> Result<Mat, Rv> {
    use der::Decode;
    let info = pkcs8::PrivateKeyInfo::from_der(der).map_err(|_| Rv::Data)?;
    match info.algorithm.oid.to_string().as_str() {
        OID_RSA => {
            let k = rsa::pkcs1::RsaPrivateKey::from_der(info.private_key).map_err(|_| Rv::Data)?;
            rsa_private_from_parts(k.modulus.as_bytes(), k.public_exponent.as_bytes(), k.private_exponent.as_bytes(),
                                   k.prime1.as_bytes(), k.prime2.as_bytes())
                .map(Mat::RsaPriv)
                .ok_or(Rv::Data)
        }
        OID_EC => {
            let params = info.algorithm.parameters.ok_or(Rv::Data)?;
            let curve_oid = params.decode_as::<der::asn1::ObjectIdentifier>().map_err(|_| Rv::Data)?;
            let curve = Curve::from_oid(&curve_oid.to_string()).ok_or(Rv::NotSupported)?;
            let d = ec_private_from_pkcs8(curve, der).ok_or(Rv::Data)?;
            let point = prims::ec_public_of(curve, &d).map_err(|_| Rv::Data)?;
            Ok(Mat::EcPriv { curve, d, point })
        }
        oid @ (OID_ED25519 | OID_X25519) => {
            let inner = der::asn1::OctetStringRef::from_der(info.private_key).map_err(|_| Rv::Data)?;
            let s: [u8; 32] = inner.as_bytes().try_into().map_err(|_| Rv::Data)?;
            Ok(if oid == OID_ED25519 {
                Mat::EdPriv { seed: s, public: prims::ed25519_public_of(&s) }
            } else {
                Mat::XPriv { secret: s, public: prims::x25519_public_of(&s) }
            })
        }
        // an RSA key has no EC parameters to read (PK11_ReadRawAttribute fails)
        _ => Err(Rv::NotSupported),
    }
}

fn ec_private_from_pkcs8(curve: Curve, der: &[u8]) -> Option<Vec<u8>> {
    use pkcs8::DecodePrivateKey;
    Some(match curve {
        Curve::P256 => p256::SecretKey::from_pkcs8_der(der).ok()?.to_bytes().to_vec(),
        Curve::P384 => p384::SecretKey::from_pkcs8_der(der).ok()?.to_bytes().to_vec(),
        Curve::P521 => p521::SecretKey::from_pkcs8_der(der).ok()?.to_bytes().to_vec(),
    })
}

/// A 32-byte key in an OKP SubjectPublicKeyInfo or PrivateKeyInfo.
fn okp_der(oid_last: u8, key: &[u8; 32], private: bool) -> Vec<u8> {
    let mut out = if private {
        vec![0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, oid_last, 0x04, 0x22, 0x04, 0x20]
    } else {
        vec![0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, oid_last, 0x03, 0x21, 0x00]
    };
    out.extend_from_slice(key);
    out
}

fn to_pkcs8(mat: &Mat) -> Result<Vec<u8>, Rv> {
    use pkcs8::EncodePrivateKey;
    let doc = |r: pkcs8::Result<der::SecretDocument>| r.map(|d| d.as_bytes().to_vec()).map_err(|_| Rv::Operation);
    match mat {
        Mat::RsaPriv(k) => doc(k.to_pkcs8_der()),
        Mat::EcPriv { curve, d, .. } => match curve {
            Curve::P256 => doc(p256::SecretKey::from_slice(d).map_err(|_| Rv::Operation)?.to_pkcs8_der()),
            Curve::P384 => doc(p384::SecretKey::from_slice(d).map_err(|_| Rv::Operation)?.to_pkcs8_der()),
            Curve::P521 => doc(p521::SecretKey::from_slice(d).map_err(|_| Rv::Operation)?.to_pkcs8_der()),
        },
        Mat::EdPriv { seed, .. } => Ok(okp_der(0x70, seed, true)),
        Mat::XPriv { secret, .. } => Ok(okp_der(0x6e, secret, true)),
        _ => Err(Rv::NotSupported),
    }
}

fn to_spki(mat: &Mat) -> Result<Vec<u8>, Rv> {
    use pkcs8::EncodePublicKey;
    let doc = |r: spki::Result<der::Document>| r.map(|d| d.as_bytes().to_vec()).map_err(|_| Rv::Operation);
    match mat {
        Mat::RsaPub(k) => doc(k.to_public_key_der()),
        Mat::EcPub { curve, point } => match curve {
            Curve::P256 => doc(p256::PublicKey::from_sec1_bytes(point).map_err(|_| Rv::Operation)?.to_public_key_der()),
            Curve::P384 => doc(p384::PublicKey::from_sec1_bytes(point).map_err(|_| Rv::Operation)?.to_public_key_der()),
            Curve::P521 => doc(p521::PublicKey::from_sec1_bytes(point).map_err(|_| Rv::Operation)?.to_public_key_der()),
        },
        Mat::EdPub(p) => Ok(okp_der(0x70, p, false)),
        Mat::XPub(p) => Ok(okp_der(0x6e, p, false)),
        _ => Err(Rv::NotSupported),
    }
}

// ---- tasks ----

/// A task's end: `early` when Firefox settles the promise at the call.
type Done<T> = (bool, Result<T, Rv>);

macro_rules! early {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(rv) => return (true, Err(rv)),
        }
    };
}

/// What an operation resolves with, before its keys get ids.
enum Out {
    V(Value),
    Key(Key),
    Pair(Key, Key),
}

/// An encrypt or decrypt task's checked parameters (AesTask, AesKwTask and RsaOaepTask's
/// constructors); `run` is their DoCrypto.
enum Cipher {
    Cbc { key: Vec<u8>, iv: Vec<u8> },
    Ctr { key: Vec<u8>, counter: Vec<u8>, bits: u32 },
    Gcm { key: Vec<u8>, iv: Vec<u8>, aad: Vec<u8>, tag_bits: u32 },
    Kw { key: Vec<u8> },
    Oaep { key: Key, hash: Hash, label: Vec<u8> },
}

/// CHECK_KEY_ALGORITHM
fn check_alg(key: &Key, name: &str) -> Result<(), Rv> {
    if key.alg.name().eq_ignore_ascii_case(name) { Ok(()) } else { Err(Rv::InvalidAccess) }
}

fn aes_init(alg: &Value, key: &Key) -> Result<Cipher, Rv> {
    let name = alg_name(alg)?;
    let k = key.sym().to_vec();
    if ![16, 24, 32].contains(&k.len()) {
        return Err(Rv::Data);
    }
    match name {
        "AES-CBC" => {
            check_alg(key, "AES-CBC")?;
            let d = dict(alg, true).map_err(|_| Rv::InvalidAccess)?;
            let iv = req_buf(d, "iv").map_err(|_| Rv::InvalidAccess)?;
            if iv.len() != 16 {
                return Err(Rv::Operation);
            }
            Ok(Cipher::Cbc { key: k, iv })
        }
        "AES-CTR" => {
            check_alg(key, "AES-CTR")?;
            let d = dict(alg, true).map_err(|_| Rv::Syntax)?;
            let counter = req_buf(d, "counter").map_err(|_| Rv::Syntax)?;
            let bits = req_int(d, "length", OCTET).map_err(|_| Rv::Syntax)? as u32;
            if counter.len() != 16 {
                return Err(Rv::Operation);
            }
            Ok(Cipher::Ctr { key: k, counter, bits })
        }
        "AES-GCM" => {
            check_alg(key, "AES-GCM")?;
            let d = dict(alg, true).map_err(|_| Rv::Operation)?;
            let iv = req_buf(d, "iv").map_err(|_| Rv::Operation)?;
            let aad = buf_member(d, "additionalData").map_err(|_| Rv::Operation)?.unwrap_or_default();
            let tag_bits = int_member(d, "tagLength", OCTET).map_err(|_| Rv::Operation)?.unwrap_or(128) as u32;
            if tag_bits > 128 || !(tag_bits == 32 || tag_bits == 64 || (tag_bits >= 96 && tag_bits % 8 == 0)) {
                return Err(Rv::Operation);
            }
            Ok(Cipher::Gcm { key: k, iv, aad, tag_bits })
        }
        _ => Err(Rv::NotSupported),
    }
}

fn aes_kw_init(alg: &Value, key: &Key) -> Result<Cipher, Rv> {
    check_alg(key, "AES-KW")?;
    alg_name(alg)?;
    let k = key.sym().to_vec();
    if ![16, 24, 32].contains(&k.len()) {
        return Err(Rv::Data);
    }
    Ok(Cipher::Kw { key: k })
}

fn rsa_oaep_init(alg: &Value, key: &Key, encrypt: bool) -> Result<Cipher, Rv> {
    check_alg(key, "RSA-OAEP")?;
    if (encrypt && !key.has_public()) || (!encrypt && !key.has_private()) {
        return Err(Rv::InvalidAccess);
    }
    let mut label = vec![];
    if js::is_object(alg) {
        let d = dict(alg, true).map_err(|_| Rv::Syntax)?;
        label = buf_member(d, "label").map_err(|_| Rv::Syntax)?.unwrap_or_default();
    }
    let hash = Hash::from_name(key.rsa_hash()).ok_or(Rv::NotSupported)?;
    Ok(Cipher::Oaep { key: key.clone(), hash, label })
}

impl Cipher {
    fn init(name: &str, alg: &Value, key: &Key, encrypt: bool) -> Result<Cipher, Rv> {
        match name {
            "AES-CBC" | "AES-CTR" | "AES-GCM" => aes_init(alg, key),
            "AES-KW" => aes_kw_init(alg, key),
            _ => rsa_oaep_init(alg, key, encrypt),
        }
    }

    fn run(&self, data: &[u8], encrypt: bool) -> Result<Vec<u8>, Rv> {
        let op = |r: prims::R<Vec<u8>>| r.map_err(|_| Rv::Operation);
        match self {
            Cipher::Cbc { key, iv } => op(prims::aes_cbc(key, iv, data, encrypt)),
            Cipher::Ctr { key, counter, bits } => op(prims::aes_ctr(key, counter, *bits, data)),
            Cipher::Gcm { key, iv, aad, tag_bits } => op(prims::aes_gcm(key, iv, aad, *tag_bits as usize / 8, data, encrypt)),
            Cipher::Kw { key } => {
                if data.is_empty() || data.len() % 8 != 0 {
                    return Err(Rv::Data);
                }
                op(prims::aes_kw(key, data, encrypt))
            }
            Cipher::Oaep { key, hash, label } => match &key.mat {
                Mat::RsaPub(k) if encrypt => op(prims::rsa_oaep_encrypt(k, *hash, label, data)),
                Mat::RsaPriv(k) if !encrypt => op(prims::rsa_oaep_decrypt(k, *hash, label, data)),
                _ => Err(Rv::Operation),
            },
        }
    }
}

fn encrypt_decrypt(alg: &Value, key: &Key, data: &[u8], encrypt: bool) -> Done<Out> {
    if !key.has(if encrypt { ENCRYPT } else { DECRYPT }) {
        return (true, Err(Rv::InvalidAccess));
    }
    let name = early!(alg_name(alg));
    if !matches!(name, "AES-CBC" | "AES-CTR" | "AES-GCM" | "RSA-OAEP") {
        return (true, Err(Rv::NotSupported));
    }
    let c = early!(Cipher::init(name, alg, key, encrypt));
    (false, c.run(data, encrypt).map(|b| Out::V(js::array_buffer(&b))))
}

/// HmacTask and AsymmetricSignVerifyTask; `sig` is None to sign.
fn sign_verify(alg: &Value, key: &Key, sig: Option<&[u8]>, data: &[u8]) -> Done<Out> {
    let sign = sig.is_none();
    if !key.has(if sign { SIGN } else { VERIFY }) {
        return (true, Err(Rv::InvalidAccess));
    }
    let name = early!(alg_name(alg));
    let result = |r: Result<Vec<u8>, ()>, ok: bool| -> Result<Out, Rv> {
        if sign { r.map(|b| Out::V(js::array_buffer(&b))).map_err(|_| Rv::Operation) } else { Ok(Out::V(json!(ok))) }
    };
    match name {
        "HMAC" => {
            early!(check_alg(key, "HMAC"));
            if key.sym().is_empty() {
                return (true, Err(Rv::Data));
            }
            let Alg::Hmac { hash, .. } = key.alg else { return (true, Err(Rv::InvalidAccess)) };
            // an HMAC key of an unknown hash has no mechanism: NSS fails to import it
            let Some(h) = Hash::from_name(hash) else { return (false, Err(Rv::InvalidAccess)) };
            let mac = h.hmac(key.sym(), data);
            (false, result(Ok(mac.clone()), sig.is_some_and(|s| s == mac.as_slice())))
        }
        "RSASSA-PKCS1-v1_5" | "RSA-PSS" | "ECDSA" | "Ed25519" => {
            let mut hash_name = "";
            let mut salt = 0usize;
            match name {
                "RSASSA-PKCS1-v1_5" => {
                    early!(check_alg(key, name));
                    hash_name = key.rsa_hash();
                }
                "RSA-PSS" => {
                    early!(check_alg(key, name));
                    hash_name = key.rsa_hash();
                    if Hash::from_name(hash_name).is_none() {
                        return (true, Err(Rv::NotSupported));
                    }
                    let d = early!(dict(alg, true).map_err(|_| Rv::Syntax));
                    salt = early!(req_int(d, "saltLength", ULONG).map_err(|_| Rv::Syntax)) as usize;
                }
                "ECDSA" => {
                    early!(check_alg(key, name));
                    let d = early!(dict(alg, true).map_err(|_| Rv::Syntax));
                    let hash = early!(req_member(d, "hash").map_err(|_| Rv::Syntax));
                    hash_name = early!(alg_name(hash).map_err(|_| Rv::NotSupported));
                }
                _ => early!(check_alg(key, name)),
            }
            let hash = Hash::from_name(hash_name);
            if hash.is_none() && name != "Ed25519" {
                return (true, Err(Rv::NotSupported));
            }
            if (sign && !key.has_private()) || (!sign && !key.has_public()) {
                return (true, Err(Rv::InvalidAccess));
            }
            let h = hash.unwrap_or(Hash::Sha256);
            let s = sig.unwrap_or(&[]);
            let out = match (&key.mat, sign) {
                (Mat::RsaPriv(k), true) if name == "RSA-PSS" => result(prims::rsa_pss_sign(k, h, salt, data), false),
                (Mat::RsaPriv(k), true) => result(prims::rsa_pkcs1_sign(k, h, data), false),
                (Mat::RsaPub(k), false) if name == "RSA-PSS" => result(Err(()), prims::rsa_pss_verify(k, h, salt, s, data)),
                (Mat::RsaPub(k), false) => result(Err(()), prims::rsa_pkcs1_verify(k, h, s, data)),
                (Mat::EcPriv { curve, d, .. }, true) => result(prims::ecdsa_sign(*curve, d, h, data), false),
                (Mat::EcPub { curve, point }, false) => result(Err(()), prims::ecdsa_verify(*curve, point, h, s, data)),
                (Mat::EdPriv { seed, .. }, true) => result(Ok(prims::ed25519_sign(seed, data)), false),
                (Mat::EdPub(p), false) => result(Err(()), prims::ed25519_verify(p, s, data)),
                // a key of another kind than the algorithm's (an imported key Gecko allowed)
                _ => if sign { Err(Rv::Operation) } else { Ok(Out::V(json!(false))) },
            };
            (false, out)
        }
        _ => (true, Err(Rv::NotSupported)),
    }
}

fn digest(alg: &Value, data: &[u8]) -> Done<Out> {
    let name = early!(alg_name(alg));
    let Some(h) = Hash::from_name(name) else { return (true, Err(Rv::NotSupported)) };
    (false, Ok(Out::V(js::array_buffer(&h.digest(data)))))
}

// ---- import ----

/// What an import task's constructor settled (ImportKeyTask::Init and its subclass's Init).
struct ImportPlan {
    name: &'static str,
    // an ImportSymmetricKeyTask whatever the name (deriveKey's): another name fails in BeforeCrypto
    symmetric: bool,
    format: String,
    usages: u32,
    hash: &'static str,
    curve: Option<&'static str>,
}

enum KeyData {
    Bytes(Vec<u8>),
    Jwk(Jwk),
}

fn import_init(format: &str, alg: &Value, usages: &[String], symmetric: bool) -> Result<ImportPlan, Rv> {
    let mut bits = 0;
    for u in usages {
        bits |= usage_bit(u).ok_or(Rv::Syntax)?;
    }
    let name = alg_name(alg).map_err(|_| Rv::Data)?;
    let symmetric = symmetric || is_symmetric(name);
    let mut plan = ImportPlan { name, symmetric, format: format.to_string(), usages: bits, hash: "", curve: None };
    match name {
        _ if symmetric => {
            if format != "jwk" && format != "raw" {
                return Err(Rv::NotSupported);
            }
            if name == "HMAC" {
                let d = dict(alg, true).map_err(|_| Rv::Syntax)?;
                let hash = req_member(d, "hash").map_err(|_| Rv::Syntax)?;
                plan.hash = alg_name(hash).map_err(|_| Rv::Syntax)?;
            }
        }
        "RSASSA-PKCS1-v1_5" | "RSA-OAEP" | "RSA-PSS" => {
            let d = dict(alg, false).map_err(|_| Rv::Data)?;
            let hash = req_member(d, "hash").map_err(|_| Rv::Data)?;
            plan.hash = alg_name(hash).map_err(|_| Rv::Data)?;
            if !has_mechanism(plan.hash) {
                return Err(Rv::NotSupported);
            }
        }
        "ECDH" | "ECDSA" => {
            if format == "raw" || format == "jwk" {
                let d = dict(alg, true).map_err(|_| Rv::Syntax)?;
                let curve = member(d, "namedCurve").ok_or(Rv::Syntax)?;
                plan.curve = Some(normalize(&js::to_string(curve)).ok_or(Rv::NotSupported)?);
            }
        }
        "Ed25519" | "X25519" => {
            if format == "raw" {
                let n = alg_name(alg).map_err(|_| Rv::Syntax)?;
                plan.curve = Some(match n {
                    "Ed25519" => "Ed25519",
                    "X25519" => "X25519",
                    _ => return Err(Rv::NotSupported),
                });
            }
        }
        _ => return Err(Rv::NotSupported),
    }
    Ok(plan)
}

fn is_symmetric(name: &str) -> bool {
    matches!(name, "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" | "PBKDF2" | "HKDF" | "HMAC")
}

/// The rest of an import: BeforeCrypto for a symmetric key, DoCrypto and AfterCrypto for the others.
/// `extractable` is the page's, for the checks (`describe` makes a key made here extractable).
fn import_finish(plan: &ImportPlan, data: KeyData, extractable: bool) -> Result<Key, Rv> {
    let format = plan.format.as_str();
    let empty = Jwk::default();
    let (jwk, raw) = match &data {
        KeyData::Jwk(j) => (Some(j), &[][..]),
        KeyData::Bytes(b) => (None, b.as_slice()),
    };
    let name = plan.name;
    let mut key = Key { kind: KeyType::Secret, extractable, alg: Alg::Kdf { name }, usages: plan.usages, mat: Mat::Sym(vec![]) };
    if plan.symmetric {
        let bytes = match jwk {
            Some(j) => j.bytes("k").ok_or(Rv::Data)?,
            None => raw.to_vec(),
        };
        if bytes.is_empty() && name != "PBKDF2" && name != "HKDF" {
            return Err(Rv::Data);
        }
        let length = 8 * bytes.len() as u32;
        let use_ = jwk.and_then(|j| j.use_.as_deref());
        match name {
            "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" => {
                if key.has_other_than(ENCRYPT | DECRYPT | WRAPKEY | UNWRAPKEY) || (name == "AES-KW" && key.has_other_than(WRAPKEY | UNWRAPKEY)) {
                    return Err(Rv::Syntax);
                }
                if ![128, 192, 256].contains(&length) {
                    return Err(Rv::Data);
                }
                key.alg = Alg::Aes { name, length };
                if use_.is_some_and(|u| u != "enc") {
                    return Err(Rv::Data);
                }
            }
            "HKDF" | "PBKDF2" => {
                if key.has_other_than(DERIVEKEY | DERIVEBITS) {
                    return Err(Rv::Syntax);
                }
                if use_.is_some() {
                    return Err(Rv::Data);
                }
            }
            "HMAC" => {
                if key.has_other_than(SIGN | VERIFY) {
                    return Err(Rv::Syntax);
                }
                key.alg = Alg::Hmac { length, hash: plan.hash };
                if Hash::from_name(plan.hash).is_none() {
                    return Err(Rv::Syntax);
                }
                if use_.is_some_and(|u| u != "sig") {
                    return Err(Rv::Data);
                }
            }
            _ => return Err(Rv::NotSupported),
        }
        if key.usages == 0 {
            return Err(Rv::Syntax);
        }
        key.mat = Mat::Sym(bytes);
        if jwk.is_some_and(|j| !jwk_compatible(j, &key, extractable)) {
            return Err(Rv::Data);
        }
        return Ok(key);
    }

    let j = jwk.unwrap_or(&empty);
    let jwk_private = format == "jwk" && j.get("d").is_some();
    let jwk_public = format == "jwk" && !jwk_private;
    match name {
        "RSASSA-PKCS1-v1_5" | "RSA-OAEP" | "RSA-PSS" => {
            if format == "spki" || jwk_public {
                key.mat = if format == "spki" {
                    match public_from_spki(raw) {
                        Ok(m @ Mat::RsaPub(_)) => m,
                        _ => return Err(Rv::Data),
                    }
                } else {
                    let (n, e) = (j.bytes("n"), j.bytes("e"));
                    match (j.kty.as_str(), n, e) {
                        ("RSA", Some(n), Some(e)) => Mat::RsaPub(
                            RsaPublicKey::new_with_max_size(BigUint::from_bytes_be(&n), BigUint::from_bytes_be(&e), 16384).map_err(|_| Rv::Data)?,
                        ),
                        _ => return Err(Rv::Data),
                    }
                };
                key.kind = KeyType::Public;
            } else if format == "pkcs8" || jwk_private {
                key.mat = if format == "pkcs8" {
                    match private_from_pkcs8(raw) {
                        Ok(m @ Mat::RsaPriv(_)) => m,
                        _ => return Err(Rv::Data),
                    }
                } else {
                    let parts: Option<Vec<Vec<u8>>> = ["n", "e", "d", "p", "q", "dp", "dq", "qi"].iter().map(|m| j.bytes(m)).collect();
                    match (j.kty.as_str(), parts) {
                        ("RSA", Some(p)) => Mat::RsaPriv(rsa_private_from_parts(&p[0], &p[1], &p[2], &p[3], &p[4]).ok_or(Rv::Data)?),
                        _ => return Err(Rv::Data),
                    }
                };
                key.kind = KeyType::Private;
            } else {
                return Err(Rv::Syntax);
            }
            let (n, e) = match &key.mat {
                Mat::RsaPub(k) => (k.n().clone(), k.e().clone()),
                Mat::RsaPriv(k) => (k.n().clone(), k.e().clone()),
                _ => return Err(Rv::Data),
            };
            let public = key.kind == KeyType::Public;
            let bad = match name {
                "RSA-OAEP" => if public { key.has_other_than(ENCRYPT | WRAPKEY) } else { key.has_other_than(DECRYPT | UNWRAPKEY) },
                _ => if public { key.has_other_than(VERIFY) } else { key.has_other_than(SIGN) },
            };
            if bad || (!public && key.usages == 0) {
                return Err(Rv::Syntax);
            }
            key.alg = Alg::Rsa { name, modulus_length: (8 * prims::rsa_len(&n)) as u16, public_exponent: e.to_bytes_be(), hash: plan.hash };
        }
        "ECDH" | "ECDSA" => {
            let mut curve = plan.curve.unwrap_or("");
            if jwk_private || format == "pkcs8" {
                key.mat = if format == "pkcs8" {
                    match private_from_pkcs8(raw)? {
                        m @ Mat::EcPriv { .. } => m,
                        _ => return Err(Rv::NotSupported),
                    }
                } else {
                    ec_private_from_jwk(j).ok_or(Rv::Data)?
                };
                if let Mat::EcPriv { curve: c, .. } = &key.mat {
                    if format == "pkcs8" {
                        curve = c.name();
                    }
                }
                key.kind = KeyType::Private;
            } else if format == "raw" || format == "spki" || jwk_public {
                key.mat = match format {
                    "raw" => {
                        let c = Curve::from_name(curve).ok_or(Rv::Data)?;
                        let f = c.flen();
                        let encoding_ok = (raw.len() == 2 * f + 1 && raw[0] == 4) || (raw.len() == f + 1 && (raw[0] == 2 || raw[0] == 3));
                        if !encoding_ok {
                            return Err(Rv::Data);
                        }
                        Mat::EcPub { curve: c, point: prims::ec_point(c, raw).map_err(|_| Rv::Data)? }
                    }
                    "spki" => match public_from_spki(raw) {
                        Ok(Mat::EcPub { curve: c, point }) => {
                            curve = c.name();
                            Mat::EcPub { curve: c, point }
                        }
                        Ok(_) => return Err(Rv::Data),
                        Err(rv) => return Err(rv),
                    },
                    _ => ec_public_from_jwk(j).ok_or(Rv::Data)?,
                };
                key.kind = KeyType::Public;
            } else {
                return Err(Rv::NotSupported);
            }
            if format == "jwk" {
                let crv = normalize(j.get("crv").map(String::as_str).unwrap_or("")).ok_or(Rv::NotSupported)?;
                if crv != curve {
                    return Err(Rv::Data);
                }
            }
            let (private_allowed, public_allowed) = if name == "ECDH" { (DERIVEBITS | DERIVEKEY, 0) } else { (SIGN, VERIFY) };
            check_asymmetric_usages(&key, private_allowed, public_allowed)?;
            key.alg = Alg::Ec { name, curve };
        }
        _ => {
            // Ed25519, X25519 (ImportOKPKeyTask)
            if jwk_private || format == "pkcs8" {
                key.mat = if format == "pkcs8" {
                    match private_from_pkcs8(raw)? {
                        m @ (Mat::EdPriv { .. } | Mat::XPriv { .. }) => m,
                        _ => return Err(Rv::NotSupported),
                    }
                } else {
                    okp_from_jwk(j, true).ok_or(Rv::Data)?
                };
                key.kind = KeyType::Private;
            } else if format == "raw" || format == "spki" || jwk_public {
                key.mat = match format {
                    "raw" => {
                        let p: [u8; 32] = raw.try_into().map_err(|_| Rv::Data)?;
                        if plan.curve == Some("Ed25519") {
                            if !prims::ed25519_valid_point(&p) {
                                return Err(Rv::Data);
                            }
                            Mat::EdPub(p)
                        } else {
                            Mat::XPub(p)
                        }
                    }
                    "spki" => match public_from_spki(raw) {
                        Ok(m @ (Mat::EdPub(_) | Mat::XPub(_))) => m,
                        Ok(_) => return Err(Rv::Data),
                        Err(rv) => return Err(rv),
                    },
                    _ => okp_from_jwk(j, false).ok_or(Rv::Data)?,
                };
                key.kind = KeyType::Public;
            } else {
                return Err(Rv::NotSupported);
            }
            if format == "jwk" {
                if name == "Ed25519" && j.alg.as_ref().is_some_and(|a| a != "EdDSA" && a != "Ed25519") {
                    return Err(Rv::Data);
                }
                normalize(j.get("crv").map(String::as_str).unwrap_or("")).ok_or(Rv::NotSupported)?;
            }
            let (private_allowed, public_allowed) = if name == "X25519" { (DERIVEKEY | DERIVEBITS, 0) } else { (SIGN, VERIFY) };
            check_asymmetric_usages(&key, private_allowed, public_allowed)?;
            key.alg = Alg::Okp { name };
        }
    }
    if format == "jwk" && !jwk_compatible(j, &key, extractable) {
        return Err(Rv::Data);
    }
    Ok(key)
}

fn check_asymmetric_usages(key: &Key, private_allowed: u32, public_allowed: u32) -> Result<(), Rv> {
    let private = key.kind == KeyType::Private;
    if (private && key.has_other_than(private_allowed)) || (!private && key.has_other_than(public_allowed)) || (private && key.usages == 0) {
        return Err(Rv::Syntax);
    }
    Ok(())
}

fn ec_public_from_jwk(j: &Jwk) -> Option<Mat> {
    if j.kty != "EC" {
        return None;
    }
    let (x, y) = (j.bytes("x")?, j.bytes("y")?);
    let curve = Curve::from_name(normalize(j.get("crv")?)?)?;
    if x.len() != y.len() {
        return None;
    }
    let mut point = vec![4];
    point.extend(x);
    point.extend(y);
    Some(Mat::EcPub { curve, point: prims::ec_point(curve, &point).ok()? })
}

fn ec_private_from_jwk(j: &Jwk) -> Option<Mat> {
    let Mat::EcPub { curve, point } = ec_public_from_jwk(j)? else { return None };
    let mut d = j.bytes("d")?;
    if d.len() > curve.flen() {
        return None;
    }
    while d.len() < curve.flen() {
        d.insert(0, 0);
    }
    prims::ec_public_of(curve, &d).ok()?;
    Some(Mat::EcPriv { curve, d, point })
}

fn okp_from_jwk(j: &Jwk, private: bool) -> Option<Mat> {
    if j.kty != "OKP" {
        return None;
    }
    let crv = normalize(j.get("crv")?)?;
    let x: [u8; 32] = j.bytes("x")?.try_into().ok()?;
    if !private {
        return match crv {
            "Ed25519" if prims::ed25519_valid_point(&x) => Some(Mat::EdPub(x)),
            "X25519" => Some(Mat::XPub(x)),
            _ => None,
        };
    }
    let d: [u8; 32] = j.bytes("d")?.try_into().ok()?;
    match crv {
        "Ed25519" => Some(Mat::EdPriv { seed: d, public: x }),
        "X25519" => Some(Mat::XPriv { secret: d, public: x }),
        _ => None,
    }
}

/// The key data of importKey (ImportKeyTask::SetKeyData): a buffer, or a JWK dictionary.
fn key_data_of(v: &Value) -> Result<KeyData, Rv> {
    if let Some((b, _)) = js::bytes(v) {
        return Ok(KeyData::Bytes(b));
    }
    Jwk::from_value(v).map(KeyData::Jwk).ok_or(Rv::Data)
}

fn import_key(format: &str, data: &Value, alg: &Value, extractable: bool, usages: &[String]) -> Done<Out> {
    if !matches!(format, "raw" | "spki" | "pkcs8" | "jwk") || usages.iter().any(|u| usage_bit(u).is_none()) {
        return (true, Err(Rv::Syntax));
    }
    let name = early!(alg_name(alg));
    if !matches!(name, "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" | "PBKDF2" | "HKDF" | "HMAC" | "RSASSA-PKCS1-v1_5" | "RSA-OAEP"
        | "RSA-PSS" | "ECDH" | "ECDSA" | "X25519" | "Ed25519")
    {
        return (true, Err(Rv::NotSupported));
    }
    let plan = early!(import_init(format, alg, usages, false));
    let data = early!(key_data_of(data));
    let symmetric = is_symmetric(name);
    if matches!(data, KeyData::Jwk(_)) && format != "jwk" && (symmetric || name.starts_with("RSA")) {
        return (true, Err(Rv::Syntax));
    }
    // a symmetric key is imported on the main thread (mEarlyComplete), the others on the crypto thread
    (symmetric, import_finish(&plan, data, extractable).map(Out::Key))
}

// ---- export ----

/// ExportKeyTask's DoCrypto: the bytes of a raw, spki or pkcs8 export, or a JWK.
fn export(format: &str, key: &Key) -> Result<Result<Vec<u8>, Map<String, Value>>, Rv> {
    match format {
        "raw" => match &key.mat {
            Mat::EcPub { point, .. } => Ok(Ok(point.clone())),
            Mat::EdPub(p) | Mat::XPub(p) => Ok(Ok(p.to_vec())),
            _ if key.sym().is_empty() => Err(Rv::NotSupported),
            _ => Ok(Ok(key.sym().to_vec())),
        },
        "pkcs8" => to_pkcs8(&key.mat).map(Ok),
        "spki" => to_spki(&key.mat).map(Ok),
        "jwk" => export_jwk(key).map(Err),
        _ => Err(Rv::Syntax),
    }
}

fn exportable(name: &str) -> bool {
    matches!(name, "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" | "PBKDF2" | "HMAC" | "RSASSA-PKCS1-v1_5" | "RSA-OAEP" | "RSA-PSS"
        | "ECDSA" | "ECDH" | "Ed25519" | "X25519")
}

fn export_key(format: &str, key: &Key) -> Done<Out> {
    if !matches!(format, "raw" | "spki" | "pkcs8" | "jwk") {
        return (true, Err(Rv::Syntax));
    }
    if !key.extractable {
        return (true, Err(Rv::InvalidAccess));
    }
    if !exportable(key.alg.name()) {
        return (true, Err(Rv::NotSupported));
    }
    (false, export(format, key).map(|r| Out::V(match r {
        Ok(b) => js::array_buffer(&b),
        Err(m) => jwk_value(&m),
    })))
}

// ---- generate ----

fn generate_key(alg: &Value, extractable: bool, usages: &[String]) -> Done<Out> {
    if usages.iter().any(|u| usage_bit(u).is_none()) {
        return (true, Err(Rv::Syntax));
    }
    let name = early!(alg_name(alg));
    // AddAllowedUsageIntersecting over the usages, for one key of the pair (`mask`)
    let usage_bits = |mask: u32| -> Result<u32, Rv> {
        let mut bits = 0;
        for u in usages {
            let b = usage_bit(u).ok_or(Rv::Syntax)?;
            if b & allowed_usages(name) != b {
                return Err(Rv::Syntax);
            }
            bits |= b & mask;
        }
        Ok(bits)
    };
    match name {
        "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" | "HMAC" => {
            let (alg_out, length) = if name == "HMAC" {
                let d = early!(dict(alg, true).map_err(|_| Rv::Syntax));
                let hash = early!(req_member(d, "hash").map_err(|_| Rv::Syntax));
                let length = early!(int_member(d, "length", ULONG).map_err(|_| Rv::Syntax));
                let hash = early!(alg_name(hash));
                let length = length.map(|l| l as usize).unwrap_or_else(|| Hash::from_name(hash).map_or(0, Hash::block_bits));
                if length == 0 {
                    return (true, Err(Rv::Data));
                }
                (Alg::Hmac { length: length as u32, hash }, length)
            } else {
                let length = early!(derived_key_length(alg)).unwrap_or(0);
                (Alg::Aes { name, length: length as u32 }, length)
            };
            let bits = early!(usage_bits(u32::MAX));
            if bits == 0 {
                return (true, Err(Rv::Syntax));
            }
            if let Alg::Hmac { hash, .. } = alg_out {
                // no mechanism for an HMAC of another hash: PK11_KeyGen fails
                if Hash::from_name(hash).is_none() || length / 8 == 0 {
                    return (false, Err(Rv::Unknown));
                }
            }
            let key = Key { kind: KeyType::Secret, extractable, alg: alg_out, usages: bits, mat: Mat::Sym(prims::random(length / 8)) };
            (false, Ok(Out::Key(key)))
        }
        "RSASSA-PKCS1-v1_5" | "RSA-OAEP" | "RSA-PSS" | "ECDH" | "ECDSA" | "Ed25519" | "X25519" => {
            enum Gen {
                Rsa(usize, u64),
                Ec(&'static str),
                Okp,
            }
            let (alg_out, how) = match name {
                "RSASSA-PKCS1-v1_5" | "RSA-OAEP" | "RSA-PSS" => {
                    let d = early!(dict(alg, true).map_err(|_| Rv::Syntax));
                    let modulus = early!(req_int(d, "modulusLength", ULONG).map_err(|_| Rv::Syntax));
                    let exponent = early!(req_member(d, "publicExponent").map_err(|_| Rv::Syntax));
                    let exponent = match js::bytes(exponent) {
                        Some((b, "Uint8Array")) => b,
                        _ => return (true, Err(Rv::Syntax)),
                    };
                    let hash = early!(req_member(d, "hash").map_err(|_| Rv::Syntax));
                    let hash = early!(alg_name(hash));
                    if exponent.len() > 8 {
                        return (true, Err(Rv::InvalidAccess));
                    }
                    let e = exponent.iter().fold(0u64, |acc, b| (acc << 8) | *b as u64);
                    (Alg::Rsa { name, modulus_length: modulus as u16, public_exponent: exponent, hash }, Gen::Rsa(modulus as usize, e))
                }
                "ECDH" | "ECDSA" => {
                    let d = early!(dict(alg, true).map_err(|_| Rv::Syntax));
                    let curve = early!(req_member(d, "namedCurve").map_err(|_| Rv::Syntax));
                    let curve = early!(normalize(&js::to_string(curve)).ok_or(Rv::NotSupported));
                    (Alg::Ec { name, curve }, Gen::Ec(curve))
                }
                _ => (Alg::Okp { name }, Gen::Okp),
            };
            let (private_mask, public_mask) = match name {
                "RSA-OAEP" => (DECRYPT | UNWRAPKEY, ENCRYPT | WRAPKEY),
                "ECDH" | "X25519" => (DERIVEKEY | DERIVEBITS, 0),
                _ => (SIGN, VERIFY),
            };
            let private_usages = early!(usage_bits(private_mask));
            let public_usages = early!(usage_bits(public_mask));
            // DoCrypto: the key pair, then the check that the private key has a usage
            let (private, public) = match how {
                Gen::Rsa(bits, e) => match prims::rsa_generate(bits, e) {
                    Ok(k) => {
                        let p = k.to_public_key();
                        (Mat::RsaPriv(k), Mat::RsaPub(p))
                    }
                    Err(_) => return (false, Err(Rv::Operation)),
                },
                Gen::Ec(curve) => match Curve::from_name(curve) {
                    Some(c) => {
                        let (d, point) = prims::ec_generate(c);
                        (Mat::EcPriv { curve: c, d, point: point.clone() }, Mat::EcPub { curve: c, point })
                    }
                    None if curve == "Ed25519" || curve == "X25519" => return (false, Err(Rv::Operation)),
                    None => return (false, Err(Rv::Unknown)),
                },
                Gen::Okp => {
                    let s: [u8; 32] = prims::random(32).try_into().unwrap();
                    if name == "Ed25519" {
                        let p = prims::ed25519_public_of(&s);
                        (Mat::EdPriv { seed: s, public: p }, Mat::EdPub(p))
                    } else {
                        let p = prims::x25519_public_of(&s);
                        (Mat::XPriv { secret: s, public: p }, Mat::XPub(p))
                    }
                }
            };
            if private_usages == 0 {
                return (false, Err(Rv::Syntax));
            }
            let private = Key { kind: KeyType::Private, extractable, alg: alg_out.clone(), usages: private_usages, mat: private };
            let public = Key { kind: KeyType::Public, extractable: true, alg: alg_out, usages: public_usages, mat: public };
            (false, Ok(Out::Pair(private, public)))
        }
        _ => (true, Err(Rv::NotSupported)),
    }
}

// ---- derive ----

/// A derivation's checked parameters (the DeriveBitsTasks' Init); `run` is their DoCrypto.
enum Derive {
    Pbkdf2 { password: Vec<u8>, salt: Vec<u8>, iterations: u32, hash: Hash, bytes: usize },
    Hkdf { ikm: Vec<u8>, salt: Vec<u8>, info: Vec<u8>, hash: Hash, bytes: usize },
    Ecdh { private: Key, public: Key, bits: Option<usize> },
    X25519 { private: Key, public: Key, bytes: Option<usize> },
}

fn kdf_hash(d: &Value) -> Result<Hash, Rv> {
    let hash = alg_name(req_member(d, "hash")?)?;
    Hash::from_name(hash).ok_or(Rv::NotSupported)
}

/// `length` is the bits asked for, None for null.
fn derive_init(name: &str, alg: &Value, key: &Key, length: Option<usize>, keys: &Keys) -> Result<Derive, Rv> {
    match name {
        "PBKDF2" => {
            check_alg(key, "PBKDF2")?;
            let d = dict(alg, true).map_err(|_| Rv::Syntax)?;
            let salt = req_buf(d, "salt").map_err(|_| Rv::Syntax)?;
            let iterations = req_int(d, "iterations", ULONG).map_err(|_| Rv::Syntax)? as u32;
            req_member(d, "hash").map_err(|_| Rv::Syntax)?;
            let length = match length {
                Some(l) if l % 8 == 0 => l,
                _ => return Err(Rv::Operation),
            };
            let hash = kdf_hash(d)?;
            Ok(Derive::Pbkdf2 { password: key.sym().to_vec(), salt, iterations, hash, bytes: length / 8 })
        }
        "HKDF" => {
            check_alg(key, "HKDF")?;
            let d = dict(alg, true).map_err(|_| Rv::TypeMismatch)?;
            req_member(d, "hash").map_err(|_| Rv::TypeMismatch)?;
            let salt = req_buf(d, "salt").map_err(|_| Rv::TypeMismatch)?;
            let info = req_buf(d, "info").map_err(|_| Rv::TypeMismatch)?;
            let length = match length {
                Some(l) if l % 8 == 0 => l,
                _ => return Err(Rv::Operation),
            };
            let hash = kdf_hash(d)?;
            Ok(Derive::Hkdf { ikm: key.sym().to_vec(), salt, info, hash, bytes: length / 8 })
        }
        "ECDH" => {
            check_alg(key, "ECDH")?;
            if !key.has_private() {
                return Err(Rv::InvalidAccess);
            }
            let d = dict(alg, true)?;
            let public = keys.get(&req_key(d, "public")?).ok_or(Rv::TypeMismatch)?;
            if !public.has_public() {
                return Err(Rv::InvalidAccess);
            }
            check_alg(public, "ECDH")?;
            if key.ec_curve() != public.ec_curve() {
                return Err(Rv::InvalidAccess);
            }
            Ok(Derive::Ecdh { private: key.clone(), public: public.clone(), bits: length })
        }
        _ => {
            check_alg(key, "X25519")?;
            if !key.has_private() {
                return Err(Rv::InvalidAccess);
            }
            if length.is_some_and(|l| l % 8 != 0) {
                return Err(Rv::Data);
            }
            let d = dict(alg, true)?;
            let public = keys.get(&req_key(d, "public")?).ok_or(Rv::TypeMismatch)?;
            check_alg(public, "X25519")?;
            if !public.has_public() {
                return Err(Rv::InvalidAccess);
            }
            Ok(Derive::X25519 { private: key.clone(), public: public.clone(), bytes: length.map(|l| l / 8) })
        }
    }
}

impl Derive {
    fn run(&self) -> Result<Vec<u8>, Rv> {
        match self {
            Derive::Pbkdf2 { password, salt, iterations, hash, bytes } => {
                if *iterations == 0 {
                    return Err(Rv::Operation);
                }
                Ok(if *bytes == 0 { vec![] } else { hash.pbkdf2(password, salt, *iterations, *bytes) })
            }
            Derive::Hkdf { ikm, salt, info, hash, bytes } => {
                if *bytes == 0 {
                    return Ok(vec![]);
                }
                hash.hkdf(ikm, salt, info, *bytes).map_err(|_| Rv::Operation)
            }
            Derive::Ecdh { private, public, bits } => {
                let (Mat::EcPriv { curve, d, .. }, Mat::EcPub { point, .. }) = (&private.mat, &public.mat) else { return Err(Rv::Operation) };
                let mut out = prims::ecdh(*curve, d, point).map_err(|_| Rv::Operation)?;
                if let Some(bits) = bits {
                    let bytes = bits.div_ceil(8);
                    if bytes > out.len() {
                        return Err(Rv::Operation);
                    }
                    out.truncate(bytes);
                    if bits % 8 != 0 {
                        *out.last_mut().unwrap() &= 0xffu8 << (8 - bits % 8);
                    }
                }
                Ok(out)
            }
            Derive::X25519 { private, public, bytes } => {
                let (Mat::XPriv { secret, .. }, Mat::XPub(p)) = (&private.mat, &public.mat) else { return Err(Rv::Operation) };
                let mut out = prims::x25519(secret, p).map_err(|_| Rv::Operation)?;
                if let Some(b) = bytes {
                    if *b > out.len() {
                        return Err(Rv::Operation);
                    }
                    out.truncate(*b);
                }
                Ok(out)
            }
        }
    }
}

fn derive_bits(alg: &Value, key: &Key, length: Option<usize>, keys: &Keys) -> Done<Out> {
    if !key.has(DERIVEBITS) {
        return (true, Err(Rv::InvalidAccess));
    }
    let name = early!(alg_name(alg));
    if !matches!(name, "PBKDF2" | "ECDH" | "HKDF" | "X25519") {
        return (true, Err(Rv::NotSupported));
    }
    let task = early!(derive_init(name, alg, key, length, keys));
    (false, task.run().map(|b| Out::V(js::array_buffer(&b))))
}

fn derive_key(alg: &Value, key: &Key, target: &Value, extractable: bool, usages: &[String], keys: &Keys) -> Done<Out> {
    if !key.has(DERIVEKEY) {
        return (true, Err(Rv::InvalidAccess));
    }
    if usages.iter().any(|u| usage_bit(u).is_none()) {
        return (true, Err(Rv::Syntax));
    }
    let name = early!(alg_name(alg));
    let length = match name {
        "HKDF" | "PBKDF2" => Some(early!(derived_key_length(target).and_then(|l| l.ok_or(Rv::NotSupported)))),
        "ECDH" | "X25519" => early!(derived_key_length(target)),
        _ => return (true, Err(Rv::NotSupported)),
    };
    let task = early!(derive_init(name, alg, key, length, keys));
    // the import of the bits (an ImportSymmetricKeyTask made at the call, dispatched once they are
    // derived: its errors come later too)
    let plan = import_init("raw", target, usages, true);
    let bits = match task.run() {
        Ok(b) => b,
        Err(rv) => return (false, Err(rv)),
    };
    let plan = match plan {
        Ok(p) => p,
        Err(rv) => return (false, Err(rv)),
    };
    (false, import_finish(&plan, KeyData::Bytes(bits), extractable).map(Out::Key))
}

// ---- wrap and unwrap ----

fn wrap_cipher_name(alg: &Value) -> Result<&'static str, Rv> {
    let name = alg_name(alg)?;
    if matches!(name, "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" | "RSA-OAEP") { Ok(name) } else { Err(Rv::NotSupported) }
}

fn wrap_key(format: &str, key: &Key, wrapping: &Key, alg: &Value) -> Done<Out> {
    if !matches!(format, "raw" | "spki" | "pkcs8" | "jwk") {
        return (true, Err(Rv::Syntax));
    }
    if !wrapping.has(WRAPKEY) || !key.extractable {
        return (true, Err(Rv::InvalidAccess));
    }
    let name = early!(wrap_cipher_name(alg));
    // the encryption task is made at the call; its errors show when it runs, after the export
    let cipher = Cipher::init(name, alg, wrapping, true);
    let data = match export(format, key) {
        Ok(Ok(b)) => b,
        Ok(Err(m)) => Value::Object(m).to_string().into_bytes(),
        Err(rv) => return (false, Err(rv)),
    };
    let cipher = match cipher {
        Ok(c) => c,
        Err(rv) => return (false, Err(rv)),
    };
    (false, cipher.run(&data, true).map(|b| Out::V(js::array_buffer(&b))))
}

#[allow(clippy::too_many_arguments)]
fn unwrap_key(format: &str, wrapped: &[u8], unwrapping: &Key, alg: &Value, key_alg: &Value, extractable: bool, usages: &[String]) -> Done<Out> {
    if !unwrapping.has(UNWRAPKEY) {
        return (true, Err(Rv::InvalidAccess));
    }
    if usages.iter().any(|u| usage_bit(u).is_none()) {
        return (true, Err(Rv::Syntax));
    }
    let key_name = early!(alg_name(key_alg));
    if !matches!(key_name, "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" | "HKDF" | "HMAC" | "RSASSA-PKCS1-v1_5" | "RSA-OAEP"
        | "RSA-PSS" | "ECDH" | "ECDSA" | "Ed25519" | "X25519")
    {
        return (true, Err(Rv::NotSupported));
    }
    let plan = import_init(format, key_alg, usages, false);
    let name = early!(wrap_cipher_name(alg));
    let cipher = early!(Cipher::init(name, alg, unwrapping, false));
    let bytes = match cipher.run(wrapped, false) {
        Ok(b) => b,
        Err(rv) => return (false, Err(rv)),
    };
    // SetKeyDataMaybeParseJWK: a parse error replaces the constructor's
    let data = if format == "jwk" {
        match std::str::from_utf8(&bytes).ok().and_then(Jwk::from_json) {
            Some(j) => Ok(KeyData::Jwk(j)),
            None => Err(Rv::Data),
        }
    } else {
        Ok(KeyData::Bytes(bytes))
    };
    let (plan, data) = match (plan, data) {
        (_, Err(rv)) | (Err(rv), _) => return (false, Err(rv)),
        (Ok(p), Ok(d)) => (p, d),
    };
    (false, import_finish(&plan, data, extractable).map(Out::Key))
}

// ---- requests ----

/// A key for the runtime: kept under `id`, described as crypto_bridge.js's `describe` does; with
/// its JWK as the material for a key made here.
fn describe(mut key: Key, id: &str, made: bool, keys: &mut Keys) -> Value {
    if made {
        key.extractable = true;
    }
    let material = if made {
        let jwk = export_jwk(&key).map(|m| jwk_value(&m)).unwrap_or(Value::Null);
        json!({ "format": "jwk", "data": jwk, "algorithm": key.alg.value(), "extractable": true, "usages": usage_names(key.usages) })
    } else {
        Value::Null
    };
    let out = json!({ "$key": {
        "id": id,
        "type": match key.kind { KeyType::Secret => "secret", KeyType::Public => "public", KeyType::Private => "private" },
        "extractable": key.extractable,
        "algorithm": key.alg.value(),
        "usages": usage_names(key.usages),
        "material": material,
    }});
    keys.insert(id.to_string(), key);
    out
}

/// A key imported again from the material the runtime kept: the page's importKey arguments, or a
/// made key's JWK.
fn import_material(m: &Value) -> Option<Key> {
    let format = m.get("format")?.as_str()?;
    let usages = js::string_sequence(m.get("usages")?)?;
    let extractable = m.get("extractable").is_some_and(js::truthy);
    let (_, r) = import_key(format, m.get("data")?, m.get("algorithm")?, extractable, &usages);
    match r {
        Ok(Out::Key(k)) => Some(k),
        _ => None,
    }
}

fn usages_arg(v: Option<&Value>) -> Vec<String> {
    v.and_then(js::string_sequence).unwrap_or_default()
}

/// One request of the runtime's (crypto_bridge.js's protocol); its `ok` value.
pub fn handle(req: &Value, keys: &mut Keys) -> Result<Value, String> {
    let call = req.get("call").and_then(Value::as_str).unwrap_or("");
    match call {
        "token" => return Ok(json!(uuid())),
        "forget" => {
            keys.clear();
            return Ok(Value::Null);
        }
        _ => {}
    }
    if let Some(materials) = req.get("materials").and_then(Value::as_object) {
        for (id, m) in materials {
            if !keys.contains_key(id) {
                if let Some(k) = import_material(m) {
                    keys.insert(id.clone(), k);
                }
            }
        }
    }
    let missing: Vec<&str> = req.get("keys").and_then(Value::as_array).into_iter().flatten()
        .filter_map(Value::as_str).filter(|id| !keys.contains_key(*id)).collect();
    if !missing.is_empty() {
        return Ok(json!({ "missing": missing }));
    }
    let args: Vec<Value> = req.get("args").and_then(Value::as_array).cloned().unwrap_or_default();
    let undefined = json!({ "$u": 1 });
    let arg = |i: usize| args.get(i).unwrap_or(&undefined);
    let key = |i: usize| -> Result<Key, String> {
        arg(i).get("$k").and_then(Value::as_str).and_then(|id| keys.get(id)).cloned().ok_or_else(|| format!("argument {i} is not a key"))
    };
    let bytes = |i: usize| -> Result<Vec<u8>, String> { js::bytes(arg(i)).map(|(b, _)| b).ok_or_else(|| format!("argument {i} is not a buffer")) };
    let string = |i: usize| js::to_string(arg(i));
    let flag = |i: usize| js::truthy(arg(i));
    let (early, result) = match call {
        "encrypt" | "decrypt" => encrypt_decrypt(arg(0), &key(1)?, &bytes(2)?, call == "encrypt"),
        "sign" => sign_verify(arg(0), &key(1)?, None, &bytes(2)?),
        "verify" => sign_verify(arg(0), &key(1)?, Some(&bytes(2)?), &bytes(3)?),
        "digest" => digest(arg(0), &bytes(1)?),
        "importKey" => import_key(&string(0), arg(1), arg(2), flag(3), &usages_arg(args.get(4))),
        "exportKey" => export_key(&string(0), &key(1)?),
        "generateKey" => generate_key(arg(0), flag(1), &usages_arg(args.get(2))),
        "deriveBits" => {
            let length = if arg(2).is_null() || js::is_undefined(arg(2)) { None } else { Some(js::unsigned_long(arg(2)) as usize) };
            derive_bits(arg(0), &key(1)?, length, keys)
        }
        "deriveKey" => derive_key(arg(0), &key(1)?, arg(2), flag(3), &usages_arg(args.get(4)), keys),
        "wrapKey" => wrap_key(&string(0), &key(1)?, &key(2)?, arg(3)),
        "unwrapKey" => unwrap_key(&string(0), &bytes(1)?, &key(2)?, arg(3), arg(4), flag(5), &usages_arg(args.get(6))),
        _ => return Err(format!("unknown crypto call {call}")),
    };
    let new_ids: Vec<String> = req.get("newIds").and_then(Value::as_array).into_iter().flatten()
        .filter_map(Value::as_str).map(str::to_string).collect();
    let id = |i: usize| new_ids.get(i).cloned().unwrap_or_default();
    let made = matches!(call, "generateKey" | "deriveKey" | "unwrapKey");
    Ok(match result {
        Err(rv) => json!({ "reject": rv.reject(), "early": early }),
        Ok(Out::V(v)) => json!({ "result": v, "early": early }),
        Ok(Out::Key(k)) => json!({ "result": describe(k, &id(0), made, keys), "early": early }),
        // CryptoKeyPair's members in the dictionary's order
        Ok(Out::Pair(private, public)) => {
            let private = describe(private, &id(0), true, keys);
            let public = describe(public, &id(1), true, keys);
            json!({ "result": js::object(vec![("privateKey", private), ("publicKey", public)]), "early": early })
        }
    })
}

/// A version 4 UUID (RFC 4122), as crypto.randomUUID makes.
fn uuid() -> String {
    let mut b = prims::random(16);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}
