use axelar_wasm_std::MajorityThreshold;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Storage};
use cw_storage_plus::Item;
use router_api::ChainName;
use xrpl_types::types::XRPLAccountId;

use crate::msg::MigrateMsg;
use crate::state::{Config, CONFIG};

#[cw_serde]
pub struct OldConfig {
    pub multisig: Addr,
    pub coordinator: Addr,
    pub gateway: Addr,
    pub signing_threshold: MajorityThreshold,
    pub xrpl_multisig: XRPLAccountId,
    pub voting_verifier: Addr,
    pub service_registry: Addr,
    pub service_name: String,
    pub chain_name: ChainName,
    pub verifier_set_diff_threshold: u32,
    pub xrpl_transaction_fee: u64,
    pub xrpl_base_reserve: u64,
    pub xrpl_owner_reserve: u64,
    pub ticket_count_threshold: u32,
}

pub const OLD_CONFIG: Item<OldConfig> = Item::new("config");

pub fn migrate_config(
    storage: &mut dyn Storage,
    msg: MigrateMsg,
) -> Result<(), axelar_wasm_std::error::ContractError> {
    let config = OLD_CONFIG.load(storage)?;
    let new_config = Config {
        multisig: config.multisig,
        coordinator: config.coordinator,
        gateway: config.gateway,
        signing_threshold: config.signing_threshold,
        xrpl_multisig: config.xrpl_multisig,
        voting_verifier: config.voting_verifier,
        service_registry: config.service_registry,
        service_name: config.service_name,
        chain_name: config.chain_name,
        verifier_set_diff_threshold: config.verifier_set_diff_threshold,
        xrpl_transaction_fee: config.xrpl_transaction_fee,
        xrpl_base_reserve: config.xrpl_base_reserve,
        xrpl_owner_reserve: config.xrpl_owner_reserve,
        ticket_count_threshold: config.ticket_count_threshold,
        relayer: msg.relayer_address,
    };

    OLD_CONFIG.remove(storage);
    CONFIG.save(storage, &new_config)?;
    Ok(())
}
