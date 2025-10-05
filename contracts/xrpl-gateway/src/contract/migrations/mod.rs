use axelar_wasm_std::address;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Api, Storage};
use cw_storage_plus::Item;
use interchain_token_service::TokenId;
use router_api::ChainName;
use xrpl_types::types::XRPLAccountId;

use crate::msg::MigrateMsg;
use crate::state;

#[cw_serde]
pub struct OldConfig {
    pub verifier: Addr,
    pub router: Addr,
    pub its_hub: Addr,
    pub its_hub_chain_name: ChainName,
    pub chain_name: ChainName,
    pub xrpl_multisig: XRPLAccountId,
    pub xrp_token_id: TokenId,
}

pub const OLD_CONFIG: Item<OldConfig> = Item::new("config");

pub fn migrate_config(
    storage: &mut dyn Storage,
    api: &dyn Api,
    msg: MigrateMsg,
) -> Result<(), axelar_wasm_std::error::ContractError> {
    let config = OLD_CONFIG.load(storage)?;

    let prover = address::validate_cosmwasm_address(api, msg.prover_address.as_str())?;
    let relayer = address::validate_cosmwasm_address(api, msg.relayer_address.as_str())?;

    let new_config = state::Config {
        verifier: config.verifier,
        prover,
        relayer,
        router: config.router,
        its_hub: config.its_hub,
        its_hub_chain_name: config.its_hub_chain_name,
        chain_name: config.chain_name,
        xrpl_multisig: config.xrpl_multisig,
        xrp_token_id: config.xrp_token_id,
    };

    OLD_CONFIG.remove(storage);
    state::save_config(storage, &new_config)?;
    Ok(())
}
