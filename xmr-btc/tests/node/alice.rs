use crate::{
    transport::{SendReceive, Transport},
    wallet,
};
use anyhow::Result;
use rand::{CryptoRng, RngCore};
use std::convert::TryInto;
use xmr_btc::{alice, bob};

// TODO: merge this with bob node
// This struct is responsible for I/O
pub struct Node<'a> {
    transport: Transport<alice::Message, bob::Message>,
    pub bitcoin_wallet: wallet::bitcoin::Wallet,
    pub monero_wallet: wallet::monero::AliceWallet<'a>,
}

impl<'a> Node<'a> {
    pub fn new(
        transport: Transport<alice::Message, bob::Message>,
        bitcoin_wallet: wallet::bitcoin::Wallet,
        monero_wallet: wallet::monero::AliceWallet<'a>,
    ) -> Node<'a> {
        Self {
            transport,
            bitcoin_wallet,
            monero_wallet,
        }
    }
}

pub async fn run_alice_until<'a, R: RngCore + CryptoRng>(
    alice: &mut Node<'a>,
    initial_state: alice::State,
    is_state: fn(&alice::State) -> bool,
    rng: &mut R,
) -> Result<alice::State> {
    let mut result = initial_state;
    loop {
        result = next_state(alice, result, rng).await?;
        if is_state(&result) {
            return Ok(result);
        }
    }
}

// TODO: move this into the lib
async fn next_state<'a, R: RngCore + CryptoRng>(
    alice: &mut Node<'a>,
    state: alice::State,
    rng: &mut R,
) -> Result<alice::State> {
    match state {
        alice::State::State0(state0) => {
            alice
                .transport
                .sender
                .send(state0.next_message(rng).into())
                .await?;

            let bob_message0: bob::Message0 =
                alice.transport.receive_message().await?.try_into()?;
            let state1 = state0.receive(bob_message0)?;
            Ok(state1.into())
        }
        alice::State::State1(state1) => {
            let bob_message1: bob::Message1 =
                alice.transport.receive_message().await?.try_into()?;
            let state2 = state1.receive(bob_message1);
            let alice_message1: alice::Message1 = state2.next_message();
            alice.transport.sender.send(alice_message1.into()).await?;
            Ok(state2.into())
        }
        alice::State::State2(state2) => {
            let bob_message2: bob::Message2 =
                alice.transport.receive_message().await?.try_into()?;
            let state3 = state2.receive(bob_message2)?;
            tokio::time::delay_for(std::time::Duration::new(5, 0)).await;
            Ok(state3.into())
        }
        alice::State::State3(state3) => {
            tracing::info!("alice is watching for locked btc");
            let state4 = state3.watch_for_lock_btc(&alice.bitcoin_wallet).await?;
            Ok(state4.into())
        }
        alice::State::State4(state4) => {
            let state5 = state4.lock_xmr(&alice.monero_wallet).await?;
            tracing::info!("alice has locked xmr");
            Ok(state5.into())
        }
        alice::State::State5(state5) => {
            alice
                .transport
                .sender
                .send(state5.next_message().into())
                .await?;
            // todo: pass in state4b as a parameter somewhere in this call to prevent the
            // user from waiting for a message that wont be sent
            let message3: bob::Message3 = alice.transport.receive_message().await?.try_into()?;
            let state6 = state5.receive(message3);
            tracing::info!("alice has received bob message 3");
            tracing::info!("alice is redeeming btc");
            state6.redeem_btc(&alice.bitcoin_wallet).await.unwrap();
            Ok(state6.into())
        }
        alice::State::State6(state6) => Ok(state6.into()),
    }
}