//! # Client Messages
//!
//! These are messages that are sent FROM the peripheral
//! Component/Client, TO the central Arbitrator.
//!
//! The [`Component` enum](enum.Component.html) is the top level
//! message sent by Component/Clients.

use crate::{PubSubPath, Version};
use byte_slab::ManagedArcStr;
use serde::{Deserialize, Serialize};
use byte_slab_derive::Reroot;

/// Component Message
///
/// This is the primary message sent FROM the peripheral
/// Component/Client, TO the central Arbitrator.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Reroot)]
pub enum Component<'a, const N: usize, const SZ: usize> {
    /// Control Messages
    ///
    /// These are used to establish or manage the connection
    /// between the Component/Client and Arbitrator
    #[serde(borrow)]
    Control(Control<'a, N, SZ>),

    /// Pub/Sub messages
    ///
    /// These are used to send or receive Pub/Sub messages
    #[serde(borrow)]
    PubSub(PubSub<'a, N, SZ>),
}

/// Pub/Sub Message
///
/// These messages are used to communicate on the Pub/Sub
/// communication layer
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Reroot)]
pub struct PubSub<'a, const N: usize, const SZ: usize> {
    /// The path in question, common to all message types
    #[serde(borrow)]
    pub path: PubSubPath<'a, N, SZ>,

    /// The pub/sub message type
    pub ty: PubSubType<'a, N, SZ>,
}

/// Pub/Sub Message Type
///
/// The specific kind of pub/sub message
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Reroot)]
pub enum PubSubType<'a, const N: usize, const SZ: usize> {
    /// Publish Message
    ///
    /// Publish the given message/payload on the given path

    Pub {
        #[serde(borrow)]
        payload: ManagedArcStr<'a, N, SZ>,
    },

    /// Subscribe Message
    ///
    /// Subscribe to the given path
    Sub,

    /// Unsubscribe Message
    ///
    /// Unsubscribe to the given path
    Unsub,
}

/// Control Messages
///
/// These messages are used to communicate on the control layer
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Reroot)]
pub struct Control<'a, const N: usize, const SZ: usize> {
    /// Sequence Number
    ///
    /// This number is chosen by the Client/Component, and
    /// will be echoed back by the Arbitrator when replying
    pub seq: u16,

    /// Control Message Type
    ///
    /// The specific control message
    #[serde(borrow)]
    pub ty: ControlType<'a, N, SZ>,
}

/// Control Message Type
///
/// The specific kind of Control Message
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Reroot)]
pub enum ControlType<'a, const N: usize, const SZ: usize> {
    /// Register Component
    ///
    /// This message is used to establish/reset the connection
    /// between a given client and an Arbitrator
    #[serde(borrow)]
    RegisterComponent(ComponentInfo<'a, N, SZ>),

    /// Register PubSubShortID
    ///
    /// This message is used to register a path "short code",
    /// which can use a u16 instead of a full utf-8 path to save
    /// message bandwidth
    #[serde(borrow)]
    RegisterPubSubShortId(PubSubShort<'a, N, SZ>),
}

/// Information about this Component/Client needed for
/// registration
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Reroot)]
pub struct ComponentInfo<'a, const N: usize, const SZ: usize> {
    /// The name of the Client/Component
    #[serde(borrow)]
    pub name: crate::Name<'a, N, SZ>,

    /// The verson of the Client/Component
    pub version: Version,
}

/// Pub/Sub Short Code Registration
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Reroot)]
pub struct PubSubShort<'a, const N: usize, const SZ: usize> {
    /// The 'long' UTF-8 path to register
    #[serde(borrow)]
    pub long_name: ManagedArcStr<'a, N, SZ>,

    /// The 'short' u16 path to register
    pub short_id: u16,
}

#[cfg(test)]
mod test {
    use super::*;
    use postcard::{from_bytes, to_stdvec};

    #[test]
    fn ser_check() {
        let name = crate::Name::borrow_from_str("cool-board");
        let version = Version {
            major: 0,
            minor: 1,
            trivial: 0,
            misc: 123,
        };

        let msg = Component::Control(Control {
            seq: 0x0504,
            ty: ControlType::RegisterComponent(ComponentInfo { name, version }),
        });

        let ser_msg = to_stdvec(&msg).unwrap();
        assert_eq!(
            &ser_msg[..],
            &[
                0x00, // Component::Control
                0x04, 0x05, // seq
                0x00, // ControlType::RegisterComponent
                0x0A, b'c', b'o', b'o', b'l', b'-', b'b', b'o', b'a', b'r', b'd', 0x00, 0x01, 0x00,
                123,
            ]
        );

        let deser_msg: Component = from_bytes(&ser_msg).unwrap();

        assert_eq!(msg, deser_msg);
    }
}
