// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::cli_state::CliState;
use crate::dev::sign_txn_helper::sign_txn_with_account_by_rpc_client;
use crate::StarcoinOpt;
use anyhow::Result;
use scmd::{CommandAction, ExecContext};
use starcoin_crypto::hash::HashValue;
use starcoin_transaction_builder::build_module_upgrade_queue_v2;
use starcoin_vm_types::account_address::AccountAddress;
use starcoin_vm_types::transaction::TransactionPayload;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "module_queue_v2")]
pub struct UpgradeModuleQueueOpt {
    #[structopt(short = "s", long)]
    /// hex encoded string, like 0x1, 0x12
    sender: Option<AccountAddress>,

    #[structopt(
        short = "g",
        name = "max-gas-amount",
        default_value = "10000000",
        help = "max gas used to deploy the module"
    )]
    max_gas_amount: u64,
    #[structopt(
        short = "p",
        long = "gas-price",
        name = "price of gas",
        default_value = "1",
        help = "gas price used to deploy the module"
    )]
    gas_price: u64,

    #[structopt(
        name = "expiration_time",
        long = "timeout",
        default_value = "3000",
        help = "how long(in seconds) the txn stay alive"
    )]
    expiration_time: u64,
    #[structopt(
        short = "b",
        name = "blocking-mode",
        long = "blocking",
        help = "blocking wait txn mined"
    )]
    blocking: bool,

    #[structopt(short = "a", name = "proposer-address", long = "proposer_address")]
    /// hex encoded string, like 0x1, 0x12
    proposer_address: Option<AccountAddress>,

    #[structopt(
        short = "m",
        name = "module-proposal-id",
        long = "module",
        help = "proposal id."
    )]
    proposal_id: u64,
}

pub struct UpgradeModuleQueueV2Command;

impl CommandAction for UpgradeModuleQueueV2Command {
    type State = CliState;
    type GlobalOpt = StarcoinOpt;
    type Opt = UpgradeModuleQueueOpt;
    type ReturnItem = HashValue;

    fn run(
        &self,
        ctx: &ExecContext<Self::State, Self::GlobalOpt, Self::Opt>,
    ) -> Result<Self::ReturnItem> {
        let opt = ctx.opt();
        let cli_state = ctx.state();
        let sender = if let Some(sender) = ctx.opt().sender {
            sender
        } else {
            ctx.state().default_account()?.address
        };
        let proposer_address = if let Some(address) = ctx.opt().proposer_address {
            address
        } else {
            sender
        };
        let module_upgrade_queue = build_module_upgrade_queue_v2(proposer_address, opt.proposal_id);
        let signed_txn = sign_txn_with_account_by_rpc_client(
            cli_state,
            sender,
            opt.max_gas_amount,
            opt.gas_price,
            opt.expiration_time,
            TransactionPayload::ScriptFunction(module_upgrade_queue),
        )?;
        let txn_hash = signed_txn.id();
        cli_state.client().submit_transaction(signed_txn)?;

        println!("txn {:#x} submitted.", txn_hash);

        if opt.blocking {
            ctx.state().watch_txn(txn_hash)?;
        }
        Ok(txn_hash)
    }
}
