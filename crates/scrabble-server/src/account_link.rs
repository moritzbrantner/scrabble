//! Optional account linking of guest-owned libraries (#132; Decision 1.46).
//!
//! The shared Moenarch account service only says *who* a request belongs to. Scrabble
//! stays the authority for its libraries: each original may record one verified
//! `account_id` as its linked owner. The original guest capability keeps working after
//! linking. On another device the linked account obtains an additional device
//! capability for the same original (`reconnect`); recipients of shares never do.
use crate::layout_library::decode_hex;
use ring::hmac;
use std::collections::{BTreeSet, HashMap};

/// Device capabilities issued by `reconnect` that one original keeps; the oldest is
/// dropped first. The original guest capability is never dropped.
pub const MAX_DEVICE_CAPABILITIES: usize = 16;

/// Whether `link` recorded a new owner or found this account already linked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Linked {
    New,
    Existing,
}

/// An original linked to an account, as listed to that account.
pub struct LinkedLibrary {
    pub id: String,
    pub name: String,
}

/// The account service's stable account id: a lowercase or uppercase UUID string.
pub fn is_account_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

/// Checks a capability against the original verifier or any reconnect device verifier.
/// Each comparison is constant-time (`hmac::verify`).
pub(crate) fn owner_matches(
    key: &hmac::Key,
    message: &[u8],
    primary: &str,
    devices: &[String],
) -> Option<bool> {
    let mut matched = false;
    for verifier in std::iter::once(primary).chain(devices.iter().map(String::as_str)) {
        let expected = decode_hex(verifier)?;
        matched |= hmac::verify(key, message, &expected).is_ok();
    }
    Some(matched)
}

/// Records `account_id` as the linked owner. `Err(())` means another account owns it.
pub(crate) fn link(owner: &mut Option<String>, account_id: &str) -> Result<Linked, ()> {
    match owner.as_deref() {
        None => {
            *owner = Some(account_id.to_owned());
            Ok(Linked::New)
        }
        Some(existing) if existing == account_id => Ok(Linked::Existing),
        Some(_) => Err(()),
    }
}

/// Adds a device verifier, dropping the oldest beyond the bound.
pub(crate) fn add_device(devices: &mut Vec<String>, verifier: String) {
    devices.push(verifier);
    if devices.len() > MAX_DEVICE_CAPABILITIES {
        let excess = devices.len() - MAX_DEVICE_CAPABILITIES;
        devices.drain(..excess);
    }
}

pub(crate) fn valid_persisted(owner: Option<&str>, devices: &[String]) -> bool {
    owner.is_none_or(is_account_id)
        && devices.len() <= MAX_DEVICE_CAPABILITIES
        && devices
            .iter()
            .all(|verifier| decode_hex(verifier).is_some())
        && (devices.is_empty() || owner.is_some())
}

/// Account id -> linked originals of one library kind.
#[derive(Default)]
pub(crate) struct AccountIndex(HashMap<String, BTreeSet<String>>);
impl AccountIndex {
    pub(crate) fn add(&mut self, account_id: Option<&str>, id: &str) {
        if let Some(account_id) = account_id {
            self.0
                .entry(account_id.to_owned())
                .or_default()
                .insert(id.to_owned());
        }
    }
    pub(crate) fn remove(&mut self, account_id: Option<&str>, id: &str) {
        if let Some(account_id) = account_id
            && let Some(ids) = self.0.get_mut(account_id)
        {
            ids.remove(id);
            if ids.is_empty() {
                self.0.remove(account_id);
            }
        }
    }
    pub(crate) fn ids(&self, account_id: &str) -> Vec<String> {
        self.0
            .get(account_id)
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_ids_are_uuid_strings() {
        assert!(is_account_id("5f0c7b8e-0d3a-4a4e-9b1f-2a6c1c4e7d10"));
        assert!(!is_account_id("5f0c7b8e0d3a4a4e9b1f2a6c1c4e7d10"));
        assert!(!is_account_id("5f0c7b8e-0d3a-4a4e-9b1f-2a6c1c4e7d1g"));
    }

    #[test]
    fn linking_is_idempotent_for_one_account_and_rejects_another() {
        let mut owner = None;
        assert_eq!(link(&mut owner, "a"), Ok(Linked::New));
        assert_eq!(link(&mut owner, "a"), Ok(Linked::Existing));
        assert_eq!(link(&mut owner, "b"), Err(()));
        assert_eq!(owner.as_deref(), Some("a"));
    }

    #[test]
    fn device_capabilities_are_bounded_oldest_first() {
        let mut devices = Vec::new();
        for index in 0..MAX_DEVICE_CAPABILITIES + 2 {
            add_device(&mut devices, format!("{index:02x}"));
        }
        assert_eq!(devices.len(), MAX_DEVICE_CAPABILITIES);
        assert_eq!(devices[0], "02");
    }
}
