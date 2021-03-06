//! # Arbitrator Messages
//!
//! These are messages that are sent FROM the central Arbitrator,
//! to the peripheral Components/Clients.
//!
//! The [`Arbitrator` enum](enum.Arbitrator.html) is the top level
//! message sent by the Arbitrator.

use crate::{PubSubPath, Uuid, CONFIG};
use byte_slab::ManagedArcSlab;
use serde::{Deserialize, Serialize};
use byte_slab_derive::Reroot;

/// The primary Arbitrator mesage
///
/// These are all messages that are sent FROM the Arbitrator,
/// TO the Components/Clients
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Reroot)]
pub enum Arbitrator<'a> {
    /// Control messages
    ///
    /// Control messages are intended to be the primary
    /// management channel between an Arbitrator and a
    /// Component/Client
    Control(Control),

    /// Pub/Sub messages
    ///
    /// These are messages sent on the Publish/Subscribe
    /// channel
    #[serde(borrow)]
    PubSub(Result<PubSubResponse<'a>, PubSubError>),

    /// Object Store messages
    ///
    /// These are messages intended for the Object Store
    /// channel for sending bulk messages.
    ///
    /// This functionality has not yet been implemented.
    ObjStore,

    /// Mailbox messages
    ///
    /// These are messages intended for the Mailbox layer,
    /// including guaranteed delivery messages and bulk
    /// message delivery
    ///
    /// This functionality has not yet been implemented.
    Mailbox,
}

/// An Arbitrator Response to a Pub/Sub message
///
/// These are any Arbitrator -> Client relevant messages
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Reroot)]
pub enum PubSubResponse<'a> {
    /// Subscription Acknowledgement
    ///
    /// Sent to acknowledge the reception of subscription
    /// request from a client
    SubAck {
        #[serde(borrow)]
        path: PubSubPath<'a>,
    },

    /// Subscription Message
    ///
    /// This is a "subscribed to" message, containing a
    /// payload sent by another Client
    SubMsg(SubMsg<'a>),
}

/// Subscription Message
///
/// This is a message that has been subscribed to by a
/// client.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Reroot)]
pub struct SubMsg<'a> {
    /// The path that this message was sent to
    ///
    /// Note: If the client used wildcard subscribe(s), this
    /// may not match the subscription text
    #[serde(borrow)]
    pub path: PubSubPath<'a>,

    /// The payload sent along with the message
    #[serde(borrow)]
    pub payload: ManagedArcSlab<'a, {CONFIG.slab_count}, {CONFIG.slab_size}>,
}

/// Control Message
///
/// This is the 'control channel', used for establishing
/// and managing connections between the Arbitrator and
/// Client(s).
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Clone, Reroot)]
pub struct Control {
    /// Sequence Number
    ///
    /// This number is provided by the client. The Arbitrator
    /// will always respond with the same sequence number when
    /// replying to a specific message
    pub seq: u16,

    /// Response
    ///
    /// The arbitrator response to the client request
    pub response: Result<ControlResponse, ControlError>,
}

/// Control Response
///
/// A successful response to a Client's request
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Reroot)]
pub enum ControlResponse {
    /// The client/component has registered
    ComponentRegistration(Uuid),

    /// The client has registered a Pub/Sub path shortcode
    PubSubShortRegistration(u16),
}

/// Control Message Errors
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Reroot)]
pub enum ControlError {
    NoWildcardsInShorts,
    ResetConnection,
}

/// Publish/Subscribe Errors
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Reroot)]
pub enum PubSubError {}

// #[cfg(test)]
// mod test {
//     use super::*;
//     use postcard::{from_bytes, to_stdvec};

//     #[test]
//     fn ser_check() {
//         let uuid = Uuid::from_bytes([
//             0xd0, 0x36, 0xe7, 0x3b, 0x23, 0xec, 0x4f, 0x60, 0xac, 0xcb, 0x0e, 0xdd, 0xb6, 0x17,
//             0xf4, 0x71,
//         ]);
//         let msg = Arbitrator::Control(Control {
//             seq: 0x0405,
//             response: Ok(ControlResponse::ComponentRegistration(uuid)),
//         });

//         let ser_msg = to_stdvec(&msg).unwrap();
//         assert_eq!(
//             &ser_msg[..],
//             &[
//                 0x00, // Arbitrator::Control
//                 0x05, 0x04, // seq
//                 0x00, // OK
//                 0x00, // ControlResponse::ComponentRegistration
//                 0xd0, 0x36, 0xe7, 0x3b, 0x23, 0xec, 0x4f, 0x60, 0xac, 0xcb, 0x0e, 0xdd, 0xb6, 0x17,
//                 0xf4, 0x71,
//             ],
//         );

//         let deser_msg: Arbitrator = from_bytes(&ser_msg).unwrap();
//         assert_eq!(deser_msg, msg);
//     }
// }
