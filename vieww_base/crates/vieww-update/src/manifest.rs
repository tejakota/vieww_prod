//! The signed update manifest, and the parser that reads one.
//!
//! # The format
//!
//! ```json
//! {
//!   "format": "vieww.update/1",
//!   "version": "1.4.0",
//!   "notes": "Fixes the caret blink on Wayland.",
//!   "assets": [
//!     {
//!       "name": "viewwstudio-linux-x86_64.tar.gz",
//!       "url": "http://updates.example.test/viewwstudio-linux-x86_64.tar.gz",
//!       "sha256": "b17a…",
//!       "ed25519_sig": "9d61…"
//!     }
//!   ]
//! }
//! ```
//!
//! # What is signed, and why that is enough
//!
//! Each asset's `ed25519_sig` is a signature **over the 32 raw bytes of its
//! `sha256` digest**, made with the publisher's seed. The public key is not
//! in the manifest — it is pinned in the application at build time
//! ([`Updater::new`](crate::Updater::new)), which is the trust root. That
//! split is what makes the rest of the manifest's plain fields safe to
//! leave unsigned:
//!
//! * A tampered `sha256` no longer matches the asset bytes → the download
//!   is refused after one buffer's worth of streaming.
//! * A swapped `sha256`+`ed25519_sig` pair from a *different, genuinely
//!   signed* asset → the digest verifies, but then the downloaded bytes
//!   hash to a different value and the apply is refused. A publisher can
//!   only ever ship assets they signed.
//! * A tampered `version` or `notes` can change what the user is *told*,
//!   not what runs — and a lowered `version` is refused outright by the
//!   updater's comparison, so the worst case is an upgrade prompt with
//!   wrong prose, not a downgrade.
//!
//! Signing the digest rather than the file is the standard construction
//! (it is what a detached signature over a checksums file is, inlined per
//! asset): the verifier's expensive work stays hashing the download, and
//! the cryptographic work stays over 32 bytes.
//!
//! # The parser
//!
//! Hand-rolled, like the repo's `json.rs` in `vieww-foundation`, because
//! this crate has no dependencies and the manifest is one fixed shape.
//! It is deliberately *strict*: unknown keys are refused rather than
//! ignored (an update feed that grows a field is a new `format` version,
//! and an old client must say so rather than half-read it), and every
//! string is unescaped for `\\`, `\"` and `\n` only — a manifest carrying
//! anything else in an escape is not one this code wrote.

use std::fmt;

use crate::ed25519;
use crate::sha256::{unhex32, unhex64};
use crate::version::Version;

/// The `format` string this code reads. A manifest carrying anything else
/// is a future (or past) format, and upgrading to it is a code change
/// rather than a hopeful parse.
pub const FORMAT: &str = "vieww.update/1";

/// One downloadable artifact named by a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// The file name, as it will be staged and applied.
    pub name: String,
    /// Where it is downloaded from. Any scheme the [`Fetch`](crate::Fetch)
    /// implementation understands; the built-in [`HttpFetch`](crate::HttpFetch)
    /// speaks `http://` only — see its docs for the honest reason.
    pub url: String,
    /// The SHA-256 of the asset bytes, as 32 raw bytes.
    pub sha256: [u8; 32],
    /// The publisher's Ed25519 signature over those 32 bytes.
    pub ed25519_sig: [u8; 64],
}

impl Asset {
    /// Whether this asset's signature covers its declared digest under `pk`.
    ///
    /// Checked at *manifest* time as well as after the download: catching a
    /// bad manifest before any bytes are fetched is one round trip saved,
    /// and catching a forged one at all is the entire point.
    #[must_use]
    pub fn signature_is_valid(&self, pk: &[u8; 32]) -> bool {
        ed25519::verify(pk, &self.sha256, &self.ed25519_sig)
    }
}

/// A parsed, verified-as-shapely update manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub notes: String,
    pub assets: Vec<Asset>,
}

/// Why a manifest could not be parsed. Deliberately a plain string-carrying
/// enum rather than one variant per lexical disappointment: every caller
/// does the same thing (refuse the update, show the reason), and a parser
/// error taxonomy nobody branches on is ceremony.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError(pub String);

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the update manifest is not valid: {}", self.0)
    }
}

impl std::error::Error for ManifestError {}

impl Manifest {
    /// Verify every asset's signature under `pk`, so a download is only
    /// ever attempted for a manifest that already checks out.
    ///
    /// # Errors
    ///
    /// [`ManifestError`] naming the first asset whose signature does not
    /// verify — a partial refusal, not a partial acceptance.
    pub fn verify(&self, pk: &[u8; 32]) -> Result<(), ManifestError> {
        for asset in &self.assets {
            if !asset.signature_is_valid(pk) {
                return Err(ManifestError(format!(
                    "asset '{}' has a signature that does not verify under the pinned key",
                    asset.name
                )));
            }
        }
        Ok(())
    }

    /// The manifest as the JSON this module parses — the writer a release
    /// pipeline uses to produce one. Unsigned by construction: filling in
    /// `ed25519_sig` is the publisher's step (see
    /// [`sign_for_release`](Self::sign_for_release)).
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&key_str("format", FORMAT));
        out.push_str(&key_str("version", &self.version.to_string()));
        out.push_str(&key_str("notes", &self.notes));
        out.push_str("  \"assets\": [");
        for (i, asset) in self.assets.iter().enumerate() {
            out.push_str(if i == 0 { "\n" } else { ",\n" });
            out.push_str("    {\n");
            out.push_str(&key_str("name", &asset.name));
            out.push_str(&key_str("url", &asset.url));
            out.push_str(&key_str("sha256", &crate::sha256::hex(&asset.sha256)));
            out.push_str(&key_str_last(
                "ed25519_sig",
                &crate::sha256::hex(&asset.ed25519_sig),
            ));
            out.push_str("    }");
        }
        if !self.assets.is_empty() {
            out.push('\n');
        }
        out.push_str("  ]\n}\n");
        out
    }

    /// Sign every asset's digest with `seed` and return the manifest that
    /// carries those signatures — the publisher's half of the channel.
    /// `vieww-build`'s manifest writer emits exactly the shape this
    /// finishes.
    #[must_use]
    pub fn sign_for_release(&self, seed: &[u8; 32]) -> Self {
        let mut signed = self.clone();
        for asset in &mut signed.assets {
            asset.ed25519_sig = ed25519::sign(seed, &asset.sha256);
        }
        signed
    }
}

/// `"key": "value",\n` with the value escaped — the shared line shape of
/// [`Manifest::to_json`], at the manifest's two-space indent. Every use is
/// a key with a key after it; the last key in an object is
/// [`key_str_last`], because the parser is strict and a trailing comma in
/// the writer would make the writer's own output refuse to parse.
fn key_str(key: &str, value: &str) -> String {
    format!("  \"{}\": \"{}\",\n", key, escape(value))
}

/// [`key_str`] without the trailing comma — the last line of an object.
fn key_str_last(key: &str, value: &str) -> String {
    format!("  \"{}\": \"{}\"\n", key, escape(value))
}

/// The escapes the writer emits and the reader accepts, and no others.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// Parse a manifest. Strict about the shape (see the module docs for why
/// unknown keys are errors, not noise).
///
/// # Errors
///
/// [`ManifestError`] naming the first thing that is not as the format says.
pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
    let mut p = P {
        text: text.as_bytes(),
        at: 0,
    };
    p.ws();
    let mut version = None;
    let mut notes = String::new();
    let mut assets = None;
    p.object(|key, value| {
        match key {
            "format" => {
                let fmt = value
                    .as_str()
                    .ok_or_else(|| err("format must be a string"))?;
                if fmt != FORMAT {
                    return Err(err(format!(
                        "format is '{fmt}', this updater reads '{FORMAT}' — a newer (or older) \
                         manifest needs a newer (or older) updater"
                    )));
                }
            }
            "version" => {
                let raw = value
                    .as_str()
                    .ok_or_else(|| err("version must be a string"))?;
                version = Some(
                    Version::parse(raw)
                        .ok_or_else(|| err(format!("version '{raw}' is not major.minor.patch")))?,
                );
            }
            "notes" => {
                notes = value
                    .as_str()
                    .ok_or_else(|| err("notes must be a string"))?
                    .to_owned();
            }
            "assets" => {
                assets = Some(parse_assets(value)?);
            }
            other => return Err(err(format!("unknown key '{other}'"))),
        }
        Ok(())
    })?;
    p.ws();
    if p.at != p.text.len() {
        return Err(err("text after the manifest's closing brace"));
    }
    let version = version.ok_or_else(|| err("no version"))?;
    let assets = assets.ok_or_else(|| err("no assets"))?;
    Ok(Manifest {
        version,
        notes,
        assets,
    })
}

fn parse_assets(value: &Value<'_>) -> Result<Vec<Asset>, ManifestError> {
    let items = value
        .as_array()
        .ok_or_else(|| err("assets must be an array"))?;
    let mut assets = Vec::with_capacity(items.len());
    for item in items {
        let mut name = None;
        let mut url = None;
        let mut sha256 = None;
        let mut sig = None;
        item.object(|key, value| {
            match key {
                "name" => {
                    name = Some(
                        value
                            .as_str()
                            .ok_or_else(|| err("asset name must be a string"))?
                            .to_owned(),
                    )
                }
                "url" => {
                    url = Some(
                        value
                            .as_str()
                            .ok_or_else(|| err("asset url must be a string"))?
                            .to_owned(),
                    )
                }
                "sha256" => {
                    let text = value
                        .as_str()
                        .ok_or_else(|| err("asset sha256 must be a string"))?;
                    sha256 = Some(
                        unhex32(text)
                            .ok_or_else(|| err("asset sha256 must be 64 hex characters"))?,
                    );
                }
                "ed25519_sig" => {
                    let text = value
                        .as_str()
                        .ok_or_else(|| err("asset ed25519_sig must be a string"))?;
                    sig = Some(
                        unhex64(text)
                            .ok_or_else(|| err("asset ed25519_sig must be 128 hex characters"))?,
                    );
                }
                other => return Err(err(format!("unknown key '{other}' in an asset"))),
            }
            Ok(())
        })?;
        let name = name.ok_or_else(|| err("an asset has no name"))?;
        // The `'{name}'` messages below borrow it; the struct takes it last.
        let url = url.ok_or_else(|| err(format!("asset '{name}' has no url")))?;
        let sha256 = sha256.ok_or_else(|| err(format!("asset '{name}' has no sha256")))?;
        let ed25519_sig = sig.ok_or_else(|| err(format!("asset '{name}' has no ed25519_sig")))?;
        // An asset name is a file name this updater will write under, and
        // `../` in one is not a malformed feed, it is an attempt to make
        // the apply step write outside the install directory.
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(err(format!(
                "asset name '{name}' is not a plain file name — a path in one is not a feed \
                 this updater will apply"
            )));
        }
        assets.push(Asset {
            name,
            url,
            sha256,
            ed25519_sig,
        });
    }
    if assets.is_empty() {
        return Err(err(
            "assets is empty — an update with nothing in it applies nothing",
        ));
    }
    Ok(assets)
}

fn err<S: Into<String>>(why: S) -> ManifestError {
    ManifestError(why.into())
}

// ── the scanner ─────────────────────────────────────────────────────────────
//
// A cursor over the bytes, the four token shapes the format has (objects,
// arrays, strings, and the whitespace between), and nothing else. No
// numbers: nothing in this format is a number, and a parser that can read
// one is a parser that will accept a manifest this format never wrote.

/// One parsed value, borrowed from the input.
#[derive(Debug)]
enum Value<'a> {
    Str(std::borrow::Cow<'a, str>),
    Array(Vec<Value<'a>>),
    /// An object, as its ordered `"key": value` pairs.
    Object(Vec<(String, Value<'a>)>),
}

impl<'a> Value<'a> {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[Value<'a>]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Walk an object's `"key": value` pairs, calling `visit` for each.
    ///
    /// Generic rather than `dyn FnMut` so a plain closure at the call site
    /// needs no type annotation: a `dyn` object here would be
    /// higher-ranked over the borrow lifetime and every caller would pay
    /// a `for<'a>` annotation for no runtime difference.
    fn object<V>(&self, mut visit: V) -> Result<(), ManifestError>
    where
        V: FnMut(&str, &Value<'a>) -> Result<(), ManifestError>,
    {
        let Self::Object(pairs) = self else {
            return Err(err("expected an object"));
        };
        for (key, value) in pairs {
            visit(key, value)?;
        }
        Ok(())
    }
}

struct P<'a> {
    text: &'a [u8],
    at: usize,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while matches!(self.text.get(self.at), Some(b' ' | b'\n' | b'\t' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), ManifestError> {
        if self.text.get(self.at) == Some(&byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(err(format!(
                "expected '{}' at byte {}",
                byte as char, self.at
            )))
        }
    }

    /// A JSON string, with `\\`, `\"` and `\n` unescaped. Anything else
    /// after a backslash is refused (see the module docs).
    fn string(&mut self) -> Result<std::borrow::Cow<'a, str>, ManifestError> {
        self.eat(b'"')?;
        let start = self.at;
        let mut out = String::new();
        let mut plain = start;
        loop {
            let Some(&b) = self.text.get(self.at) else {
                return Err(err("unterminated string"));
            };
            match b {
                b'"' => {
                    if out.is_empty() {
                        let raw = &self.text[start..self.at];
                        self.at += 1; // past the closing quote
                        return Ok(std::borrow::Cow::Borrowed(
                            std::str::from_utf8(raw).map_err(|_| err("a string is not UTF-8"))?,
                        ));
                    }
                    out.push_str(
                        std::str::from_utf8(&self.text[plain..self.at])
                            .map_err(|_| err("a string is not UTF-8"))?,
                    );
                    self.at += 1;
                    return Ok(std::borrow::Cow::Owned(out));
                }
                b'\\' => {
                    out.push_str(
                        std::str::from_utf8(&self.text[plain..self.at])
                            .map_err(|_| err("a string is not UTF-8"))?,
                    );
                    let escaped = *self
                        .text
                        .get(self.at + 1)
                        .ok_or_else(|| err("unterminated escape"))?;
                    match escaped {
                        b'\\' => out.push('\\'),
                        b'"' => out.push('"'),
                        b'n' => out.push('\n'),
                        other => {
                            return Err(err(format!(
                                "escape '\\{}' is not one this format writes",
                                other as char
                            )))
                        }
                    }
                    self.at += 2;
                    plain = self.at;
                }
                _ => self.at += 1,
            }
        }
    }

    /// A bare word expected to be `null`, `true` or `false` — present so a
    /// manifest can be refused with "this format has no literals" rather
    /// than "expected ',' at byte N".
    fn literal(&mut self) -> Result<&'a str, ManifestError> {
        let start = self.at;
        while matches!(self.text.get(self.at), Some(b'a'..=b'z')) {
            self.at += 1;
        }
        std::str::from_utf8(&self.text[start..self.at]).map_err(|_| err("a literal is not UTF-8"))
    }

    fn value(&mut self) -> Result<Value<'a>, ManifestError> {
        self.ws();
        match self.text.get(self.at) {
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.ws();
                if self.text.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    self.ws();
                    match self.text.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Value::Array(items));
                        }
                        _ => return Err(err("expected ',' or ']' in an array")),
                    }
                }
            }
            Some(b'{') => {
                self.at += 1;
                Ok(Value::Object(self.object_body()?))
            }
            Some(b'a'..=b'z') => {
                let word = self.literal()?;
                Err(err(format!(
                    "'{word}' — this format has no literals, only strings, arrays and objects"
                )))
            }
            _ => Err(err("expected a value")),
        }
    }

    /// The `"key": value, …` body of an object, as ordered pairs.
    fn object_body(&mut self) -> Result<Vec<(String, Value<'a>)>, ManifestError> {
        let mut pairs = Vec::new();
        self.ws();
        if self.text.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(pairs);
        }
        loop {
            self.ws();
            let key = self.string()?;
            self.ws();
            self.eat(b':')?;
            let value = self.value()?;
            pairs.push((key.into_owned(), value));
            self.ws();
            match self.text.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(pairs);
                }
                _ => return Err(err("expected ',' or '}' in an object")),
            }
        }
    }

    /// Parse a whole object, visiting its pairs.
    fn object<V>(&mut self, visit: V) -> Result<(), ManifestError>
    where
        V: FnMut(&str, &Value<'a>) -> Result<(), ManifestError>,
    {
        self.ws();
        let value = self.value()?;
        value.object(visit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha256::digest;

    fn manifest_text(seed: &[u8; 32]) -> (String, [u8; 32]) {
        let pk = ed25519::public_key(seed);
        let d = digest(b"the asset bytes");
        let sig = ed25519::sign(seed, &d);
        (
            format!(
                "{{\n\
                 \x20 \"format\": \"{FORMAT}\",\n\
                 \x20 \"version\": \"1.4.0\",\n\
                 \x20 \"notes\": \"fixes things\",\n\
                 \x20 \"assets\": [\n\
                 \x20   {{\n\
                 \x20     \"name\": \"app.tar.gz\",\n\
                 \x20     \"url\": \"http://updates.example.test/app.tar.gz\",\n\
                 \x20     \"sha256\": \"{}\",\n\
                 \x20     \"ed25519_sig\": \"{}\"\n\
                 \x20   }}\n\
                 \x20 ]\n}}\n",
                crate::sha256::hex(&d),
                crate::sha256::hex(&sig),
            ),
            pk,
        )
    }

    #[test]
    fn a_well_formed_manifest_parses_and_verifies() {
        let seed = [7u8; 32];
        let (text, pk) = manifest_text(&seed);
        let manifest = parse(&text).expect("parses");
        assert_eq!(manifest.version, Version::parse("1.4.0").unwrap());
        assert_eq!(manifest.notes, "fixes things");
        assert_eq!(manifest.assets.len(), 1);
        assert_eq!(manifest.assets[0].name, "app.tar.gz");
        assert!(manifest.verify(&pk).is_ok());
    }

    #[test]
    fn a_tampered_digest_does_not_verify() {
        let seed = [7u8; 32];
        let d = digest(b"the asset bytes");
        let sig = ed25519::sign(&seed, &d);
        // Swap the declared digest for the digest of different bytes,
        // keeping the old signature: the signature no longer covers the
        // declared digest.
        let other = digest(b"other bytes");
        let tampered = format!(
            "{{\"format\":\"{FORMAT}\",\"version\":\"1.4.0\",\"notes\":\"\",\
             \"assets\":[{{\"name\":\"app.tar.gz\",\"url\":\"http://u/\",\
             \"sha256\":\"{}\",\"ed25519_sig\":\"{}\"}}]}}",
            crate::sha256::hex(&other),
            crate::sha256::hex(&sig),
        );
        let manifest = parse(&tampered).expect("still parses");
        assert!(
            manifest.verify(&ed25519::public_key(&seed)).is_err(),
            "a digest the signature does not cover must be refused"
        );
    }

    #[test]
    fn a_foreign_key_does_not_verify() {
        let signer = [7u8; 32];
        let (text, _real_pk) = manifest_text(&signer);
        let attacker = [8u8; 32];
        let attacker_pk = ed25519::public_key(&attacker);
        let manifest = parse(&text).expect("parses");
        assert!(
            manifest.verify(&attacker_pk).is_err(),
            "signed by one key, checked against another: refused"
        );
    }

    #[test]
    fn the_writer_and_reader_round_trip() {
        let seed = [9u8; 32];
        let d = digest(b"round trip");
        let manifest = Manifest {
            version: Version::parse("2.1.0-rc.2").unwrap(),
            notes: "notes with \"quotes\" and a\nnewline".to_owned(),
            assets: vec![Asset {
                name: "app.tar.gz".to_owned(),
                url: "http://updates.example.test/app.tar.gz".to_owned(),
                sha256: d,
                ed25519_sig: ed25519::sign(&seed, &d),
            }],
        };
        let text = manifest.to_json();
        let back = parse(&text).expect("the writer's output parses");
        assert_eq!(back, manifest);
        assert!(back.verify(&ed25519::public_key(&seed)).is_ok());
    }

    #[test]
    fn signing_for_release_makes_a_manifest_that_verifies() {
        let seed = [11u8; 32];
        let d = digest(b"to be signed");
        let unsigned = Manifest {
            version: Version::parse("1.0.1").unwrap(),
            notes: String::new(),
            assets: vec![Asset {
                name: "app.msi".to_owned(),
                url: "http://updates.example.test/app.msi".to_owned(),
                sha256: d,
                ed25519_sig: [0u8; 64],
            }],
        };
        let signed = unsigned.sign_for_release(&seed);
        assert_ne!(unsigned.assets[0].ed25519_sig, signed.assets[0].ed25519_sig);
        assert!(signed.verify(&ed25519::public_key(&seed)).is_ok());
        assert!(signed.verify(&ed25519::public_key(&[12u8; 32])).is_err());
    }

    #[test]
    fn wrong_format_versions_are_named_not_guessed() {
        let (text, _) = manifest_text(&[7u8; 32]);
        let future = text.replace(FORMAT, "vieww.update/2");
        let error = parse(&future).unwrap_err();
        assert!(
            error.0.contains("vieww.update/2"),
            "the error names the format it saw: {}",
            error.0
        );
    }

    #[test]
    fn unknown_keys_are_refused() {
        let (text, _) = manifest_text(&[7u8; 32]);
        let extra = text.replace(
            "\"notes\": \"fixes things\",",
            "\"notes\": \"fixes things\", \"delta\": \"no\",",
        );
        let error = parse(&extra).unwrap_err();
        assert!(error.0.contains("delta"), "{}", error.0);
        // A future feed that grows a boolean hits the format's other
        // refusal — no literals — rather than the unknown-key one; both
        // name themselves, and a client either way says "newer manifest
        // format" rather than half-reading it.
        let literal = text.replace(
            "\"notes\": \"fixes things\",",
            "\"notes\": \"fixes things\", \"delta\": true,",
        );
        let error = parse(&literal).unwrap_err();
        assert!(error.0.contains("no literals"), "{}", error.0);
    }

    #[test]
    fn an_asset_name_that_is_a_path_is_refused() {
        let seed = [7u8; 32];
        let d = digest(b"x");
        let sig = ed25519::sign(&seed, &d);
        let text = format!(
            "{{\"format\":\"{FORMAT}\",\"version\":\"1.0.0\",\"notes\":\"\",\
             \"assets\":[{{\"name\":\"../../etc/passwd\",\"url\":\"http://x/\",\
             \"sha256\":\"{}\",\"ed25519_sig\":\"{}\"}}]}}",
            crate::sha256::hex(&d),
            crate::sha256::hex(&sig),
        );
        let error = parse(&text).unwrap_err();
        assert!(error.0.contains("not a plain file name"), "{}", error.0);
    }

    #[test]
    fn empty_assets_and_missing_fields_are_named_errors() {
        let d = digest(b"x");
        let empty = format!(
            "{{\"format\":\"{FORMAT}\",\"version\":\"1.0.0\",\"notes\":\"\",\"assets\":[]}}"
        );
        assert!(parse(&empty).unwrap_err().0.contains("empty"));
        let no_version = format!(
            "{{\"format\":\"{FORMAT}\",\"notes\":\"\",\"assets\":[{{\"name\":\"a\",\"url\":\"http://x/\",\
             \"sha256\":\"{}\",\"ed25519_sig\":\"{}\"}}]}}",
            crate::sha256::hex(&d),
            crate::sha256::hex(&[0u8; 64]),
        );
        assert!(parse(&no_version).unwrap_err().0.contains("no version"));
        let trailing = {
            let (text, _) = manifest_text(&[7u8; 32]);
            format!("{text}trailing")
        };
        assert!(parse(&trailing).unwrap_err().0.contains("after"));
    }

    #[test]
    fn malformed_input_is_refused_not_panicked() {
        for text in [
            "",
            "{",
            "}",
            "[]",
            "{\"format\": 1}",
            "{\"format\": \"x\",}",
            "{\"format\":\"vieww.update/1\",\"version\":\"1.0.0\",\"notes\":\"\",\
             \"assets\":[{\"name\":\"a\"}]}",
            "null",
            "true",
        ] {
            assert!(parse(text).is_err(), "{text:?} should not parse");
        }
    }

    #[test]
    fn escapes_round_trip() {
        let seed = [7u8; 32];
        let d = digest(b"esc");
        let manifest = Manifest {
            version: Version::parse("1.0.0").unwrap(),
            notes: "line one\nline \"two\" \\ backslash".to_owned(),
            assets: vec![Asset {
                name: "a".to_owned(),
                url: "http://x/".to_owned(),
                sha256: d,
                ed25519_sig: ed25519::sign(&seed, &d),
            }],
        };
        let back = parse(&manifest.to_json()).expect("round trips");
        assert_eq!(back.notes, manifest.notes);
    }
}
