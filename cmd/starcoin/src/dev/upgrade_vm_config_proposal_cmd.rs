// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::cli_state::CliState;
use crate::dev::sign_txn_helper::{get_dao_config, sign_txn_with_account_by_rpc_client};
use crate::StarcoinOpt;
use anyhow::{bail, Result};
use scmd::{CommandAction, ExecContext};
use starcoin_config::BuiltinNetworkID;
use starcoin_crypto::hash::HashValue;
use starcoin_transaction_builder::build_vm_config_upgrade_proposal;
use starcoin_vm_types::account_address::AccountAddress;
use starcoin_vm_types::transaction::TransactionPayload;
use std::str::FromStr;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "vm_config_proposal")]
#[allow(clippy::upper_case_acronyms)]
pub struct UpgradeVMConfigProposalOpt {
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

    #[structopt(
        short = "n",
        name = "net",
        long = "net",
        help = "chain net, for example, proxima"
    )]
    net: String,
}

#[allow(clippy::upper_case_acronyms)]
pub struct UpgradeVMConfigProposalCommand;

impl CommandAction for UpgradeVMConfigProposalCommand {
    type State = CliState;
    type GlobalOpt = StarcoinOpt;
    type Opt = UpgradeVMConfigProposalOpt;
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
        match BuiltinNetworkID::from_str(ctx.opt().net.as_str()) {
            Ok(net) => {
                let genesis_config = net.genesis_config().clone();
                let min_action_delay = get_dao_config(cli_state)?.min_action_delay;
                let vm_config_upgrade_proposal =
                    build_vm_config_upgrade_proposal(genesis_config.vm_config, min_action_delay);
                let signed_txn = sign_txn_with_account_by_rpc_client(
                    cli_state,
                    sender,
                    opt.max_gas_amount,
                    opt.gas_price,
                    opt.expiration_time,
                    TransactionPayload::ScriptFunction(vm_config_upgrade_proposal),
                )?;
                let txn_hash = signed_txn.id();
                cli_state.client().submit_transaction(signed_txn)?;

                println!("txn {:#x} submitted.", txn_hash);

                if opt.blocking {
                    ctx.state().watch_txn(txn_hash)?;
                }
                Ok(txn_hash)
            }
            Err(_) => {
                bail!("net name is wrong.")
            }
        }
    }
}
