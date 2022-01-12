//! # Anachro ICD
//!
//! This is the Interface Control Document (ICD) for the Anachro PC
//! communication protocol.
//!
//! This library defines the types used on by clients and servers of
//! the anachro protocol.
//!
//! This library is currently primarily focused on a Pub/Sub style
//! protocol, but will add support for Object Store and Mailbox
//! style communication in the future.
#![no_std]

use core::hash::Hash;
use serde::{
    Deserialize, Serialize,
};

pub mod arbitrator;
pub mod component;
use byte_slab::{ManagedArcStr, Reroot};
use byte_slab_derive::Reroot;

#[toml_cfg::toml_config]
pub struct Config {

    /// A const value for the Maximum Pub/Sub Path
    #[default(127)]
    max_path_len: usize,

    /// A const value for the maximum device name
    #[default(32)]
    max_name_len: usize,

    #[default(32)]
    slab_count: usize,

    #[default(512)]
    slab_size: usize,
}

/// Publish/Subscribe Path - Short or Long
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Reroot)]
pub enum PubSubPath<'a> {
    /// A long form, UTF-8 Path
    #[serde(borrow)]
    Long(Path<'a>),

    /// A short form, 'memoized' path
    ///
    /// Pre-registered with the server
    Short(u16),
}

/// Device version - SemVer
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy, Reroot)]
pub struct Version {
    /// Major Semver Version
    pub major: u8,

    /// Minor Semver Version
    pub minor: u8,

    /// Trivial Semver Version
    pub trivial: u8,

    /// Misc Version
    ///
    /// Could be build number, etc.
    pub misc: u8,
}

/// A Pub/Sub Path as a Managed String
pub type Path<'a> = ManagedArcStr<'a, {CONFIG.slab_count}, {CONFIG.slab_size}>;

/// A device name as a Managed String
pub type Name<'a> = ManagedArcStr<'a, {CONFIG.slab_count}, {CONFIG.slab_size}>;

/// A UUID as a block of 16 bytes
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// Create a new UUID from an array of bytes
    pub const fn from_bytes(by: [u8; 16]) -> Self {
        Uuid(by)
    }

    /// Obtain the UUID contents as a borrowed array of bytes
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Obtain the UUID contents as a slice of bytes
    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }
}

// TODO: My derive doesn't handle arrays yet
impl Reroot for Uuid {
    type Retval = Self;

    fn reroot(self, _key: &byte_slab::RerooterKey) -> Result<Self::Retval, ()> {
        Ok(self)
    }
}

/// A function for matching pub/sub paths
///
/// ## Examples
///
/// ```
/// # use anachro_icd::matches;
/// #
/// assert!(matches(
///  "/+/temperature/#",
///  "/dev_1/temperature/front",
/// ));
/// ```
pub fn matches(subscr: &str, publ: &str) -> bool {
    if subscr.is_empty() || publ.is_empty() {
        return false;
    }

    let mut s_iter = subscr.split('/');
    let mut p_iter = publ.split('/');

    loop {
        match (s_iter.next(), p_iter.next()) {
            (Some("+"), Some(_)) => continue,
            (Some("#"), _) | (None, None) => return true,
            (Some(lhs), Some(rhs)) if lhs == rhs => continue,
            _ => return false,
        }
    }
}
