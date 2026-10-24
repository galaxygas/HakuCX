/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The password, payment card and address vault.
//!
//! Everything is encrypted with a random 256-bit data key (XChaCha20-Poly1305). The data key is
//! stored wrapped: by the operating system (Windows DPAPI, through a [`Protector`] the browser
//! supplies), and optionally by a recovery password (Argon2id). The vault file never contains
//! the data key or any secret in the clear.
//!
//! [`VaultData::merge`] combines two copies item by item (last change wins, deletions are kept
//! as tombstones). It is used both when two windows of a profile save at the same time and by
//! folder sync.

use std::collections::BTreeMap;

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::{Zeroize, Zeroizing};

pub const FORMAT: u32 = 1;
const AAD_DATA: &[u8] = b"hakucx-vault-data-v1";
const AAD_KEY: &[u8] = b"hakucx-vault-key-v1";

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum VaultError {
    #[error("the vault file is damaged")]
    Damaged,
    #[error("this vault was made by a newer HakuCX")]
    TooNew,
    #[error("Windows could not unlock the vault ({0})")]
    Os(String),
    #[error("the vault can only be opened with its recovery password")]
    NeedsPassword,
    #[error("wrong password")]
    WrongPassword,
    #[error("encryption failed")]
    Crypto,
}

// ------------------------------------------------------------------ crypto

/// A 256-bit key, wiped from memory when dropped.
#[derive(Clone)]
pub struct Key(Zeroizing<[u8; 32]>);

impl Key {
    pub fn random() -> Key {
        let mut k = [0u8; 32];
        fill_random(&mut k);
        let key = Key(Zeroizing::new(k));
        k.zeroize();
        key
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Key> {
        let array: [u8; 32] = bytes.try_into().ok()?;
        Some(Key(Zeroizing::new(array)))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub fn fill_random(buf: &mut [u8]) {
    getrandom::fill(buf).expect("the operating system's random number generator failed");
}

/// Ciphertext with its random nonce.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sealed {
    #[serde(with = "b64")]
    pub nonce: Vec<u8>,
    #[serde(with = "b64")]
    pub ct: Vec<u8>,
}

pub fn seal(key: &Key, plaintext: &[u8], aad: &[u8]) -> Result<Sealed, VaultError> {
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let mut nonce = [0u8; 24];
    fill_random(&mut nonce);
    let ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| VaultError::Crypto)?;
    Ok(Sealed {
        nonce: nonce.to_vec(),
        ct,
    })
}

/// Decrypt; `None` when the key is wrong or the data was changed.
pub fn open(key: &Key, sealed: &Sealed, aad: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if sealed.nonce.len() != 24 {
        return None;
    }
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    cipher
        .decrypt(
            XNonce::from_slice(&sealed.nonce),
            Payload {
                msg: &sealed.ct,
                aad,
            },
        )
        .ok()
        .map(Zeroizing::new)
}

/// Argon2id parameters, stored next to what they protect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    #[serde(with = "b64")]
    pub salt: Vec<u8>,
}

impl KdfParams {
    /// OWASP's recommended Argon2id settings (64 MiB, 3 passes) with a fresh salt.
    pub fn new() -> KdfParams {
        let mut salt = vec![0u8; 16];
        fill_random(&mut salt);
        KdfParams {
            m_kib: 64 * 1024,
            t: 3,
            p: 1,
            salt,
        }
    }

    /// Small parameters, for tests only.
    #[cfg(test)]
    pub fn fast() -> KdfParams {
        KdfParams {
            m_kib: 64,
            t: 1,
            ..KdfParams::new()
        }
    }
}

impl Default for KdfParams {
    fn default() -> Self {
        Self::new()
    }
}

/// Derive a key from a password. Parameters are bounded so a crafted file cannot make
/// HakuCX allocate gigabytes or spin for minutes.
pub fn derive_key(password: &str, params: &KdfParams) -> Result<Key, VaultError> {
    if params.m_kib > 1024 * 1024 || params.t > 64 || params.p == 0 || params.p > 16 {
        return Err(VaultError::Damaged);
    }
    let argon = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(params.m_kib, params.t, params.p, Some(32))
            .map_err(|_| VaultError::Damaged)?,
    );
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), &params.salt, out.as_mut())
        .map_err(|_| VaultError::Crypto)?;
    Ok(Key(out))
}

/// Something that can wrap a secret so only this user on this machine can unwrap it
/// (Windows DPAPI in the browser; a fake in tests).
pub trait Protector {
    fn protect(&self, secret: &[u8]) -> Result<Vec<u8>, String>;
    fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, String>;
}

mod b64 {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)
    }
}

// ------------------------------------------------------------------ data

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Login {
    pub id: String,
    /// `scheme://host[:port]` of the page the login is for.
    pub origin: String,
    pub username: String,
    pub password: String,
    pub note: String,
    pub created: i64,
    pub modified: i64,
    pub last_used: i64,
    pub times_used: u32,
}

impl Default for Login {
    fn default() -> Self {
        Login {
            id: new_id(),
            origin: String::new(),
            username: String::new(),
            password: String::new(),
            note: String::new(),
            created: 0,
            modified: 0,
            last_used: 0,
            times_used: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub nickname: String,
    pub holder: String,
    /// Digits only.
    pub number: String,
    pub exp_month: u8,
    pub exp_year: u16,
    pub created: i64,
    pub modified: i64,
}

impl Default for Card {
    fn default() -> Self {
        Card {
            id: new_id(),
            nickname: String::new(),
            holder: String::new(),
            number: String::new(),
            exp_month: 0,
            exp_year: 0,
            created: 0,
            modified: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Address {
    pub id: String,
    pub full_name: String,
    pub organization: String,
    /// Street lines, newline-separated.
    pub street: String,
    pub city: String,
    /// State, province or county.
    pub region: String,
    pub postal_code: String,
    /// ISO 3166-1 alpha-2 code or a country name.
    pub country: String,
    pub phone: String,
    pub email: String,
    pub created: i64,
    pub modified: i64,
}

impl Default for Address {
    fn default() -> Self {
        Address {
            id: new_id(),
            full_name: String::new(),
            organization: String::new(),
            street: String::new(),
            city: String::new(),
            region: String::new(),
            postal_code: String::new(),
            country: String::new(),
            phone: String::new(),
            email: String::new(),
            created: 0,
            modified: 0,
        }
    }
}

/// Everything the vault holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VaultData {
    pub logins: Vec<Login>,
    pub cards: Vec<Card>,
    pub addresses: Vec<Address>,
    /// Sites where HakuCX never offers to save: origin -> when that was chosen.
    pub never_save: BTreeMap<String, i64>,
    /// Deleted items (and sites removed from `never_save`, as `never:<origin>`): id -> when.
    pub tombstones: BTreeMap<String, i64>,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// The origin a login belongs to, from a page URL.
pub fn origin_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return None;
    }
    Some(u.origin().ascii_serialization())
}

/// What a submitted login form means for the vault.
#[derive(Clone, Debug, PartialEq)]
pub enum Submission {
    /// Nothing saved for this site and user: offer to save.
    New,
    /// Saved with a different password: offer to update this login.
    Update(String),
    /// Already saved exactly so.
    Known(String),
    /// The user chose never to save on this site.
    Never,
}

impl VaultData {
    /// Logins usable on a page at `origin`: the same origin, the same host with or without
    /// `www.`, and logins saved on `http` used on the `https` version (never the reverse).
    pub fn logins_for(&self, origin: &str) -> Vec<&Login> {
        let Ok(page) = url::Url::parse(origin) else {
            return Vec::new();
        };
        let host = |u: &url::Url| {
            u.host_str()
                .unwrap_or_default()
                .trim_start_matches("www.")
                .to_owned()
        };
        let page_host = host(&page);
        let mut out: Vec<&Login> = self
            .logins
            .iter()
            .filter(|l| {
                let Ok(saved) = url::Url::parse(&l.origin) else {
                    return false;
                };
                let same_scheme = saved.scheme() == page.scheme();
                let upgrade = saved.scheme() == "http" && page.scheme() == "https";
                let port_ok = if same_scheme {
                    saved.port_or_known_default() == page.port_or_known_default()
                } else {
                    saved.port().is_none() && page.port().is_none()
                };
                (same_scheme || upgrade) && port_ok && host(&saved) == page_host
            })
            .collect();
        // Exact origin first, then most used.
        out.sort_by_key(|l| (l.origin != origin, std::cmp::Reverse(l.times_used)));
        out
    }

    pub fn classify(&self, origin: &str, username: &str, password: &str) -> Submission {
        if self.never_save.contains_key(origin) {
            return Submission::Never;
        }
        match self
            .logins
            .iter()
            .find(|l| l.origin == origin && l.username == username)
        {
            Some(l) if l.password == password => Submission::Known(l.id.clone()),
            Some(l) => Submission::Update(l.id.clone()),
            None => Submission::New,
        }
    }

    /// Add a login or change an existing one (by id). Returns its id.
    pub fn upsert_login(&mut self, mut login: Login, now: i64) -> String {
        login.modified = now;
        match self.logins.iter_mut().find(|l| l.id == login.id) {
            Some(existing) => {
                login.created = existing.created;
                login.last_used = existing.last_used.max(login.last_used);
                login.times_used = existing.times_used.max(login.times_used);
                *existing = login;
                existing.id.clone()
            }
            None => {
                if login.created == 0 {
                    login.created = now;
                }
                let id = login.id.clone();
                self.tombstones.remove(&id);
                self.logins.push(login);
                id
            }
        }
    }

    /// A login was filled or confirmed by a submission.
    pub fn touch_login(&mut self, id: &str, now: i64) {
        if let Some(l) = self.logins.iter_mut().find(|l| l.id == id) {
            l.last_used = now;
            l.times_used = l.times_used.saturating_add(1);
            l.modified = now;
        }
    }

    pub fn delete(&mut self, id: &str, now: i64) -> bool {
        let before = self.logins.len() + self.cards.len() + self.addresses.len();
        self.logins.retain(|l| l.id != id);
        self.cards.retain(|c| c.id != id);
        self.addresses.retain(|a| a.id != id);
        let removed = before != self.logins.len() + self.cards.len() + self.addresses.len();
        if removed {
            self.tombstones.insert(id.to_owned(), now);
        }
        removed
    }

    pub fn set_never_save(&mut self, origin: &str, never: bool, now: i64) {
        let key = format!("never:{origin}");
        if never {
            self.never_save.insert(origin.to_owned(), now);
            self.tombstones.remove(&key);
        } else if self.never_save.remove(origin).is_some() {
            self.tombstones.insert(key, now);
        }
    }

    pub fn upsert_card(&mut self, mut card: Card, now: i64) -> String {
        card.number.retain(|c| c.is_ascii_digit());
        card.modified = now;
        match self.cards.iter_mut().find(|c| c.id == card.id) {
            Some(existing) => {
                card.created = existing.created;
                *existing = card;
                existing.id.clone()
            }
            None => {
                card.created = now;
                let id = card.id.clone();
                self.cards.push(card);
                id
            }
        }
    }

    pub fn upsert_address(&mut self, mut address: Address, now: i64) -> String {
        address.modified = now;
        match self.addresses.iter_mut().find(|a| a.id == address.id) {
            Some(existing) => {
                address.created = existing.created;
                *existing = address;
                existing.id.clone()
            }
            None => {
                address.created = now;
                let id = address.id.clone();
                self.addresses.push(address);
                id
            }
        }
    }

    /// Combine with another copy: for each item the most recent change wins; a deletion wins
    /// over changes made before it. Returns whether `self` changed.
    pub fn merge(&mut self, other: &VaultData) -> bool {
        let before = self.clone();
        for (id, at) in &other.tombstones {
            let mine = self.tombstones.entry(id.clone()).or_insert(*at);
            *mine = (*mine).max(*at);
        }
        merge_items(&mut self.logins, &other.logins, |l| (&l.id, l.modified));
        merge_items(&mut self.cards, &other.cards, |c| (&c.id, c.modified));
        merge_items(&mut self.addresses, &other.addresses, |a| {
            (&a.id, a.modified)
        });
        for (origin, at) in &other.never_save {
            let entry = self.never_save.entry(origin.clone()).or_insert(*at);
            *entry = (*entry).max(*at);
        }
        let tombstones = self.tombstones.clone();
        let dead = |id: &str, modified: i64| tombstones.get(id).is_some_and(|at| *at >= modified);
        self.logins.retain(|l| !dead(&l.id, l.modified));
        self.cards.retain(|c| !dead(&c.id, c.modified));
        self.addresses.retain(|a| !dead(&a.id, a.modified));
        self.never_save
            .retain(|origin, at| !dead(&format!("never:{origin}"), *at));
        *self != before
    }
}

fn merge_items<T: Clone>(mine: &mut Vec<T>, theirs: &[T], key: impl Fn(&T) -> (&String, i64)) {
    for item in theirs {
        let (id, modified) = key(item);
        match mine.iter_mut().find(|m| key(m).0 == id) {
            Some(existing) => {
                if modified > key(existing).1 {
                    *existing = item.clone();
                }
            }
            None => mine.push(item.clone()),
        }
    }
}

// ------------------------------------------------------------------ the vault file

#[derive(Clone, Debug, Serialize, Deserialize)]
struct VaultFile {
    format: u32,
    /// The data key wrapped by the operating system.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "opt_b64")]
    os_key: Option<Vec<u8>>,
    /// The data key wrapped by a key derived from the recovery password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery: Option<Recovery>,
    data: Sealed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Recovery {
    kdf: KdfParams,
    key: Sealed,
}

mod opt_b64 {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|t| {
                base64::engine::general_purpose::STANDARD
                    .decode(t)
                    .map_err(serde::de::Error::custom)
            })
            .transpose()
    }
}

/// An open vault: the data key and the decrypted data.
pub struct Vault {
    key: Key,
    pub data: VaultData,
    recovery: Option<Recovery>,
}

impl Vault {
    pub fn create() -> Vault {
        Vault {
            key: Key::random(),
            data: VaultData::default(),
            recovery: None,
        }
    }

    fn parse(bytes: &[u8]) -> Result<VaultFile, VaultError> {
        let file: VaultFile = serde_json::from_slice(bytes).map_err(|_| VaultError::Damaged)?;
        if file.format > FORMAT {
            return Err(VaultError::TooNew);
        }
        Ok(file)
    }

    fn with_key(file: VaultFile, key: Key) -> Result<Vault, VaultError> {
        let plain = open(&key, &file.data, AAD_DATA).ok_or(VaultError::Damaged)?;
        let data = serde_json::from_slice(&plain).map_err(|_| VaultError::Damaged)?;
        Ok(Vault {
            key,
            data,
            recovery: file.recovery,
        })
    }

    /// Open with the operating system's key.
    pub fn open_os(bytes: &[u8], protector: &dyn Protector) -> Result<Vault, VaultError> {
        let file = Self::parse(bytes)?;
        let Some(blob) = file.os_key.as_ref() else {
            return Err(if file.recovery.is_some() {
                VaultError::NeedsPassword
            } else {
                VaultError::Damaged
            });
        };
        let raw = protector.unprotect(blob).map_err(|e| {
            if file.recovery.is_some() {
                VaultError::NeedsPassword
            } else {
                VaultError::Os(e)
            }
        })?;
        let key = Key::from_bytes(&raw).ok_or(VaultError::Damaged)?;
        Self::with_key(file, key)
    }

    /// Open with the recovery password (when Windows can no longer unwrap the key).
    pub fn open_password(bytes: &[u8], password: &str) -> Result<Vault, VaultError> {
        let file = Self::parse(bytes)?;
        let recovery = file.recovery.clone().ok_or(VaultError::Damaged)?;
        let kek = derive_key(password, &recovery.kdf)?;
        let raw = open(&kek, &recovery.key, AAD_KEY).ok_or(VaultError::WrongPassword)?;
        let key = Key::from_bytes(&raw).ok_or(VaultError::Damaged)?;
        Self::with_key(file, key)
    }

    pub fn has_recovery_password(&self) -> bool {
        self.recovery.is_some()
    }

    /// Set (or remove, with `None`) the recovery password. Slow by design (Argon2id).
    pub fn set_recovery_password(&mut self, password: Option<&str>) -> Result<(), VaultError> {
        self.recovery = match password {
            Some(p) => {
                let kdf = KdfParams::new();
                let kek = derive_key(p, &kdf)?;
                Some(Recovery {
                    key: seal(&kek, self.key.as_bytes(), AAD_KEY)?,
                    kdf,
                })
            }
            None => None,
        };
        Ok(())
    }

    #[cfg(test)]
    fn set_recovery_password_fast(&mut self, password: &str) {
        let kdf = KdfParams::fast();
        let kek = derive_key(password, &kdf).unwrap();
        self.recovery = Some(Recovery {
            key: seal(&kek, self.key.as_bytes(), AAD_KEY).unwrap(),
            kdf,
        });
    }

    /// The encrypted file. The data key is wrapped by `protector` (if any) and by the
    /// recovery password (if one is set).
    pub fn to_bytes(&self, protector: Option<&dyn Protector>) -> Result<Vec<u8>, VaultError> {
        let plain = Zeroizing::new(serde_json::to_vec(&self.data).map_err(|_| VaultError::Crypto)?);
        let os_key = match protector {
            Some(p) => Some(p.protect(self.key.as_bytes()).map_err(VaultError::Os)?),
            None => None,
        };
        let file = VaultFile {
            format: FORMAT,
            os_key,
            recovery: self.recovery.clone(),
            data: seal(&self.key, &plain, AAD_DATA)?,
        };
        serde_json::to_vec_pretty(&file).map_err(|_| VaultError::Crypto)
    }
}

// ------------------------------------------------------------------ cards

/// Card network from the number's prefix.
pub fn card_brand(number: &str) -> &'static str {
    let d: String = number.chars().filter(char::is_ascii_digit).collect();
    let p = |n: usize| d.get(..n).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    if d.starts_with('4') {
        "Visa"
    } else if (51..=55).contains(&p(2)) || (2221..=2720).contains(&p(4)) {
        "Mastercard"
    } else if matches!(p(2), 34 | 37) {
        "American Express"
    } else if p(4) == 6011 || p(2) == 65 || (644..=649).contains(&p(3)) {
        "Discover"
    } else if (3528..=3589).contains(&p(4)) {
        "JCB"
    } else if matches!(p(2), 36 | 38) || (300..=305).contains(&p(3)) {
        "Diners Club"
    } else if p(2) == 62 {
        "UnionPay"
    } else {
        "Card"
    }
}

/// The Luhn check digit test.
pub fn luhn_valid(number: &str) -> bool {
    let digits: Vec<u32> = number.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(12..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                *d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

// ------------------------------------------------------------------ CSV import and export

/// Result of a password import.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportReport {
    pub added: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub skipped: u32,
}

/// Parse CSV (RFC 4180: quoted fields, doubled quotes, newlines inside quotes).
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r| !(r.len() == 1 && r[0].is_empty()));
    rows
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) || s.starts_with([' ', '=', '+', '-', '@']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

impl VaultData {
    /// Import Chrome, Edge, Opera, Brave or Firefox password CSV exports (columns are found
    /// by name: `url`/`origin`, `username`, `password`, optional `note`/`name`).
    pub fn import_csv(&mut self, text: &str, now: i64) -> ImportReport {
        let mut report = ImportReport::default();
        let rows = parse_csv(text);
        let Some((header, body)) = rows.split_first() else {
            return report;
        };
        let col = |names: &[&str]| {
            header
                .iter()
                .position(|h| names.contains(&h.trim().to_ascii_lowercase().as_str()))
        };
        let (Some(url_col), Some(user_col), Some(pass_col)) = (
            col(&["url", "origin", "login_uri", "website"]),
            col(&["username", "login", "login_username", "user"]),
            col(&["password", "login_password"]),
        ) else {
            report.skipped = body.len() as u32;
            return report;
        };
        let note_col = col(&["note", "notes", "comment"]);
        for r in body {
            let get = |i: usize| r.get(i).map(|s| s.as_str()).unwrap_or("");
            let (Some(origin), password) = (origin_of(get(url_col)), get(pass_col)) else {
                report.skipped += 1;
                continue;
            };
            if password.is_empty() {
                report.skipped += 1;
                continue;
            }
            let username = get(user_col);
            match self.classify(&origin, username, password) {
                Submission::Known(_) => report.unchanged += 1,
                Submission::Update(id) => {
                    if let Some(l) = self.logins.iter().find(|l| l.id == id).cloned() {
                        self.upsert_login(
                            Login {
                                password: password.to_owned(),
                                ..l
                            },
                            now,
                        );
                    }
                    report.updated += 1;
                }
                Submission::New | Submission::Never => {
                    self.upsert_login(
                        Login {
                            origin,
                            username: username.to_owned(),
                            password: password.to_owned(),
                            note: note_col.map(get).unwrap_or("").to_owned(),
                            ..Login::default()
                        },
                        now,
                    );
                    report.added += 1;
                }
            }
        }
        report
    }

    /// Chrome's export format (`name,url,username,password,note`), which every major browser
    /// and password manager imports.
    pub fn export_csv(&self) -> Zeroizing<String> {
        let mut out = String::from("name,url,username,password,note\n");
        for l in &self.logins {
            let name = url::Url::parse(&l.origin)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .unwrap_or_default();
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                csv_field(&name),
                csv_field(&format!("{}/", l.origin)),
                csv_field(&l.username),
                csv_field(&l.password),
                csv_field(&l.note)
            ));
        }
        Zeroizing::new(out)
    }
}

/// Base64 for small binary values in other modules' files.
pub fn encode_b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn decode_b64(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// XORs with a constant: stands in for DPAPI.
    struct FakeOs(u8);

    impl Protector for FakeOs {
        fn protect(&self, secret: &[u8]) -> Result<Vec<u8>, String> {
            Ok(secret.iter().map(|b| b ^ self.0).collect())
        }
        fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
            if self.0 == 0 {
                return Err("no key".into());
            }
            Ok(Zeroizing::new(blob.iter().map(|b| b ^ self.0).collect()))
        }
    }

    fn login(origin: &str, user: &str, pass: &str) -> Login {
        Login {
            origin: origin.into(),
            username: user.into(),
            password: pass.into(),
            ..Login::default()
        }
    }

    #[test]
    fn round_trip_with_os_key_and_recovery_password() {
        let mut v = Vault::create();
        v.data
            .upsert_login(login("https://a.test", "kim", "hunter2"), 1);
        v.set_recovery_password_fast("correct horse");
        let bytes = v.to_bytes(Some(&FakeOs(0x5a))).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(!text.contains("hunter2") && !text.contains("kim"));

        let opened = Vault::open_os(&bytes, &FakeOs(0x5a)).unwrap();
        assert_eq!(opened.data.logins[0].password, "hunter2");
        // Windows lost the key: the recovery password still opens it.
        assert_eq!(
            Vault::open_os(&bytes, &FakeOs(0)).err(),
            Some(VaultError::NeedsPassword)
        );
        assert_eq!(
            Vault::open_password(&bytes, "wrong").err(),
            Some(VaultError::WrongPassword)
        );
        let recovered = Vault::open_password(&bytes, "correct horse").unwrap();
        assert_eq!(recovered.data, opened.data);
    }

    #[test]
    fn tampering_is_detected() {
        let v = Vault::create();
        let bytes = v.to_bytes(Some(&FakeOs(1))).unwrap();
        let mut file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let ct = file["data"]["ct"].as_str().unwrap().to_owned();
        let mut raw = decode_b64(&ct).unwrap();
        raw[0] ^= 1;
        file["data"]["ct"] = encode_b64(&raw).into();
        let bad = serde_json::to_vec(&file).unwrap();
        assert_eq!(
            Vault::open_os(&bad, &FakeOs(1)).err(),
            Some(VaultError::Damaged)
        );
        assert_eq!(
            Vault::open_os(b"{}", &FakeOs(1)).err(),
            Some(VaultError::Damaged)
        );
    }

    #[test]
    fn submissions_and_matching() {
        let mut d = VaultData::default();
        let id = d.upsert_login(login("https://www.site.test", "kim", "a"), 1);
        d.upsert_login(login("http://site.test", "lee", "b"), 1);
        d.upsert_login(login("https://other.test", "kim", "c"), 1);
        assert_eq!(
            d.classify("https://www.site.test", "kim", "a"),
            Submission::Known(id.clone())
        );
        assert_eq!(
            d.classify("https://www.site.test", "kim", "x"),
            Submission::Update(id)
        );
        assert_eq!(
            d.classify("https://www.site.test", "new", "x"),
            Submission::New
        );
        d.set_never_save("https://nope.test", true, 2);
        assert_eq!(d.classify("https://nope.test", "a", "b"), Submission::Never);

        let users = |o: &str| {
            d.logins_for(o)
                .iter()
                .map(|l| l.username.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(users("https://site.test"), vec!["kim", "lee"]);
        // An https login is never offered on http.
        assert_eq!(users("http://site.test"), vec!["lee"]);
        assert!(users("https://evil-site.test").is_empty());
        assert!(users("https://site.test:8443").is_empty());
    }

    #[test]
    fn merge_keeps_latest_and_deletions() {
        let mut a = VaultData::default();
        let id = a.upsert_login(login("https://a.test", "kim", "one"), 10);
        let gone = a.upsert_login(login("https://b.test", "kim", "x"), 10);
        let mut b = a.clone();
        // b changes the password later; a deletes the other login later.
        let changed = b.logins.iter().find(|l| l.id == id).cloned().unwrap();
        b.upsert_login(
            Login {
                password: "two".into(),
                ..changed
            },
            20,
        );
        a.delete(&gone, 30);
        let new_in_b = b.upsert_login(login("https://c.test", "lee", "y"), 25);
        b.set_never_save("https://never.test", true, 25);

        let mut merged = a.clone();
        assert!(merged.merge(&b));
        assert_eq!(
            merged.logins.iter().find(|l| l.id == id).unwrap().password,
            "two"
        );
        assert!(merged.logins.iter().all(|l| l.id != gone));
        assert!(merged.logins.iter().any(|l| l.id == new_in_b));
        assert!(merged.never_save.contains_key("https://never.test"));
        // Merging is symmetric and idempotent.
        let mut other = b.clone();
        other.merge(&a);
        assert_eq!(sorted(&other), sorted(&merged));
        assert!(!merged.clone().merge(&b));

        // Removing a site from "never save" syncs too.
        let mut c = merged.clone();
        c.set_never_save("https://never.test", false, 40);
        merged.merge(&c);
        assert!(!merged.never_save.contains_key("https://never.test"));
    }

    fn sorted(d: &VaultData) -> Vec<String> {
        let mut ids: Vec<String> = d
            .logins
            .iter()
            .map(|l| format!("{}:{}", l.id, l.password))
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn csv_import_and_export() {
        let chrome = "name,url,username,password,note\r\nsite,https://site.test/login,kim,\"p,a\"\"ss\",hi\r\nbad,notaurl,x,y,\r\n";
        let mut d = VaultData::default();
        let r = d.import_csv(chrome, 1);
        assert_eq!((r.added, r.skipped), (1, 1));
        assert_eq!(d.logins[0].origin, "https://site.test");
        assert_eq!(d.logins[0].password, "p,a\"ss");
        assert_eq!(d.logins[0].note, "hi");
        let firefox = "\"url\",\"username\",\"password\",\"httpRealm\"\n\"https://site.test\",\"kim\",\"new\",\"\"\n";
        let r = d.import_csv(firefox, 2);
        assert_eq!((r.added, r.updated), (0, 1));
        assert_eq!(d.logins[0].password, "new");
        assert_eq!(d.import_csv(firefox, 3).unchanged, 1);

        let out = d.export_csv();
        assert!(out.starts_with("name,url,username,password,note\n"));
        let mut again = VaultData::default();
        assert_eq!(again.import_csv(&out, 4).added, 1);
        assert_eq!(again.logins[0].password, "new");
        // Spreadsheet formula injection is neutralized by quoting.
        assert_eq!(csv_field("=cmd"), "\"=cmd\"");
    }

    #[test]
    fn cards() {
        assert!(luhn_valid("4111 1111 1111 1111"));
        assert!(!luhn_valid("4111 1111 1111 1112"));
        assert_eq!(card_brand("4111111111111111"), "Visa");
        assert_eq!(card_brand("5555555555554444"), "Mastercard");
        assert_eq!(card_brand("2223003122003222"), "Mastercard");
        assert_eq!(card_brand("378282246310005"), "American Express");
        assert_eq!(card_brand("6011111111111117"), "Discover");
        let mut d = VaultData::default();
        let id = d.upsert_card(
            Card {
                number: "4111-1111-1111-1111".into(),
                ..Card::default()
            },
            5,
        );
        assert_eq!(d.cards[0].number, "4111111111111111");
        assert!(d.delete(&id, 6));
        assert!(d.tombstones.contains_key(&id));
    }

    #[test]
    fn kdf_parameters_are_bounded() {
        let mut p = KdfParams::fast();
        p.m_kib = u32::MAX;
        assert_eq!(derive_key("x", &p).err(), Some(VaultError::Damaged));
    }
}
