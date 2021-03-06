use anyhow::{bail, Result};
use libp2p::{
    request_response::{
        handler::RequestProtocol, ProtocolSupport, RequestResponse, RequestResponseConfig,
        RequestResponseEvent, RequestResponseMessage,
    },
    swarm::{NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters},
    NetworkBehaviour,
};
use rand::rngs::OsRng;
use std::{
    collections::VecDeque,
    task::{Context, Poll},
    time::Duration,
};
use tracing::{debug, error};

use crate::network::request_response::{AliceToBob, BobToAlice, Codec, Message0Protocol, TIMEOUT};
use xmr_btc::{alice::State0, bob};

#[derive(Debug)]
pub enum OutEvent {
    Msg(bob::Message0),
}

/// A `NetworkBehaviour` that represents send/recv of message 0.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "OutEvent", poll_method = "poll")]
#[allow(missing_debug_implementations)]
pub struct Message0 {
    rr: RequestResponse<Codec<Message0Protocol>>,
    #[behaviour(ignore)]
    events: VecDeque<OutEvent>,
    #[behaviour(ignore)]
    state: Option<State0>,
}

impl Message0 {
    pub fn set_state(&mut self, state: State0) -> Result<()> {
        if self.state.is_some() {
            bail!("Trying to set state a second time");
        }
        self.state = Some(state);

        Ok(())
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<RequestProtocol<Codec<Message0Protocol>>, OutEvent>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }

        Poll::Pending
    }
}

impl Default for Message0 {
    fn default() -> Self {
        let timeout = Duration::from_secs(TIMEOUT);
        let mut config = RequestResponseConfig::default();
        config.set_request_timeout(timeout);

        Self {
            rr: RequestResponse::new(
                Codec::default(),
                vec![(Message0Protocol, ProtocolSupport::Full)],
                config,
            ),
            events: Default::default(),
            state: None,
        }
    }
}

impl NetworkBehaviourEventProcess<RequestResponseEvent<BobToAlice, AliceToBob>> for Message0 {
    fn inject_event(&mut self, event: RequestResponseEvent<BobToAlice, AliceToBob>) {
        match event {
            RequestResponseEvent::Message {
                message:
                    RequestResponseMessage::Request {
                        request, channel, ..
                    },
                ..
            } => {
                if let BobToAlice::Message0(msg) = request {
                    debug!("Received Message0");
                    let response = match &self.state {
                        None => panic!("No state, did you forget to set it?"),
                        Some(state) => {
                            // TODO: Get OsRng from somewhere?
                            AliceToBob::Message0(state.next_message(&mut OsRng))
                        }
                    };
                    self.rr.send_response(channel, response);
                    debug!("Sent Message0");

                    self.events.push_back(OutEvent::Msg(msg));
                }
            }
            RequestResponseEvent::Message {
                message: RequestResponseMessage::Response { .. },
                ..
            } => panic!("Alice should not get a Response"),
            RequestResponseEvent::InboundFailure { error, .. } => {
                error!("Inbound failure: {:?}", error);
            }
            RequestResponseEvent::OutboundFailure { error, .. } => {
                error!("Outbound failure: {:?}", error);
            }
        }
    }
}
