use crate::{
    monero::CreateWalletForOutput,
    state::{Alice, Bob, Swap},
};
use anyhow::Result;
use futures::{
    future::{select, Either},
    pin_mut,
};
use xmr_btc::bitcoin::{
    poll_until_block_height_is_gte, BroadcastSignedTransaction, TransactionBlockHeight, TxCancel,
    TxPunish, TxRefund, WatchForRawTransaction,
};

pub async fn recover(
    bitcoin_wallet: crate::bitcoin::Wallet,
    monero_wallet: crate::monero::Wallet,
    state: Swap,
) -> Result<()> {
    match state {
        Swap::Alice(state) => alice_recover(bitcoin_wallet, monero_wallet, state).await,
        Swap::Bob(state) => bob_recover(bitcoin_wallet, monero_wallet, state).await,
    }
}

pub async fn alice_recover(
    bitcoin_wallet: crate::bitcoin::Wallet,
    monero_wallet: crate::monero::Wallet,
    state: Alice,
) -> Result<()> {
    match state {
        Alice::Handshaken(_) | Alice::BtcLocked(_) | Alice::SwapComplete => Ok(()),
        Alice::XmrLocked(state) => {
            let tx_cancel = TxCancel::new(
                &state.tx_lock,
                state.refund_timelock,
                state.a.public(),
                state.B.clone(),
            );

            // Ensure that TxCancel is on the blockchain
            if bitcoin_wallet
                .0
                .get_raw_transaction(tx_cancel.txid())
                .await
                .is_err()
            {
                let tx_lock_height = bitcoin_wallet
                    .transaction_block_height(state.tx_lock.txid())
                    .await;
                poll_until_block_height_is_gte(
                    &bitcoin_wallet,
                    tx_lock_height + state.refund_timelock,
                )
                .await;

                let sig_a = state.a.sign(tx_cancel.digest());
                let sig_b = state.tx_cancel_sig_bob.clone();

                let tx_cancel = tx_cancel
                    .clone()
                    .add_signatures(
                        &state.tx_lock,
                        (state.a.public(), sig_a),
                        (state.B.clone(), sig_b),
                    )
                    .expect("sig_{a,b} to be valid signatures for tx_cancel");

                // TODO: We should not fail if the transaction is already on the blockchain
                bitcoin_wallet
                    .broadcast_signed_transaction(tx_cancel)
                    .await?;
            }

            let tx_cancel_height = bitcoin_wallet
                .transaction_block_height(tx_cancel.txid())
                .await;
            let poll_until_bob_can_be_punished = poll_until_block_height_is_gte(
                &bitcoin_wallet,
                tx_cancel_height + state.punish_timelock,
            );
            pin_mut!(poll_until_bob_can_be_punished);

            let tx_refund = TxRefund::new(&tx_cancel, &state.refund_address);

            match select(
                bitcoin_wallet.watch_for_raw_transaction(tx_refund.txid()),
                poll_until_bob_can_be_punished,
            )
            .await
            {
                Either::Left((tx_refund_published, ..)) => {
                    let tx_refund_sig = tx_refund
                        .extract_signature_by_key(tx_refund_published, state.a.public())?;
                    let tx_refund_encsig = state
                        .a
                        .encsign(state.S_b_bitcoin.clone(), tx_refund.digest());

                    let s_b = xmr_btc::bitcoin::recover(
                        state.S_b_bitcoin,
                        tx_refund_sig,
                        tx_refund_encsig,
                    )?;
                    let s_b = monero::PrivateKey::from_scalar(
                        xmr_btc::monero::Scalar::from_bytes_mod_order(s_b.to_bytes()),
                    );

                    let s_a = monero::PrivateKey {
                        scalar: state.s_a.into_ed25519(),
                    };

                    monero_wallet
                        .create_and_load_wallet_for_output(s_a + s_b, state.v)
                        .await?;
                }
                Either::Right(_) => {
                    let tx_punish =
                        TxPunish::new(&tx_cancel, &state.punish_address, state.punish_timelock);

                    let sig_a = state.a.sign(tx_punish.digest());
                    let sig_b = state.tx_cancel_sig_bob.clone();

                    let sig_tx_punish = tx_punish.add_signatures(
                        &tx_cancel,
                        (state.a.public(), sig_a),
                        (state.B.clone(), sig_b),
                    )?;

                    bitcoin_wallet
                        .broadcast_signed_transaction(sig_tx_punish)
                        .await?;
                }
            };

            Ok(())
        }
        Alice::BtcRedeemable { redeem_tx, .. } => {
            bitcoin_wallet
                .broadcast_signed_transaction(redeem_tx)
                .await?;
            Ok(())
        }
        Alice::BtcPunishable(state) => {
            let tx_cancel = TxCancel::new(
                &state.tx_lock,
                state.refund_timelock,
                state.a.public(),
                state.B.clone(),
            );

            let tx_punish = TxPunish::new(&tx_cancel, &state.punish_address, state.punish_timelock);

            let sig_a = state.a.sign(tx_punish.digest());
            let sig_b = state.tx_cancel_sig_bob.clone();

            let sig_tx_punish = tx_punish.add_signatures(
                &tx_cancel,
                (state.a.public(), sig_a),
                (state.B.clone(), sig_b),
            )?;

            bitcoin_wallet
                .broadcast_signed_transaction(sig_tx_punish)
                .await?;

            Ok(())
        }
        Alice::BtcRefunded {
            view_key,
            spend_key,
            ..
        } => {
            monero_wallet
                .create_and_load_wallet_for_output(spend_key, view_key)
                .await?;
            Ok(())
        }
    }
}

pub async fn bob_recover(
    _bitcoin_wallet: crate::bitcoin::Wallet,
    _monero_wallet: crate::monero::Wallet,
    _state: Bob,
) -> Result<()> {
    todo!()
}