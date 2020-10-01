pub mod node;

use crate::{
    alice,
    bitcoin::{self, BuildTxLockPsbt, GetRawTransaction, TxCancel},
    monero,
};
use anyhow::{anyhow, Result};
use ecdsa_fun::{
    adaptor::{Adaptor, EncryptedSignature},
    nonce::Deterministic,
    Signature,
};
use rand::{CryptoRng, RngCore};
use sha2::Sha256;
use std::convert::TryFrom;

#[derive(Debug)]
pub enum State {
    State0(State0),
    State1(State1),
    State2(State2),
    State2b(State2b),
    State3(State3),
    State4(State4),
}

macro_rules! impl_try_from_parent_state {
    ($type:ident) => {
        impl TryFrom<State> for $type {
            type Error = anyhow::Error;
            fn try_from(from: State) -> Result<Self> {
                if let State::$type(state) = from {
                    Ok(state)
                } else {
                    Err(anyhow!("Failed to convert parent state to child state"))
                }
            }
        }
    };
}

impl_try_from_parent_state!(State0);
impl_try_from_parent_state!(State1);
impl_try_from_parent_state!(State2);
impl_try_from_parent_state!(State2b);
impl_try_from_parent_state!(State3);
impl_try_from_parent_state!(State4);

macro_rules! impl_from_child_state {
    ($type:ident) => {
        impl From<$type> for State {
            fn from(from: $type) -> Self {
                State::$type(from)
            }
        }
    };
}

impl_from_child_state!(State0);
impl_from_child_state!(State1);
impl_from_child_state!(State2);
impl_from_child_state!(State2b);
impl_from_child_state!(State3);
impl_from_child_state!(State4);

// todo: use macro
pub fn is_state4(state: &State) -> bool {
    match state {
        State::State4 { .. } => true,
        _ => false,
    }
}

pub fn is_state2b(state: &State) -> bool {
    match state {
        State::State2b { .. } => true,
        _ => false,
    }
}

#[derive(Debug)]
pub enum Message {
    Message0(Message0),
    Message1(Message1),
    Message2(Message2),
    Message3(Message3),
}

impl From<Message0> for Message {
    fn from(m: Message0) -> Self {
        Message::Message0(m)
    }
}

impl TryFrom<Message> for Message0 {
    type Error = UnexpectedMessage;

    fn try_from(m: Message) -> Result<Self, Self::Error> {
        match m {
            Message::Message0(m) => Ok(m),
            _ => Err(UnexpectedMessage {
                expected_type: "Create0".to_string(),
                received: m,
            }),
        }
    }
}

impl From<Message1> for Message {
    fn from(m: Message1) -> Self {
        Message::Message1(m)
    }
}

impl TryFrom<Message> for Message1 {
    type Error = UnexpectedMessage;

    fn try_from(m: Message) -> Result<Self, Self::Error> {
        match m {
            Message::Message1(m) => Ok(m),
            _ => Err(UnexpectedMessage {
                expected_type: "Create0".to_string(),
                received: m,
            }),
        }
    }
}

impl From<Message2> for Message {
    fn from(m: Message2) -> Self {
        Message::Message2(m)
    }
}

impl TryFrom<Message> for Message2 {
    type Error = UnexpectedMessage;

    fn try_from(m: Message) -> Result<Self, Self::Error> {
        match m {
            Message::Message2(m) => Ok(m),
            _ => Err(UnexpectedMessage {
                expected_type: "Create0".to_string(),
                received: m,
            }),
        }
    }
}

impl From<Message3> for Message {
    fn from(m: Message3) -> Self {
        Message::Message3(m)
    }
}

impl TryFrom<Message> for Message3 {
    type Error = UnexpectedMessage;

    fn try_from(m: Message) -> Result<Self, Self::Error> {
        match m {
            Message::Message3(m) => Ok(m),
            _ => Err(UnexpectedMessage {
                expected_type: "Create0".to_string(),
                received: m,
            }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("expected message of type {expected_type}, got {received:?}")]
pub struct UnexpectedMessage {
    expected_type: String,
    received: Message,
}

impl UnexpectedMessage {
    pub fn new<T>(received: Message) -> Self {
        let expected_type = std::any::type_name::<T>();

        Self {
            expected_type: expected_type.to_string(),
            received,
        }
    }
}

#[derive(Debug)]
pub struct Message0 {
    pub(crate) B: bitcoin::PublicKey,
    pub(crate) S_b_monero: monero::PublicKey,
    pub(crate) S_b_bitcoin: bitcoin::PublicKey,
    pub(crate) dleq_proof_s_b: cross_curve_dleq::Proof,
    pub(crate) v_b: monero::PrivateViewKey,
    pub(crate) refund_address: bitcoin::Address,
}

#[derive(Debug)]
pub struct Message1 {
    pub(crate) tx_lock: bitcoin::TxLock,
}

#[derive(Debug)]
pub struct Message2 {
    pub(crate) tx_punish_sig: Signature,
    pub(crate) tx_cancel_sig: Signature,
}

#[derive(Debug)]
pub struct Message3 {
    pub(crate) tx_redeem_encsig: EncryptedSignature,
}

#[derive(Debug)]
pub struct State0 {
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    v_b: monero::PrivateViewKey,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
}

impl State0 {
    pub fn new<R: RngCore + CryptoRng>(
        rng: &mut R,
        btc: bitcoin::Amount,
        xmr: monero::Amount,
        refund_timelock: u32,
        punish_timelock: u32,
        refund_address: bitcoin::Address,
    ) -> Self {
        let b = bitcoin::SecretKey::new_random(rng);

        let s_b = cross_curve_dleq::Scalar::random(rng);
        let v_b = monero::PrivateViewKey::new_random(rng);

        Self {
            b,
            s_b,
            v_b,
            btc,
            xmr,
            refund_timelock,
            punish_timelock,
            refund_address,
        }
    }

    pub fn next_message<R: RngCore + CryptoRng>(&self, rng: &mut R) -> Message0 {
        let dleq_proof_s_b = cross_curve_dleq::Proof::new(rng, &self.s_b);

        Message0 {
            B: self.b.public(),
            S_b_monero: monero::PublicKey::from_private_key(&monero::PrivateKey {
                scalar: self.s_b.into_ed25519(),
            }),
            S_b_bitcoin: self.s_b.into_secp256k1().into(),
            dleq_proof_s_b,
            v_b: self.v_b,
            refund_address: self.refund_address.clone(),
        }
    }

    pub async fn receive<W>(self, wallet: &W, msg: alice::Message0) -> anyhow::Result<State1>
    where
        W: BuildTxLockPsbt,
    {
        msg.dleq_proof_s_a.verify(
            &msg.S_a_bitcoin.clone().into(),
            msg.S_a_monero
                .point
                .decompress()
                .ok_or_else(|| anyhow!("S_a is not a monero curve point"))?,
        )?;

        let tx_lock =
            bitcoin::TxLock::new(wallet, self.btc, msg.A.clone(), self.b.public()).await?;
        let v = msg.v_a + self.v_b;

        Ok(State1 {
            A: msg.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: msg.S_a_monero,
            S_a_bitcoin: msg.S_a_bitcoin,
            v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: msg.redeem_address,
            punish_address: msg.punish_address,
            tx_lock,
        })
    }
}

#[derive(Debug, Clone)]
pub struct State1 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
}

impl State1 {
    pub fn next_message(&self) -> Message1 {
        Message1 {
            tx_lock: self.tx_lock.clone(),
        }
    }

    pub fn receive(self, msg: alice::Message1) -> Result<State2> {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.A.clone(),
            self.b.public(),
        );
        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        bitcoin::verify_sig(&self.A, &tx_cancel.digest(), &msg.tx_cancel_sig)?;
        bitcoin::verify_encsig(
            self.A.clone(),
            self.s_b.into_secp256k1().into(),
            &tx_refund.digest(),
            &msg.tx_refund_encsig,
        )?;

        Ok(State2 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: msg.tx_cancel_sig,
            tx_refund_encsig: msg.tx_refund_encsig,
        })
    }
}

#[derive(Debug, Clone)]
pub struct State2 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: EncryptedSignature,
}

impl State2 {
    pub fn next_message(&self) -> Message2 {
        let tx_cancel = TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.A.clone(),
            self.b.public(),
        );
        let tx_cancel_sig = self.b.sign(tx_cancel.digest());
        let tx_punish =
            bitcoin::TxPunish::new(&tx_cancel, &self.punish_address, self.punish_timelock);
        let tx_punish_sig = self.b.sign(tx_punish.digest());

        Message2 {
            tx_punish_sig,
            tx_cancel_sig,
        }
    }

    pub async fn lock_btc<W>(self, bitcoin_wallet: &W) -> Result<State2b>
    where
        W: bitcoin::SignTxLock + bitcoin::BroadcastSignedTransaction,
    {
        let signed_tx_lock = bitcoin_wallet.sign_tx_lock(self.tx_lock.clone()).await?;

        tracing::info!("{}", self.tx_lock.txid());
        let _ = bitcoin_wallet
            .broadcast_signed_transaction(signed_tx_lock)
            .await?;

        Ok(State2b {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            tx_refund_encsig: self.tx_refund_encsig,
        })
    }
}

#[derive(Debug, Clone)]
pub struct State2b {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: EncryptedSignature,
}

impl State2b {
    // todo: loop until punish? timelock has expired
    pub async fn watch_for_lock_xmr<W>(self, xmr_wallet: &W, msg: alice::Message2) -> Result<State3>
    where
        W: monero::CheckTransfer,
    {
        let S_b_monero = monero::PublicKey::from_private_key(&monero::PrivateKey::from_scalar(
            self.s_b.into_ed25519(),
        ));
        let S = self.S_a_monero + S_b_monero;

        xmr_wallet
            .check_transfer(S, self.v.public(), msg.tx_lock_proof, self.xmr)
            .await?;

        Ok(State3 {
            A: self.A,
            b: self.b,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_cancel_sig_a: self.tx_cancel_sig_a,
            tx_refund_encsig: self.tx_refund_encsig,
        })
    }

    pub async fn refund_btc<W: bitcoin::BroadcastSignedTransaction>(
        &self,
        bitcoin_wallet: &W,
    ) -> Result<()> {
        let tx_cancel = bitcoin::TxCancel::new(
            &self.tx_lock,
            self.refund_timelock,
            self.A.clone(),
            self.b.public(),
        );
        let tx_refund = bitcoin::TxRefund::new(&tx_cancel, &self.refund_address);

        {
            let sig_b = self.b.sign(tx_cancel.digest());
            let sig_a = self.tx_cancel_sig_a.clone();

            let signed_tx_cancel = tx_cancel.clone().add_signatures(
                &self.tx_lock,
                (self.A.clone(), sig_a),
                (self.b.public(), sig_b),
            )?;

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_cancel)
                .await?;
        }

        {
            let adaptor = Adaptor::<Sha256, Deterministic<Sha256>>::default();

            let sig_b = self.b.sign(tx_refund.digest());
            let sig_a = adaptor
                .decrypt_signature(&self.s_b.into_secp256k1(), self.tx_refund_encsig.clone());

            let signed_tx_refund = tx_refund.add_signatures(
                &tx_cancel.clone(),
                (self.A.clone(), sig_a),
                (self.b.public(), sig_b),
            )?;

            let _ = bitcoin_wallet
                .broadcast_signed_transaction(signed_tx_refund)
                .await?;
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }
}

#[derive(Debug, Clone)]
pub struct State3 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    tx_cancel_sig_a: Signature,
    tx_refund_encsig: EncryptedSignature,
}

impl State3 {
    pub fn next_message(&self) -> Message3 {
        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin.clone(), tx_redeem.digest());

        Message3 { tx_redeem_encsig }
    }

    pub async fn watch_for_redeem_btc<W>(self, bitcoin_wallet: &W) -> Result<State4>
    where
        W: GetRawTransaction,
    {
        let tx_redeem = bitcoin::TxRedeem::new(&self.tx_lock, &self.redeem_address);
        let tx_redeem_encsig = self.b.encsign(self.S_a_bitcoin.clone(), tx_redeem.digest());

        let tx_redeem_candidate = bitcoin_wallet.get_raw_transaction(tx_redeem.txid()).await?;

        let tx_redeem_sig =
            tx_redeem.extract_signature_by_key(tx_redeem_candidate, self.b.public())?;
        let s_a = bitcoin::recover(self.S_a_bitcoin.clone(), tx_redeem_sig, tx_redeem_encsig)?;
        let s_a =
            monero::PrivateKey::from_scalar(monero::Scalar::from_bytes_mod_order(s_a.to_bytes()));

        Ok(State4 {
            A: self.A,
            b: self.b,
            s_a,
            s_b: self.s_b,
            S_a_monero: self.S_a_monero,
            S_a_bitcoin: self.S_a_bitcoin,
            v: self.v,
            btc: self.btc,
            xmr: self.xmr,
            refund_timelock: self.refund_timelock,
            punish_timelock: self.punish_timelock,
            refund_address: self.refund_address,
            redeem_address: self.redeem_address,
            punish_address: self.punish_address,
            tx_lock: self.tx_lock,
            tx_refund_encsig: self.tx_refund_encsig,
            tx_cancel_sig: self.tx_cancel_sig_a,
        })
    }
}

#[derive(Debug, Clone)]
pub struct State4 {
    A: bitcoin::PublicKey,
    b: bitcoin::SecretKey,
    s_a: monero::PrivateKey,
    s_b: cross_curve_dleq::Scalar,
    S_a_monero: monero::PublicKey,
    S_a_bitcoin: bitcoin::PublicKey,
    v: monero::PrivateViewKey,
    btc: bitcoin::Amount,
    xmr: monero::Amount,
    refund_timelock: u32,
    punish_timelock: u32,
    refund_address: bitcoin::Address,
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,
    tx_lock: bitcoin::TxLock,
    tx_refund_encsig: EncryptedSignature,
    tx_cancel_sig: Signature,
}

impl State4 {
    pub async fn claim_xmr<W>(&self, monero_wallet: &W) -> Result<()>
    where
        W: monero::ImportOutput,
    {
        let s_b = monero::PrivateKey {
            scalar: self.s_b.into_ed25519(),
        };

        let s = self.s_a + s_b;

        // NOTE: This actually generates and opens a new wallet, closing the currently
        // open one.
        monero_wallet.import_output(s, self.v).await?;

        Ok(())
    }
    #[cfg(test)]
    pub fn tx_lock_id(&self) -> bitcoin::Txid {
        self.tx_lock.txid()
    }
}
