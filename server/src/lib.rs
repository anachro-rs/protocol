//! # The Anachro Protocol Server/Broker Library
//!
//! This crate is used by devices acting as a Server/Broker of the Anachro Protocol

#![no_std]

use {
    anachro_icd::{
        arbitrator::{self, Arbitrator, Control as AControl, ControlError, SubMsg},
        component::{
            Component, ComponentInfo, Control, ControlType, PubSub, PubSubShort, PubSubType,
        },
    },
    byte_slab::ManagedArcSlab,
    core::default::Default,
    heapless::Vec,
};

use core::ops::Deref;

pub use anachro_icd::{self, Name, Path, PubSubPath, Uuid, Version};
use defmt::Format;
pub use postcard::from_bytes_cobs;

type ClientStore<const N: usize, const SZ: usize> = Vec<Client<N, SZ>, 8>;

/// The Broker Interface
///
/// This is the primary interface for devices acting as a broker.
///
/// Currently the max capacity is fixed with a maximum of 8
/// clients connected. Each Client may subscribe up to 8 topics.
/// Each Client may register up to 8 shortcodes.
///
/// In the future, these limits may be configurable.
///
/// As a note, the Broker currently creates a sizable object, due
/// to the fixed upper limits
#[derive(Default)]
pub struct Broker<const N: usize, const SZ: usize> {
    clients: ClientStore<N, SZ>,
}

#[derive(Debug, PartialEq, Eq, Format)]
pub enum ServerError {
    ClientAlreadyRegistered,
    UnknownClient,
    ClientDisconnected,
    ConnectionError,
    ResourcesExhausted,
    UnknownShortcode,
    InternalError,
    DeserializeFailure,
}

pub const fn reset_msg<const N: usize, const SZ: usize>() -> Arbitrator<'static, N, SZ> {
    Arbitrator::Control(AControl {
        response: Err(ControlError::ResetConnection),
        seq: 0,
    })
}

// Public Interfaces
impl<const N: usize, const SZ: usize> Broker<N, SZ> {
    /// Create a new broker with no clients attached
    #[inline(always)]
    pub fn new() -> Self {
        Broker::default()
    }

    /// Register a client to the broker
    ///
    /// This can be done dynamically, e.g. when a client connects for the
    /// first time, e.g. a TCP session is established, or the first packet
    /// is received, or can be done ahead-of-time, e.g. when communicating
    /// with a fixed set of wired devices.
    ///
    /// Clients must be registered before messages from them can be processed.
    ///
    /// If an already-registered client is re-registered, they will be reset to
    /// an initial connection state, dropping all subscriptions or shortcodes.
    pub fn register_client(&mut self, id: &Uuid) -> Result<(), ServerError> {
        if self.clients.iter().find(|c| &c.id == id).is_none() {
            self.clients
                .push(Client {
                    id: *id,
                    state: ClientState::SessionEstablished,
                })
                .map_err(|_| ServerError::ResourcesExhausted)?;
            Ok(())
        } else {
            Err(ServerError::ClientAlreadyRegistered)
        }
    }

    /// Remove a client from the broker
    ///
    /// This could be necessary if the connection to a client breaks or times out
    /// Once removed, no further messages to or from this client will be processed
    pub fn remove_client(&mut self, id: &Uuid) -> Result<(), ServerError> {
        let pos = self
            .clients
            .iter()
            .position(|c| &c.id == id)
            .ok_or(ServerError::UnknownClient)?;
        self.clients.swap_remove(pos);
        Ok(())
    }

    /// Reset a client registered with the broker, without removing it
    ///
    /// This could be necessary if the connection to a client breaks or times out.
    pub fn reset_client(&mut self, id: &Uuid) -> Result<(), ServerError> {
        let mut client = self.client_by_id_mut(id)?;
        client.state = ClientState::SessionEstablished;
        Ok(())
    }

    /// Process a single message from a client
    ///
    /// A message from a client will be processed. If processing this message
    /// generates responses that need to be sent (e.g. a publish occurs and
    /// subscribed clients should be notified, or if the broker is responding
    /// to a request from the client), they will be returned, and the messages
    /// should be sent to the appropriate clients.
    ///
    /// Requests and Responses are addressed by the Uuid registered for each client
    ///
    /// **NOTE**: If an error occurs, you probably should send a `RESET_MESSAGE` to
    /// that client to force them to reconnect. You may also want to `remove_client`
    /// or `reset_client`, depending on the situation. This will hopefully be handled
    /// automatically in the future.
    pub fn process_msg<SI: ServerIoIn<N, SZ>, SO: ServerIoOut<N, SZ>>(
        &mut self,
        sio_in: &mut SI,
        sio_out: &mut SO,
    ) -> Result<(), ServerError> {
        let Request { source, msg } = match sio_in.recv() {
            Ok(Some(req)) => {
                defmt::info!("Broker: Got Request");
                req
            }
            Ok(None) => return Ok(()),
            Err(e) => {
                // TODO: Actual error handling
                match e {
                    ServerIoError::ResponsePushFailed => {
                        defmt::error!("Broker: Got Disconnected");
                        // TODO: This is probably not always right. For now, report client
                        // disconnection to reset connection
                        return Err(ServerError::ClientDisconnected);
                    }
                    ServerIoError::DeserializeFailure => {
                        defmt::error!("Broker: Got Bad Deserialize");
                        return Err(ServerError::DeserializeFailure);
                    }
                }
            }
        };

        match msg {
            Component::Control(ctrl) => {
                defmt::info!("Broker: Got Control");
                let client = self.client_by_id_mut(&source)?;

                if let Some(msg) = client.process_control(ctrl)? {
                    defmt::info!("Broker: Reply Control");
                    sio_out
                        .push_response(msg)
                        .map_err(|_| ServerError::ResourcesExhausted)?;
                }
            }
            Component::PubSub(PubSub { path, ty }) => match ty {
                PubSubType::Pub { payload } => {
                    defmt::info!("Broker: Got Publish");
                    self.process_publish(sio_out, path, payload, source)?;
                }
                PubSubType::Sub => {
                    defmt::info!("Broker: Got Subscribe");
                    let client = self.client_by_id_mut(&source)?;
                    sio_out
                        .push_response(client.process_subscribe(path)?)
                        .map_err(|_| ServerError::ResourcesExhausted)?;
                }
                PubSubType::Unsub => {
                    let client = self.client_by_id_mut(&source)?;
                    client.process_unsub(path)?;
                    todo!()
                }
            },
        }

        Ok(())
    }
}

// Private interfaces
impl<const N: usize, const SZ: usize> Broker<N, SZ> {
    fn client_by_id_mut(&mut self, id: &Uuid) -> Result<&mut Client<N, SZ>, ServerError> {
        self.clients
            .iter_mut()
            .find(|c| &c.id == id)
            .ok_or(ServerError::UnknownClient)
    }

    fn process_publish<SO: ServerIoOut<N, SZ>>(
        &mut self,
        sio: &mut SO,
        path: PubSubPath<'static, N, SZ>,
        payload: ManagedArcSlab<'static, N, SZ>,
        source: Uuid,
    ) -> Result<(), ServerError> {
        // TODO: Make sure we're not publishing to wildcards

        // First, find the sender's path
        let source_id = self
            .clients
            .iter()
            .filter_map(|c| c.state.as_connected().ok().map(|x| (c, x)))
            .find(|(c, _x)| c.id == source)
            .ok_or(ServerError::UnknownClient)?;
        let path = match path {
            PubSubPath::Long(lp) => lp,
            PubSubPath::Short(sid) => source_id
                .1
                .shortcuts
                .iter()
                .find(|s| s.short == sid)
                .ok_or(ServerError::UnknownShortcode)?
                .long
                .clone(),
        };

        // Then, find all applicable destinations, max of 1 per destination
        'client: for (client, state) in self
            .clients
            .iter()
            .filter_map(|c| c.state.as_connected().ok().map(|x| (c, x)))
        {
            if client.id == source {
                // Don't send messages back to the sender
                continue;
            }

            for subt in state.subscriptions.iter() {
                if anachro_icd::matches(subt.deref(), path.deref()) {
                    let msg = Arbitrator::PubSub(Ok(arbitrator::PubSubResponse::SubMsg(SubMsg {
                        path: PubSubPath::Long(path.clone()),
                        payload: payload.clone(),
                    })));
                    sio.push_response(Response {
                        dest: client.id,
                        msg,
                    })
                    .map_err(|_| ServerError::ResourcesExhausted)?;
                    continue 'client;
                }
            }
        }

        Ok(())
    }
}

struct Client<const N: usize, const SZ: usize> {
    id: Uuid,
    state: ClientState<N, SZ>,
}

impl<const N: usize, const SZ: usize> Client<N, SZ> {
    fn process_control(&mut self, ctrl: Control<'static, N, SZ>) -> Result<Option<Response<'static, N, SZ>>, ServerError> {
        let response;

        let next = match ctrl.ty {
            ControlType::RegisterComponent(ComponentInfo { name, version }) => match &self.state {
                ClientState::SessionEstablished | ClientState::Connected(_) => {
                    defmt::info!("Broker: Got Register");

                    let resp = Arbitrator::Control(arbitrator::Control {
                        seq: ctrl.seq,
                        response: Ok(arbitrator::ControlResponse::ComponentRegistration(self.id)),
                    });

                    response = Some(Response {
                        dest: self.id,
                        msg: resp,
                    });

                    defmt::info!("Broker: Reply Connected");

                    Some(ClientState::Connected(ConnectedState {
                        _name: name,
                        _version: version,
                        subscriptions: Vec::new(),
                        shortcuts: Vec::new(),
                    }))
                }
            },
            ControlType::RegisterPubSubShortId(PubSubShort {
                long_name,
                short_id,
            }) => {
                let state = self.state.as_connected_mut()?;

                if long_name.contains('#') || long_name.contains('+') {
                    // TODO: How to handle wildcards + short names?
                    let resp = Arbitrator::Control(arbitrator::Control {
                        seq: ctrl.seq,
                        response: Err(arbitrator::ControlError::NoWildcardsInShorts),
                    });

                    response = Some(Response {
                        dest: self.id,
                        msg: resp,
                    });
                } else {
                    let shortcut_exists = state
                        .shortcuts
                        .iter()
                        .any(|sc| (sc.long.deref() == long_name.deref()) && (sc.short == short_id));

                    if !shortcut_exists {
                        state
                            .shortcuts
                            .push(Shortcut {
                                long: long_name,
                                short: short_id,
                            })
                            .map_err(|_| ServerError::ResourcesExhausted)?;
                    }

                    let resp = Arbitrator::Control(arbitrator::Control {
                        seq: ctrl.seq,
                        response: Ok(arbitrator::ControlResponse::PubSubShortRegistration(
                            short_id,
                        )),
                    });

                    response = Some(Response {
                        dest: self.id,
                        msg: resp,
                    });
                }

                // TODO: Dupe check?

                None
            }
        };

        if let Some(next) = next {
            self.state = next;
        }

        Ok(response)
    }

    fn process_subscribe(
        &mut self,
        path: PubSubPath<'static, N, SZ>,
    ) -> Result<Response<'static, N, SZ>, ServerError> {
        let state = self.state.as_connected_mut()?;

        // Determine canonical path
        let path_str = match path {
            PubSubPath::Long(ref lp) => lp.clone(),
            PubSubPath::Short(sid) => state
                .shortcuts
                .iter()
                .find(|s| s.short == sid)
                .ok_or(ServerError::UnknownShortcode)?
                .long
                .clone(),
        };

        // Only push if not a dupe
        if state
            .subscriptions
            .iter()
            .find(|s| s.deref() == path_str.deref())
            .is_none()
        {
            state
                .subscriptions
                .push(path_str)
                .map_err(|_| ServerError::ResourcesExhausted)?;
        }

        let resp = Arbitrator::PubSub(Ok(arbitrator::PubSubResponse::SubAck {
            path: path.clone(),
        }));

        Ok(Response {
            dest: self.id,
            msg: resp,
        })
    }

    fn process_unsub(&mut self, _path: PubSubPath<'static, N, SZ>) -> Result<(), ServerError> {
        let _state = self.state.as_connected_mut()?;

        todo!()
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum ClientState<const N: usize, const SZ: usize> {
    SessionEstablished,
    Connected(ConnectedState<N, SZ>),
}

impl<const N: usize, const SZ: usize> ClientState<N, SZ> {
    fn as_connected(&self) -> Result<&ConnectedState<N, SZ>, ServerError> {
        match self {
            ClientState::Connected(state) => Ok(state),
            _ => Err(ServerError::ClientDisconnected),
        }
    }

    fn as_connected_mut(&mut self) -> Result<&mut ConnectedState<N, SZ>, ServerError> {
        match self {
            ClientState::Connected(ref mut state) => Ok(state),
            _ => Err(ServerError::ClientDisconnected),
        }
    }
}

#[derive(Debug)]
struct ConnectedState<const N: usize, const SZ: usize> {
    _name: Name<'static, N, SZ>,
    _version: Version,
    subscriptions: Vec<Path<'static, N, SZ>, 8>,
    shortcuts: Vec<Shortcut<N, SZ>, 8>,
}

#[derive(Debug)]
struct Shortcut<const N: usize, const SZ: usize> {
    long: Path<'static, N, SZ>,
    short: u16,
}

/// A request FROM the Client, TO the Broker
///
/// This message is addressed by a UUID used when registering the client
#[derive(Debug)]
pub struct Request<'a, const N: usize, const SZ: usize> {
    pub source: Uuid,
    pub msg: Component<'a, N, SZ>,
}

/// A response TO the Client, FROM the Broker
///
/// This message is addressed by a UUID used when registering the client
#[derive(Debug)]
pub struct Response<'a, const N: usize, const SZ: usize> {
    pub dest: Uuid,
    pub msg: Arbitrator<'a, N, SZ>,
}

#[derive(Debug, Format)]
pub enum ServerIoError {
    ResponsePushFailed,
    DeserializeFailure,
}

pub trait ServerIoIn<const N: usize, const SZ: usize> {
    fn recv(&mut self) -> Result<Option<Request<'static, N, SZ>>, ServerIoError>;
}

pub trait ServerIoOut<const N: usize, const SZ: usize> {
    fn push_response(&mut self, resp: Response<'static, N, SZ>) -> Result<(), ServerIoError>;
}

impl<'resp, const N: usize, const SZ: usize, const CT: usize> ServerIoOut<N, SZ> for Vec<Response<'static, N, SZ>, CT>
{
    fn push_response(&mut self, resp: Response<'static, N, SZ>) -> core::result::Result<(), ServerIoError> {
        self.push(resp)
            .map_err(|_| ServerIoError::ResponsePushFailed)
    }
}
