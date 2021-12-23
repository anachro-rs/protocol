//! The Client interface
//!
//! This is the primary interface used by clients. It is used to track
//! the state of a connection, and process any incoming or outgoing messages

use {
    core::ops::Deref,
    crate::{client_io::{ClientIo, RecvPayload}, table::Table, Error, RecvMsg},
    anachro_icd::{
        self,
        arbitrator::{Arbitrator, Control as AControl, ControlResponse, PubSubResponse},
        component::{
            Component, ComponentInfo, Control as CControl, ControlType, PubSub, PubSubShort,
            PubSubType,
        },
        Name, Path, PubSubPath, Uuid, Version,
        matches,
    },
    byte_slab::{ManagedArcStr, ManagedArcSlab},
};

/// The shortcode offset used for Publish topics
///
/// For now, I have defined `0x0000..=0x7FFF` as the range used
/// for subscription topic shortcodes, and `0x8000..=0xFFFF` as
/// the range used for publish topic shortcodes. This is an
/// implementation detail, and should not be relied upon.
pub const PUBLISH_SHORTCODE_OFFSET: u16 = 0x8000;

#[derive(Debug)]
enum ClientState {
    Disconnected,
    PendingRegistration,
    Registered,
    Subscribing,
    Subscribed,
    ShortCodingSub,
    ShortCodingPub,
    Active,
}

impl ClientState {
    pub(crate) fn as_active(&self) -> Result<(), Error> {
        match self {
            ClientState::Active => Ok(()),
            _ => Err(Error::NotActive),
        }
    }
}

/// The Client interface
///
/// This is the primary interface used by clients. It is used to track
/// the state of a connection, and process any incoming or outgoing messages
pub struct Client<const N: usize, const SZ: usize> {
    state: ClientState,
    // TODO: This should probably just be a &'static str
    name: Name<'static, N, SZ>,
    version: Version,
    ctr: u16,
    sub_paths: &'static [&'static str],
    pub_short_paths: &'static [&'static str],
    timeout_ticks: Option<u8>,
    uuid: Uuid,
    current_tick: u8,
    current_idx: usize,
}

impl<const N: usize, const SZ: usize> Client<N, SZ> {
    /// Create a new client instance
    ///
    /// ## Parameters
    ///
    /// ### `name`
    ///
    /// The name of this device or client
    ///
    /// ### `version`
    ///
    /// The semantic version number of this client
    ///
    /// ### `ctr_init`
    ///
    /// A value to initialize the control counter.
    ///
    /// You may choose to initialize this with a fixed or random value
    ///
    /// ### `sub_paths`
    ///
    /// The subscription paths that the device is interested in
    ///
    /// This is typically provided by the `Table::sub_paths()` method on
    /// a table type generated using the `pubsub_table!()` macro.
    ///
    /// ### `pub_paths`
    ///
    /// The publishing paths that the device is interested in
    ///
    /// This is typically provided by the `Table::pub_paths()` method on
    /// a table type generated using the `pubsub_table!()` macro.
    ///
    /// ### `timeout_ticks`
    ///
    /// The number of ticks used to time-out waiting for certain responses
    /// before retrying automatically.
    ///
    /// Set to `None` to disable automatic retries. This will require the
    /// user to manually call `Client::reset_connection()` if a message is
    /// lost.
    ///
    /// Ticks are counted by calls to `Client::process_one()`, which should be
    /// called at a semi-regular rate. e.g. if you call `process_one()` every
    /// 10ms, a `timeout_ticks: Some(100)` would automatically timeout after
    /// 1s of waiting for a response.
    pub fn new(
        name: &'static str,
        version: Version,
        ctr_init: u16,
        sub_paths: &'static [&'static str],
        pub_short_paths: &'static [&'static str],
        timeout_ticks: Option<u8>,
    ) -> Self {
        Self {
            name: ManagedArcStr::Borrowed(name),
            version,
            ctr: ctr_init,
            state: ClientState::Disconnected,
            sub_paths,
            pub_short_paths,
            timeout_ticks,
            uuid: Uuid::from_bytes([0u8; 16]),
            current_tick: 0,
            current_idx: 0,
        }
    }

    /// Reset the client connection
    ///
    /// This immediately disconnects the client, at which point
    /// it will begin attemption to re-establish a connection to
    /// the broker.
    pub fn reset_connection(&mut self) {
        #[cfg(feature = "defmt")] defmt::error!("Resetting Connection.");
        self.state = ClientState::Disconnected;
        self.current_tick = 0;
        self.current_idx = 0;
    }

    /// Obtain the `Uuid` assigned by the broker to this client
    ///
    /// If the client is not connected, `None` will be returned.
    pub fn get_id(&self) -> Option<&Uuid> {
        if self.is_connected() {
            Some(&self.uuid)
        } else {
            None
        }
    }

    /// Is the client connected?
    pub fn is_connected(&self) -> bool {
        self.state.as_active().is_ok()
    }

    /// Publish a message
    ///
    /// This interface publishes a message from the client to the broker.
    ///
    /// ## Parameters
    ///
    /// ### `cio`
    ///
    /// This is the `ClientIo` instance used by the client
    ///
    /// ### `path`
    ///
    /// This is the path to publish to. This is typically created by using
    /// the `Table::serialize()` method, which returns a path and the serialized
    /// payload
    ///
    /// ### `payload`
    ///
    /// The serialized payload to publish. This is typically created by using
    /// the `Table::serialize()` method, which returns a path and the serialized
    /// payload
    pub fn publish<'a, 'b: 'a, C: ClientIo<N, SZ>>(
        &'b self,
        cio: &mut C,
        path: ManagedArcStr<'static, N, SZ>,
        payload: ManagedArcSlab<'static, N, SZ>,
    ) -> Result<(), Error> {
        #[cfg(feature = "defmt")] defmt::info!("Publishing message.");
        self.state.as_active()?;

        let path = match self.pub_short_paths.iter().position(|pth| &path.deref() == pth) {
            Some(short) => PubSubPath::Short((short as u16) | PUBLISH_SHORTCODE_OFFSET),
            None => PubSubPath::Long(path),
        };

        let msg = Component::PubSub(PubSub {
            path,
            ty: PubSubType::Pub { payload },
        });

        cio.send(msg)?;

        Ok(())
    }

    /// Process a single incoming message
    ///
    /// This function *must* be called regularly to process messages
    /// that have been received by the broker. It is suggested to call
    /// it at regular intervals if you are using the `timeout_ticks` option
    /// when creating the Client.
    ///
    /// If a subscription message has been received, it will be returned
    /// with the path that was published to. If the Client has subscribed
    /// using a wildcard, it may not exactly match the subscription topic
    ///
    /// The `anachro-icd::matches` function can be used to compare if a topic
    /// matches a given fixed or wildcard path, if necessary.
    pub fn process_one<C: ClientIo<N, SZ>, T: Table<N, SZ>>(
        &mut self,
        cio: &mut C,
    ) -> Result<Option<RecvMsg<T, N, SZ>>, Error> {
        let mut response: Option<RecvMsg<T, N, SZ>> = None;

        match &mut self.state {
            // =====================================
            // Disconnected
            // =====================================
            ClientState::Disconnected => {
                self.disconnected(cio)?;
            }

            // =====================================
            // Pending Registration
            // =====================================
            ClientState::PendingRegistration => {
                self.pending_registration(cio)?;

                if self.timeout_violated() {
                    // println!("violated!");
                    #[cfg(feature = "defmt")] defmt::warn!("Registration Timeout violated! Going to disconnected state");
                    self.state = ClientState::Disconnected;
                    self.current_tick = 0;
                }
            }

            // =====================================
            // Registered
            // =====================================
            ClientState::Registered => {
                self.registered(cio)?;
            }

            // =====================================
            // Subscribing
            // =====================================
            ClientState::Subscribing => {
                self.subscribing(cio)?;

                if self.timeout_violated() {
                    #[cfg(feature = "defmt")] defmt::info!("Sub timeout. Resending");
                    let msg = Component::PubSub(PubSub {
                        path: PubSubPath::Long(ManagedArcStr::Borrowed(
                            self.sub_paths[self.current_idx],
                        )),
                        ty: PubSubType::Sub,
                    });

                    cio.send(msg)?;

                    self.current_tick = 0;
                }
            }

            // =====================================
            // Subscribed
            // =====================================
            ClientState::Subscribed => {
                self.subscribed(cio)?;
            }

            // =====================================
            // ShortCoding
            // =====================================
            ClientState::ShortCodingSub => {
                self.shortcoding_sub(cio)?;

                if self.timeout_violated() {
                    #[cfg(feature = "defmt")] defmt::info!("SCS timeout. Resending");
                    self.ctr = self.ctr.wrapping_add(1);

                    let msg = Component::Control(CControl {
                        seq: self.ctr,
                        ty: ControlType::RegisterPubSubShortId(PubSubShort {
                            long_name: ManagedArcStr::Borrowed(self.sub_paths[self.current_idx]),
                            short_id: self.current_idx as u16,
                        }),
                    });

                    cio.send(msg)?;

                    self.current_tick = 0;
                }
            }

            ClientState::ShortCodingPub => {
                self.shortcoding_pub(cio)?;

                if self.timeout_violated() {
                    #[cfg(feature = "defmt")] defmt::info!("SCP timeout. Resending");
                    self.ctr = self.ctr.wrapping_add(1);

                    let msg = Component::Control(CControl {
                        seq: self.ctr,
                        ty: ControlType::RegisterPubSubShortId(PubSubShort {
                            long_name: ManagedArcStr::Borrowed(self.pub_short_paths[self.current_idx]),
                            short_id: (self.current_idx as u16) | PUBLISH_SHORTCODE_OFFSET,
                        }),
                    });

                    cio.send(msg)?;

                    self.current_tick = 0;
                }
            }

            // =====================================
            // Active
            // =====================================
            ClientState::Active => {
                response = self.active(cio)?;
            }
        };

        Ok(response)
    }
}

// Private interfaces for the client. These are largely used to
// process incoming messages and handle state
impl<const N: usize, const SZ: usize> Client<N, SZ> {
    /// Have we reached the timeout limit provided by the user?
    fn timeout_violated(&self) -> bool {
        match self.timeout_ticks {
            Some(ticks) if ticks <= self.current_tick => true,
            Some(_) => false,
            None => false,
        }
    }

    /// Process messages while in a `ClientState::Disconnected` state
    fn disconnected<C: ClientIo<N, SZ>>(&mut self, cio: &mut C) -> Result<(), Error> {
        self.ctr += 1;

        #[cfg(feature = "defmt")] defmt::info!("Disconnected -> Pending Registration");

        let resp = Component::Control(CControl {
            seq: self.ctr,
            ty: ControlType::RegisterComponent(ComponentInfo {
                name: self.name.clone(),
                version: self.version,
            }),
        });

        cio.send(resp)?;

        #[cfg(feature = "defmt")] defmt::info!("PR sent.");

        self.state = ClientState::PendingRegistration;
        self.current_tick = 0;

        Ok(())
    }

    /// Process messages while in a `ClientState::PendingRegistration state`
    fn pending_registration<C: ClientIo<N, SZ>>(&mut self, cio: &mut C) -> Result<(), Error> {
        let msg = cio.recv()?;
        let msg = match msg {
            Some(msg) => msg,
            None => {
                self.current_tick = self.current_tick.saturating_add(1);
                return Ok(());
            }
        };

        #[cfg(feature = "defmt")] defmt::info!("Got PR response");
        // println!("got pr mesg!");

        if let Arbitrator::Control(AControl { seq, response }) = msg.msg {
            if seq != self.ctr {
                #[cfg(feature = "defmt")] defmt::warn!("ctr mismatch! {:?} {:?}", seq, self.ctr);
                self.current_tick = self.current_tick.saturating_add(1);
                // TODO, restart connection process? Just disregard?
                Err(Error::UnexpectedMessage)
            } else if let Ok(ControlResponse::ComponentRegistration(uuid)) = response {
                #[cfg(feature = "defmt")] defmt::info!("Registered!");
                self.uuid = uuid;
                self.state = ClientState::Registered;
                self.current_tick = 0;
                Ok(())
            } else {
                self.current_tick = self.current_tick.saturating_add(1);
                #[cfg(feature = "defmt")] defmt::warn!("Other Error");
                // TODO, restart connection process? Just disregard?
                Err(Error::UnexpectedMessage)
            }
        } else {
            // println!("??? {:?}?", msg);
            #[cfg(feature = "defmt")] defmt::info!("Not a control message while waiting for PR");
            self.current_tick = self.current_tick.saturating_add(1);
            Ok(())
        }
    }

    /// Process messages while in a `ClientState::Registered` state
    fn registered<C: ClientIo<N, SZ>>(&mut self, cio: &mut C) -> Result<(), Error> {
        if self.sub_paths.is_empty() {
            #[cfg(feature = "defmt")] defmt::info!("No subscriptions");
            self.state = ClientState::Subscribed;
            self.current_tick = 0;
        } else {
            #[cfg(feature = "defmt")] defmt::info!("Start subscribing");
            let msg = Component::PubSub(PubSub {
                path: PubSubPath::Long(Path::Borrowed(self.sub_paths[0])),
                ty: PubSubType::Sub,
            });

            cio.send(msg)?;

            self.state = ClientState::Subscribing;
            self.current_idx = 0;
            self.current_tick = 0;
        }

        Ok(())
    }

    /// Process messages while in a `ClientState::Subscribing` state
    fn subscribing<C: ClientIo<N, SZ>>(&mut self, cio: &mut C) -> Result<(), Error> {
        let msg = cio.recv()?;
        let msg = match msg {
            Some(msg) => msg,
            None => {
                self.current_tick = self.current_tick.saturating_add(1);
                return Ok(());
            }
        };

        if let Arbitrator::PubSub(Ok(PubSubResponse::SubAck {
            path: PubSubPath::Long(pth),
        })) = msg.msg
        {
            if pth.deref() == self.sub_paths[self.current_idx] {
                self.current_idx += 1;
                if self.current_idx >= self.sub_paths.len() {
                    self.state = ClientState::Subscribed;
                    self.current_tick = 0;
                } else {
                    let msg = Component::PubSub(PubSub {
                        path: PubSubPath::Long(Path::Borrowed(
                            self.sub_paths[self.current_idx],
                        )),
                        ty: PubSubType::Sub,
                    });

                    cio.send(msg)?;

                    self.state = ClientState::Subscribing;
                    self.current_tick = 0;
                }
            } else {
                self.current_tick = self.current_tick.saturating_add(1);
            }
        } else {
            self.current_tick = self.current_tick.saturating_add(1);
        }

        Ok(())
    }

    /// Process messages while in a `ClientState::Subscribed` state
    fn subscribed<C: ClientIo<N, SZ>>(&mut self, cio: &mut C) -> Result<(), Error> {
        match (self.sub_paths.len(), self.pub_short_paths.len()) {
            (0, 0) => {
                self.state = ClientState::Active;
                self.current_tick = 0;
            }
            (0, _n) => {
                self.ctr = self.ctr.wrapping_add(1);
                let msg = Component::Control(CControl {
                    seq: self.ctr,
                    ty: ControlType::RegisterPubSubShortId(PubSubShort {
                        long_name: ManagedArcStr::Borrowed(self.pub_short_paths[0]),
                        short_id: PUBLISH_SHORTCODE_OFFSET,
                    }),
                });

                cio.send(msg)?;

                self.state = ClientState::ShortCodingPub;
                self.current_tick = 0;
                self.current_idx = 0;
            }
            (_n, _) => {
                // TODO: This doesn't handle the case when the subscribe shortcode is
                // a wildcard, which the broker will reject
                self.ctr = self.ctr.wrapping_add(1);
                let msg = Component::Control(CControl {
                    seq: self.ctr,
                    ty: ControlType::RegisterPubSubShortId(PubSubShort {
                        long_name: ManagedArcStr::Borrowed(self.sub_paths[0]),
                        short_id: 0x0000,
                    }),
                });

                cio.send(msg)?;

                self.state = ClientState::ShortCodingSub;
                self.current_tick = 0;
                self.current_idx = 0;
            }
        }
        Ok(())
    }

    /// Process messages while in a `ClientState::ShortcodingSub` state
    fn shortcoding_sub<C: ClientIo<N, SZ>>(&mut self, cio: &mut C) -> Result<(), Error> {
        let msg = cio.recv()?;
        let msg = match msg {
            Some(msg) => msg,
            None => {
                self.current_tick = self.current_tick.saturating_add(1);
                return Ok(());
            }
        };

        if let Arbitrator::Control(AControl {
            seq,
            response: Ok(ControlResponse::PubSubShortRegistration(sid)),
        }) = msg.msg
        {
            if seq == self.ctr && sid == (self.current_idx as u16) {
                self.current_idx += 1;

                if self.current_idx >= self.sub_paths.len() {
                    if self.pub_short_paths.is_empty() {
                        self.state = ClientState::Active;
                        self.current_tick = 0;
                    } else {
                        self.ctr = self.ctr.wrapping_add(1);

                        let msg = Component::Control(CControl {
                            seq: self.ctr,
                            ty: ControlType::RegisterPubSubShortId(PubSubShort {
                                long_name: ManagedArcStr::Borrowed(self.pub_short_paths[0]),
                                short_id: PUBLISH_SHORTCODE_OFFSET,
                            }),
                        });

                        cio.send(msg)?;

                        self.current_tick = 0;
                        self.current_idx = 0;
                        self.state = ClientState::ShortCodingPub;
                    }
                } else {
                    self.ctr = self.ctr.wrapping_add(1);

                    // TODO: This doesn't handle subscriptions with wildcards
                    let msg = Component::Control(CControl {
                        seq: self.ctr,
                        ty: ControlType::RegisterPubSubShortId(PubSubShort {
                            long_name: ManagedArcStr::Borrowed(self.sub_paths[self.current_idx]),
                            short_id: self.current_idx as u16,
                        }),
                    });

                    cio.send(msg)?;

                    self.current_tick = 0;
                }
            } else {
                self.current_tick = self.current_tick.saturating_add(1);
            }
        } else {
            self.current_tick = self.current_tick.saturating_add(1);
        }

        Ok(())
    }

    /// Process messages while in a `ClientState::ShortcodingPub` state
    fn shortcoding_pub<C: ClientIo<N, SZ>>(&mut self, cio: &mut C) -> Result<(), Error> {
        let msg = cio.recv()?;
        let msg = match msg {
            Some(msg) => msg,
            None => {
                self.current_tick = self.current_tick.saturating_add(1);
                return Ok(());
            }
        };

        if let Arbitrator::Control(AControl {
            seq,
            response: Ok(ControlResponse::PubSubShortRegistration(sid)),
        }) = msg.msg
        {
            if seq == self.ctr && sid == ((self.current_idx as u16) | PUBLISH_SHORTCODE_OFFSET) {
                self.current_idx += 1;

                if self.current_idx >= self.pub_short_paths.len() {
                    self.state = ClientState::Active;
                    self.current_tick = 0;
                } else {
                    self.ctr = self.ctr.wrapping_add(1);

                    let msg = Component::Control(CControl {
                        seq: self.ctr,
                        ty: ControlType::RegisterPubSubShortId(PubSubShort {
                            long_name: ManagedArcStr::Borrowed(self.pub_short_paths[self.current_idx]),
                            short_id: ((self.current_idx as u16) | PUBLISH_SHORTCODE_OFFSET),
                        }),
                    });

                    cio.send(msg)?;

                    self.current_tick = 0;
                }
            } else {
                self.current_tick = self.current_tick.saturating_add(1);
            }
        } else {
            self.current_tick = self.current_tick.saturating_add(1);
        }

        Ok(())
    }

    /// Process messages while in a Connected state
    fn active<C: ClientIo<N, SZ>, T: Table<N, SZ>>(&mut self, cio: &mut C) -> Result<Option<RecvMsg<T, N, SZ>>, Error> {
        let msg = cio.recv()?;
        let (pubsub, arc) = match msg {
            Some(RecvPayload { msg: Arbitrator::PubSub(Ok(PubSubResponse::SubMsg(ps))), arc } ) => (ps, arc),
            Some(_) => {
                // TODO: Maybe something else? return err?
                return Ok(None);
            }
            None => {
                return Ok(None);
            }
        };

        // Determine the path
        let path = match &pubsub.path {
            PubSubPath::Short(sid) => {
                Path::Borrowed(
                    *self
                        .sub_paths
                        .get(*sid as usize)
                        .ok_or(Error::UnexpectedMessage)?,
                )
            },
            PubSubPath::Long(ms) => {
                ms.clone()
            },
        };

        let path_match = T::sub_paths().iter().find(|sp| matches(sp, &path));

        if let Some(pm) = path_match {
            if let Ok(payload) = T::from_sub_msg(
                pm,
                path.clone(),
                pubsub,
                &arc
            ) {
                return Ok(Some(RecvMsg {
                    path,
                    payload,
                    arc,
                }));
            }
        }

        Err(Error::UnexpectedMessage)
    }
}
