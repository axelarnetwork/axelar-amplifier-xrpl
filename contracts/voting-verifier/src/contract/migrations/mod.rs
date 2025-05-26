use axelar_wasm_std::migrate_from_version;
use axelar_wasm_std::msg_id::MessageIdFormat;
use axelar_wasm_std::nonempty::String as NonEmptyString;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{DepsMut, Empty, Env, Response};

use crate::state::CONFIG;

pub type MigrateMsg = Empty;

#[cfg_attr(not(feature = "library"), entry_point)]
#[migrate_from_version("1.1")]
pub fn migrate(
    deps: DepsMut,
    _env: Env,
    _msg: MigrateMsg,
) -> Result<Response, axelar_wasm_std::error::ContractError> {
    let mut config = CONFIG.load(deps.storage).expect("failed to load config");

    config.msg_id_format = MessageIdFormat::HexTxHashAndEventIndex;
    config.source_gateway_address =
        NonEmptyString::try_from("kQDgkzGhZ3BIKKQ15jp2ShUqOMPJoe6xqcfj7XrCnnbglS-s")
            .expect("gateway address should be non-empty");

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::default())
}
