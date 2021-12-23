//! The Client Table Interface
//!
//! This module contains items used for defining pubsub tables, which
//! are the primary way of specifying the topics that the client is
//! interested in publishing or subscribing to.
//!
//! Each client may define their own table of publish and subscribe topics,
//! or may use a table defined by a common shared library. Different clients
//! do not need to have the same table, but for successful operation, all
//! clients must agree on the same data type used for a given path or wildcard
//! path topic.

use anachro_icd::arbitrator::SubMsg;
use postcard;
use byte_slab::{SlabArc, ManagedArcStr};

/// An error type used by the `Table` trait
#[derive(Debug, PartialEq, Eq)]
pub enum TableError {
    NoMatch,
    Postcard(postcard::Error),
    Reroot,
}

/// A trait describing publish and subscription topics
///
/// This is used to interact with the `Client` interface.
pub trait Table<const N: usize, const SZ: usize>: Sized {
    type Message: Sized;

    /// Create a Table item from a given SubMsg`
    fn from_sub_msg(
        matched_path: &'static str,
        path: ManagedArcStr<'static, N, SZ>,
        msg: SubMsg<'static, N, SZ>,
        arc: &SlabArc<N, SZ>
    ) -> Result<Self::Message, TableError>;
}
