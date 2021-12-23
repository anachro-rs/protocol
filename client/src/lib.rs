//! # The Anachro Protocol Client Library
//!
//! This crate is used by devices acting as a Client of the Anachro Protocol

#![no_std]

pub use {
    crate::{
        client::{Client, PUBLISH_SHORTCODE_OFFSET},
        client_io::{ClientIo, ClientIoError, RecvPayload},
        table::{Table, TableError},
    },
    anachro_icd::{self, arbitrator::SubMsg, Path, PubSubPath, Version},
    postcard::{from_bytes, from_bytes_cobs, to_slice, to_slice_cobs},
    byte_slab::{ManagedArcStr, ManagedArcSlab, SlabArc},
};

mod client;
mod client_io;
mod table;

/// The main Client error type
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NotActive,
    Busy,
    UnexpectedMessage,
    ClientIoError(ClientIoError),
}

impl From<ClientIoError> for Error {
    fn from(other: ClientIoError) -> Self {
        Error::ClientIoError(other)
    }
}

/// A message that has been received FROM the Broker, TO the Client
pub struct RecvMsg<T: Table<N, SZ>, const N: usize, const SZ: usize> {
    pub path: Path<'static, N, SZ>,
    pub payload: T,
    pub arc: SlabArc<N, SZ>,
}

/// A message to be sent TO the Broker, FROM the Client
#[derive(Debug)]
pub struct SendMsg<'a, const N: usize, const SZ: usize> {
    pub buf: ManagedArcSlab<'a, N, SZ>,
    pub path: ManagedArcStr<'a, N, SZ>,
}
