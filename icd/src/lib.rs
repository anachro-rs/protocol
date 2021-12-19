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

use core::{hash::Hash, marker::PhantomData, str::FromStr};
use heapless::String;
use serde::{
    de::{Deserializer, Visitor},
    ser::Serializer,
    Deserialize, Serialize,
};

pub mod arbitrator;
pub mod component;

/// A const value for the Maximum Pub/Sub Path
pub const MAX_PATH_LEN: usize = 127;

/// A const value for the maximum device name
pub const MAX_NAME_LEN: usize = 32;

/// Publish/Subscribe Path - Short or Long
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, defmt::Format)]
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
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy, defmt::Format)]
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
pub type Path<'a> = ManagedString<'a, MAX_PATH_LEN>;

/// A device name as a Managed String
pub type Name<'a> = ManagedString<'a, MAX_NAME_LEN>;

/// A borrowed or owned string
///
/// Basically like CoW, but with heapless::String
#[derive(Debug, Eq, PartialEq, Clone, defmt::Format)]
pub enum ManagedString<'a, const T: usize>
{
    Owned(String<T>),
    Borrow(&'a str),
}

impl<'a, const N: usize> Serialize for ManagedString<'a, N>
{
    /// We can serialize our managed string as a str
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'a, 'de: 'a, const N: usize> Deserialize<'de> for ManagedString<'a, N>
where
{
    /// We can deserialize as a borrowed str
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(ManagedStringVisitor::new())
    }
}

struct ManagedStringVisitor<'a, const N: usize>
{
    _lt: PhantomData<&'a ()>,
}

impl<'a, const N: usize> ManagedStringVisitor<'a, N>
{
    fn new() -> ManagedStringVisitor<'a, N> {
        Self {
            _lt: PhantomData,
        }
    }
}

impl<'de: 'a, 'a, const N: usize> Visitor<'de> for ManagedStringVisitor<'a, N>
{
    type Value = ManagedString<'a, N>;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(formatter, "a borrowable str")
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        // Always borrow
        Ok(ManagedString::Borrow(value))
    }
}

impl<'a, const N: usize> ManagedString<'a, N>
{
    /// Obtain self as a string slice
    pub fn as_str(&self) -> &str {
        match self {
            ManagedString::Owned(o) => o.as_str(),
            ManagedString::Borrow(s) => s,
        }
    }

    /// Retrieve a borrowed copy of the string
    pub fn as_borrowed(&'a self) -> ManagedString<'a, N> {
        ManagedString::Borrow(self.as_str())
    }

    /// Attempt to create an Owned string from a given str
    ///
    /// May fail if the heapless::String is not large enough to
    /// contain this slice
    pub fn try_from_str(input: &str) -> Result<ManagedString<'static, N>, ()> {
        Ok(ManagedString::Owned(String::from_str(input)?))
    }

    /// Create a Borrowed string from a given str
    pub fn borrow_from_str(input: &str) -> ManagedString<N> {
        ManagedString::Borrow(input)
    }

    /// Attempt to convert to an Owned string from the current state
    ///
    /// May fail if the heapless::String is not large enough to
    /// contain this slice
    pub fn try_to_owned(&self) -> Result<ManagedString<'static, N>, ()> {
        ManagedString::try_from_str(self.as_str())
    }
}

/// A UUID as a block of 16 bytes
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy, Hash, defmt::Format)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// Create a new UUID from an array of bytes
    pub const fn from_bytes(by: [u8; 16]) -> Self {
        Uuid(by)
    }

    /// Obtain the UUID contents as a borrowed array of bytes
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Obtain the UUID contents as a slice of bytes
    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
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
