#![warn(
    unused_extern_crates,
    missing_debug_implementations,
    missing_copy_implementations,
    rust_2018_idioms,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![forbid(unsafe_code)]

use anyhow::{bail, Result};
use cli::Options;
use futures::{channel::mpsc, StreamExt};
use libp2p::Multiaddr;
use log::LevelFilter;
use std::{io, io::Write, process, sync::Arc};
use structopt::StructOpt;
use tracing::info;
use url::Url;

mod cli;
mod trace;

use swap::{alice, bitcoin, bob, monero, Cmd, Rsp, SwapAmounts};

// TODO: Add root seed file instead of generating new seed each run.
// TODO: Remove all instances of the todo! macro

// TODO: Add a config file with these in it.
// Alice's address and port until we have a config file.
pub const PORT: u16 = 9876; // Arbitrarily chosen.
pub const ADDR: &str = "127.0.0.1";
pub const BITCOIND_JSON_RPC_URL: &str = "127.0.0.1:8332";
pub const MONERO_WALLET_RPC_PORT: u16 = 18083;

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Options::from_args();

    trace::init_tracing(LevelFilter::Debug)?;

    let addr = format!("/ip4/{}/tcp/{}", ADDR, PORT);
    let alice: Multiaddr = addr.parse().expect("failed to parse Alice's address");

    if opt.as_alice {
        info!("running swap node as Alice ...");

        if opt.piconeros.is_some() || opt.satoshis.is_some() {
            bail!("Alice cannot set the amount to swap via the cli");
        }

        let url = Url::parse(BITCOIND_JSON_RPC_URL).expect("failed to parse url");
        let bitcoin_wallet = bitcoin::Wallet::new("alice", &url)
            .await
            .expect("failed to create bitcoin wallet");

        let monero_wallet = Arc::new(monero::Wallet::localhost(MONERO_WALLET_RPC_PORT));

        let redeem = bitcoin_wallet
            .new_address()
            .await
            .expect("failed to get new redeem address");
        let punish = bitcoin_wallet
            .new_address()
            .await
            .expect("failed to get new punish address");

        let bitcoin_wallet = Arc::new(bitcoin_wallet);
        swap_as_alice(bitcoin_wallet, monero_wallet, alice.clone(), redeem, punish).await?;
    } else {
        info!("running swap node as Bob ...");

        let url = Url::parse(BITCOIND_JSON_RPC_URL).expect("failed to parse url");
        let bitcoin_wallet = bitcoin::Wallet::new("bob", &url)
            .await
            .expect("failed to create bitcoin wallet");

        let monero_wallet = Arc::new(monero::Wallet::localhost(MONERO_WALLET_RPC_PORT));

        let refund = bitcoin_wallet
            .new_address()
            .await
            .expect("failed to get new address");

        let bitcoin_wallet = Arc::new(bitcoin_wallet);
        match (opt.piconeros, opt.satoshis) {
            (Some(_), Some(_)) => bail!("Please supply only a single amount to swap"),
            (None, None) => bail!("Please supply an amount to swap"),
            (Some(_picos), _) => todo!("support starting with picos"),
            (None, Some(sats)) => {
                swap_as_bob(bitcoin_wallet, monero_wallet, sats, alice, refund).await?;
            }
        };
    }

    Ok(())
}

async fn swap_as_alice(
    bitcoin_wallet: Arc<swap::bitcoin::Wallet>,
    monero_wallet: Arc<swap::monero::Wallet>,
    addr: Multiaddr,
    redeem: ::bitcoin::Address,
    punish: ::bitcoin::Address,
) -> Result<()> {
    alice::swap(bitcoin_wallet, monero_wallet, addr, redeem, punish).await
}

async fn swap_as_bob(
    bitcoin_wallet: Arc<swap::bitcoin::Wallet>,
    monero_wallet: Arc<swap::monero::Wallet>,
    sats: u64,
    alice: Multiaddr,
    refund: ::bitcoin::Address,
) -> Result<()> {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(1);
    let (mut rsp_tx, rsp_rx) = mpsc::channel(1);
    tokio::spawn(bob::swap(
        bitcoin_wallet,
        monero_wallet,
        sats,
        alice,
        cmd_tx,
        rsp_rx,
        refund,
    ));

    loop {
        let read = cmd_rx.next().await;
        match read {
            Some(cmd) => match cmd {
                Cmd::VerifyAmounts(p) => {
                    let rsp = verify(p);
                    rsp_tx.try_send(rsp)?;
                    if rsp == Rsp::Abort {
                        process::exit(0);
                    }
                }
            },
            None => {
                info!("Channel closed from other end");
                return Ok(());
            }
        }
    }
}

fn verify(amounts: SwapAmounts) -> Rsp {
    let mut s = String::new();
    println!("Got rate from Alice for XMR/BTC swap\n");
    println!("{}", amounts);
    print!("Would you like to continue with this swap [y/N]: ");

    let _ = io::stdout().flush();
    io::stdin()
        .read_line(&mut s)
        .expect("Did not enter a correct string");

    if let Some('\n') = s.chars().next_back() {
        s.pop();
    }
    if let Some('\r') = s.chars().next_back() {
        s.pop();
    }

    if !is_yes(&s) {
        println!("No worries, try again later - Alice updates her rate regularly");
        return Rsp::Abort;
    }

    Rsp::VerifiedAmounts
}

fn is_yes(s: &str) -> bool {
    matches!(s, "y" | "Y" | "yes" | "YES" | "Yes")
}
