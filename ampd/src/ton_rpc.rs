use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use axelar_wasm_std::msg_id::HexTxHash;
use error_stack::{report, ResultExt};
use ethers_core::types::H256;
use multisig::key::PublicKey;
use multisig::verifier_set::VerifierSet;
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use reqwest::Client;
use router_api::ChainName;
use serde_json::Value;
use ton_utils::{
    build_cell_chain, cell_parse_call_contract_log, cell_parse_rotate_signers_log, WeightedSigners,
};
use tonlib_core::cell::{Cell, TonCellError};
use tonlib_core::tlb_types::traits::TLBObject;
use tonlib_core::TonAddress;
use tracing::info;

use crate::handlers::ton_verify_msg::{FetchingError, Message};
use crate::handlers::ton_verify_verifier_set::VerifierSetConfirmation;

fn parse_rotate_signers_log(
    cell: &Arc<Cell>,
) -> error_stack::Result<WeightedSigners, FetchingError> {
    Ok(cell_parse_rotate_signers_log(cell).map_err(|_| FetchingError::InvalidCall)?)
}

fn parse_call_contract_log(
    message_id: HexTxHash,
    cell: &Arc<Cell>,
) -> error_stack::Result<Message, FetchingError> {
    let (payload_hash, destination_address, destination_chain, source_address) =
        cell_parse_call_contract_log(cell).map_err(|_| FetchingError::InvalidCall)?;

    let payload_hash = H256::from(payload_hash);

    let destination_chain =
        ChainName::from_str(&destination_chain).map_err(|_| FetchingError::InvalidCall)?;

    Ok(Message {
        message_id,
        payload_hash: H256::from(payload_hash),
        destination_address,
        destination_chain,
        source_address,
    })
}

pub struct TonRpcClient {
    rpc_url: String,
}

impl TonRpcClient {
    pub fn new(rpc_url: &str) -> Self {
        TonRpcClient {
            rpc_url: rpc_url.to_owned(),
        }
    }
}

const OP_SIGNERS_ROTATED: u32 = 0x0000002A;
const OP_CALL_CONTRACT: u32 = 0x00000009;

pub struct TonLog {
    opcode: u32,
    cell: Arc<Cell>,
}

#[async_trait::async_trait]
pub trait TonClient: Send + Sync + 'static {
    async fn get_log(
        &self,
        contract_address: &TonAddress,
        tx_hash: &HexTxHash,
    ) -> error_stack::Result<TonLog, FetchingError>;
}

pub async fn verify_call_contract(
    ton_rpc: &impl TonClient,
    gateway: &TonAddress,
    expected_message: &Message,
) -> bool {
    let log: Result<TonLog, _> = ton_rpc.get_log(gateway, &expected_message.message_id).await;
    if log.is_err() {
        print!("Getting log failed");
        return false;
    }
    let log = log.unwrap();

    // check that opcode is correct
    if log.opcode != OP_CALL_CONTRACT {
        print!("Comparing opcode failed");
        info!(
            "Invalid opcode, got {} expected {}",
            log.opcode, OP_CALL_CONTRACT
        );
        return false;
    }

    // decode cell
    if let Ok(result) = parse_call_contract_log(expected_message.message_id.clone(), &log.cell) {
        // compare
        if result != *expected_message {
            print!("Caimed event is incorrect");
            info!(
                "Claimed event is incorrect: Got {:?} but expected {:?}",
                result, expected_message
            );
            return false;
        }
    } else {
        print!("Failed to parse event body");
        info!("Failed to parse event body as a contract call data");
        return false;
    }
    return true;
}

pub async fn verify_verifier_set(
    ton_rpc: &impl TonClient,
    gateway: &TonAddress,
    expected_verifier_set: &VerifierSetConfirmation,
) -> bool {
    let log: Result<TonLog, _> = ton_rpc
        .get_log(gateway, &expected_verifier_set.message_id)
        .await;
    if log.is_err() {
        return false;
    }
    let log = log.unwrap();

    // check that opcode is correct
    if log.opcode != OP_SIGNERS_ROTATED {
        info!(
            "Invalid opcode, got {} expected {}",
            log.opcode, OP_SIGNERS_ROTATED
        );
        return false;
    }

    // decode cell
    if let Ok(derived_weighted_signers) = parse_rotate_signers_log(&log.cell) {
        let expected_weighted_signers =
            WeightedSigners::try_from(expected_verifier_set.verifier_set.clone());
        if expected_weighted_signers.is_err() {
            info!("Failed to convert verifier set to weighted signers");
            return false;
        }
        let expected_weighted_signers = expected_weighted_signers.unwrap();

        if derived_weighted_signers != expected_weighted_signers {
            info!(
                "Claimed event is incorrect: Got {:?} but expected {:?}",
                derived_weighted_signers, expected_weighted_signers
            );
            return false;
        }
    } else {
        info!("Failed to parse event body as a contract call data");
        return false;
    }
    return true;
}

#[async_trait]
impl TonClient for TonRpcClient {
    async fn get_log(
        &self,
        contract_address: &TonAddress,
        tx_hash: &HexTxHash,
    ) -> error_stack::Result<TonLog, FetchingError> {
        let mut data: HashMap<String, String> = HashMap::new();
        data.insert(
            "hash".to_string(),
            tx_hash.tx_hash_as_hex_no_prefix().to_string(),
        );

        let method = "transactions";

        let client = Client::new();

        let res = client
            .get(format!("{}/{}", self.rpc_url, method))
            .query(&data)
            .send()
            .await
            .change_context(FetchingError::Client)?;

        let status = res.status();
        let text = res.text().await.change_context(FetchingError::Client)?;

        if !status.is_success() {
            info!("RPC query failed");
            return Err(report!(FetchingError::Client));
        }

        let result: Value = serde_json::from_str(&text).change_context(FetchingError::Client)?;

        if let Some(transactions) = result.get("transactions").and_then(|v| v.as_array()) {
            // check the size of the response
            if transactions.is_empty() {
                info!("Transaction not found");
                return Err(report!(FetchingError::NotFound));
            }
        } else {
            info!("Failed to get transactions array");
            return Err(report!(FetchingError::Client));
        }

        // access result["transactions"][0]["account"] and check if it matches the provided address
        if let Some(address) = result
            .get("transactions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("account"))
            .and_then(|v| v.as_str())
        {
            if let Ok(address) = TonAddress::from_hex_str(address) {
                if address != *contract_address {
                    info!("Log was emitted on a different contract");
                    return Err(report!(FetchingError::InvalidCall));
                }
            } else {
                info!("Failed to decode contract address");
                return Err(report!(FetchingError::Client));
            }
        } else {
            info!("Failed to get contract address");
            return Err(report!(FetchingError::Client));
        }

        // access result["transactions"][0]["description"]["aborted"] and check if it is false
        if let Some(aborted) = result
            .get("transactions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("description"))
            .and_then(|v| v.get("aborted"))
            .and_then(|v| v.as_bool())
        {
            if aborted {
                info!("Transaction aborted");
                return Err(report!(FetchingError::InvalidCall));
            }
        } else {
            info!("Failed to get aborted value");
            return Err(report!(FetchingError::Client));
        }

        let opcode: u32;
        // get result["transactions"][0]["in_msg"]["opcode"]
        if let Some(_opcode) = result
            .get("transactions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("in_msg"))
            .and_then(|v| v.get("opcode"))
            .and_then(|v| v.as_str())
        {
            match u32::from_str_radix(_opcode.trim_start_matches("0x"), 16) {
                Ok(_opcode) => opcode = _opcode,
                Err(_) => {
                    info!("Failed to decode opcode");
                    return Err(report!(FetchingError::InvalidCall));
                }
            }
        } else {
            info!("Failed to get log at expected index");
            return Err(report!(FetchingError::InvalidCall));
        }

        // access result["transactions"][0]["out_msgs"][0]["message_content"]["body"], load it as a cell
        if let Some(log) = result
            .get("transactions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("out_msgs"))
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("message_content"))
            .and_then(|v| v.get("body"))
            .and_then(|v| v.as_str())
        {
            // attempt to parse this log as a cell
            if let Ok(cell) = Cell::from_boc_b64(log).and_then(|c| Arc::from_cell(&c)) {
                return Ok(TonLog { opcode, cell });
            } else {
                info!("Failed to load event body as a cell");
                return Err(report!(FetchingError::InvalidCall));
            }
        } else {
            info!("Failed to load event body");
            return Err(report!(FetchingError::InvalidCall));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use axelar_wasm_std::msg_id::HexTxHash;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use cosmwasm_std::{Addr, HexBinary, Uint128};
    use error_stack::report;
    use multisig::key::PublicKey;
    use multisig::msg::Signer;
    use multisig::verifier_set::VerifierSet;
    use tonlib_core::cell::Cell;
    use tonlib_core::tlb_types::traits::TLBObject;
    use tonlib_core::TonAddress;

    use crate::ton_rpc::{
        parse_call_contract_log, parse_rotate_signers_log, verify_call_contract,
        verify_verifier_set, FetchingError, Message, TonClient, TonLog, TonRpcClient,
        VerifierSetConfirmation, WeightedSigners, OP_CALL_CONTRACT,
    };

    const TEST_GATEWAY_ADDRESS: &str = "EQCd5sQG0Swz5pyNMZfh1a_J7GUykPQDr0oFMUq4oEfes27G";
    const TEST_EXAMPLE_TX_LOG_CALL_CONTRACT: &str = "te6cckEBBAEA5QADg4AcPMZ9bgNiMWiFLuLZ3ODT3Qj2rbcRiS/f1NA9opZaWPXUykhs4AH2lBVEFjqex7VaPbPTvuLH5GEs5sIeXm+pcAECAwAcYXZhbGFuY2hlLWZ1amkAVDB4ZDcwNjdBZTNDMzU5ZTgzNzg5MGIyOEI3QkQwZDIwODRDZkRmNDliNQDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE0hlbGxvIGZyb20gUmVsYXllciEAAAAAAAAAAAAAAAAAne0F4Q==";
    const TEST_EXAMPLE_TX_HASH_CALL_CONTRACT: &str = "jq3K6fvoS5e3DwwW4V2N6pxRyB+9BYYBpn0Ps6Qq7Z8=";
    const TEST_EXAMPLE_TX_LOG_SIGNER_ROTATION: &str = "te6cckECCAEAAg8AAWGAAAAAAAAAAAAAAAAAAAABgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADAAQICzgIFAgEgAwQA4QDoQe/884Qvh1w3RjnS8CZZ+TWMJulDV8d3IZkElUxuAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAOEQ83AI9ItX54QfRoGk0V9NdHRDrfSHHIRkvVvXeQGZdMAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIAIBIAYHAOESB7edy2hV4XJ5ZoIYgG4w/nDBxKeP8bX80qk3+1YFOUAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIADhHm1Vi6P5lT5QHixEuipi6eQH4U65pW+1+DjkQutBJZkAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACDebNbp";
    const TEST_EXAMPLE_TX_HASH_SIGNER_ROTATION: &str =
        "90LLog5oCVb2pkwwUVAloQRX3pyJ0xI54BKqWHDajwo=";

    pub struct MockTonClient;

    #[async_trait]
    impl TonClient for MockTonClient {
        async fn get_log(
            &self,
            contract_address: &TonAddress,
            tx_hash: &HexTxHash,
        ) -> error_stack::Result<TonLog, FetchingError> {
            let example_tx_hash = STANDARD.decode(TEST_EXAMPLE_TX_HASH_CALL_CONTRACT).unwrap();
            let example_tx_hash: [u8; 32] = example_tx_hash.try_into().unwrap();
            if tx_hash.tx_hash == example_tx_hash {
                if *contract_address == TonAddress::from_base64_url(TEST_GATEWAY_ADDRESS).unwrap() {
                    print!("Returning a TonLog object now");
                    return Ok(TonLog {
                        opcode: OP_CALL_CONTRACT,
                        cell: Arc::new(
                            Cell::from_boc_b64(TEST_EXAMPLE_TX_LOG_CALL_CONTRACT).unwrap(),
                        ),
                    });
                } else {
                    print!("Didn't ask for the gateway!");
                    return Err(report!(FetchingError::InvalidCall));
                }
            }
            print!("Unknown transaction!");
            return Err(report!(FetchingError::NotFound));
        }
    }

    #[test]
    fn should_parse_signers_rotated_log() {
        let signers_rotated_log = "te6cckECCAEAAg8AAWGAAAAAAAAAAAAAAAAAAAABgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADAAQICzgIFAgEgAwQA4QDoQe/884Qvh1w3RjnS8CZZ+TWMJulDV8d3IZkElUxuAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAOEQ83AI9ItX54QfRoGk0V9NdHRDrfSHHIRkvVvXeQGZdMAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIAIBIAYHAOESB7edy2hV4XJ5ZoIYgG4w/nDBxKeP8bX80qk3+1YFOUAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIADhHm1Vi6P5lT5QHixEuipi6eQH4U65pW+1+DjkQutBJZkAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACDebNbp";
        let signers_rotated_log_cell = Arc::new(Cell::from_boc_b64(signers_rotated_log).unwrap());

        let expected_verifier_set = get_expected_signers();
        let expected_weighted_signers =
            WeightedSigners::try_from(expected_verifier_set.clone()).unwrap();

        let derived_weighted_signers = parse_rotate_signers_log(&signers_rotated_log_cell).unwrap();
        assert_eq!(derived_weighted_signers, expected_weighted_signers);
    }

    fn get_expected_signers() -> VerifierSet {
        let mut expected_signers = BTreeMap::new();
        expected_signers.insert(
            "0".to_string(),
            Signer {
                address: Addr::unchecked(""),
                weight: Uint128::from(1u128),
                pub_key: PublicKey::Ed25519(
                    HexBinary::from_hex(
                        "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8",
                    )
                    .unwrap(),
                ),
            },
        );
        expected_signers.insert(
            "1".to_string(),
            Signer {
                address: Addr::unchecked(""),
                weight: Uint128::from(1u128),
                pub_key: PublicKey::Ed25519(
                    HexBinary::from_hex(
                        "43cdc023d22d5f9e107d1a0693457d35d1d10eb7d21c721192f56f5de40665d3",
                    )
                    .unwrap(),
                ),
            },
        );
        expected_signers.insert(
            "2".to_string(),
            Signer {
                address: Addr::unchecked(""),
                weight: Uint128::from(1u128),
                pub_key: PublicKey::Ed25519(
                    HexBinary::from_hex(
                        "481ede772da15785c9e59a086201b8c3f9c307129e3fc6d7f34aa4dfed5814e5",
                    )
                    .unwrap(),
                ),
            },
        );
        expected_signers.insert(
            "3".to_string(),
            Signer {
                address: Addr::unchecked(""),
                weight: Uint128::from(1u128),
                pub_key: PublicKey::Ed25519(
                    HexBinary::from_hex(
                        "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664",
                    )
                    .unwrap(),
                ),
            },
        );

        VerifierSet {
            signers: expected_signers,
            threshold: Uint128::from(3u128),
            created_at: 1,
        }
    }

    #[tokio::test]
    async fn should_accept_correct_call_contract() {
        let mock_client = MockTonClient;
        print!("Setup mock client");

        let correct_gateway = TonAddress::from_base64_url(TEST_GATEWAY_ADDRESS).unwrap();
        let expected_message_cell =
            Arc::new(Cell::from_boc_b64(TEST_EXAMPLE_TX_LOG_CALL_CONTRACT).unwrap());
        print!("Expected message cell is {:?}", expected_message_cell);

        let example_tx_hash = STANDARD.decode(TEST_EXAMPLE_TX_HASH_CALL_CONTRACT).unwrap();
        let example_tx_hash: [u8; 32] = example_tx_hash.try_into().unwrap();
        let message_id = HexTxHash::new(example_tx_hash);

        let expected_message = parse_call_contract_log(message_id, &expected_message_cell).unwrap();
        print!("Expected message is {:?}", expected_message);

        let result = verify_call_contract(&mock_client, &correct_gateway, &expected_message).await;
        assert_eq!(result, true);
    }

    #[tokio::test]
    async fn should_reject_invalid_call_contract_invalid_gateway() {
        let mock_client = MockTonClient;
        print!("Setup mock client");

        let incorrect_gateway =
            TonAddress::from_base64_url("EQC4ekoPZEt6GG7nGhRUY09wwipirKGmumdrUXXCHX_ZMELQ")
                .unwrap();
        let expected_message_cell =
            Arc::new(Cell::from_boc_b64(TEST_EXAMPLE_TX_LOG_CALL_CONTRACT).unwrap());
        print!("Expected message cell is {:?}", expected_message_cell);

        let example_tx_hash = STANDARD.decode(TEST_EXAMPLE_TX_HASH_CALL_CONTRACT).unwrap();
        let example_tx_hash: [u8; 32] = example_tx_hash.try_into().unwrap();
        let message_id = HexTxHash::new(example_tx_hash);

        let expected_message = parse_call_contract_log(message_id, &expected_message_cell).unwrap();
        print!("Expected message is {:?}", expected_message);

        let result =
            verify_call_contract(&mock_client, &incorrect_gateway, &expected_message).await;
        assert_eq!(result, false);
    }

    #[tokio::test]
    async fn should_reject_incorrect_call_contract_invalid_hash() {
        let mock_client = MockTonClient;
        print!("Setup mock client");

        let correct_gateway = TonAddress::from_base64_url(TEST_GATEWAY_ADDRESS).unwrap();
        let expected_message_cell =
            Arc::new(Cell::from_boc_b64(TEST_EXAMPLE_TX_LOG_CALL_CONTRACT).unwrap());
        print!("Expected message cell is {:?}", expected_message_cell);

        let bad_tx_hash = STANDARD
            .decode("TbNq7Zr/5q37Y1fQ1wXyFIU/pbFXBfSz8j9TuUD0b64=")
            .unwrap();
        let bad_tx_hash: [u8; 32] = bad_tx_hash.try_into().unwrap();
        let bad_message_id = HexTxHash::new(bad_tx_hash);

        let expected_message =
            parse_call_contract_log(bad_message_id, &expected_message_cell).unwrap();
        print!("Expected message is {:?}", expected_message);

        let result = verify_call_contract(&mock_client, &correct_gateway, &expected_message).await;
        assert_eq!(result, false);
    }

    #[tokio::test]
    async fn should_reject_incorrect_call_contract_invalid_data() {
        let mock_client = MockTonClient;
        print!("Setup mock client");

        let correct_gateway = TonAddress::from_base64_url(TEST_GATEWAY_ADDRESS).unwrap();
        let expected_message_cell =
            Arc::new(Cell::from_boc_b64(TEST_EXAMPLE_TX_LOG_CALL_CONTRACT).unwrap());
        print!("Expected message cell is {:?}", expected_message_cell);

        let example_tx_hash = STANDARD.decode(TEST_EXAMPLE_TX_HASH_CALL_CONTRACT).unwrap();
        let example_tx_hash: [u8; 32] = example_tx_hash.try_into().unwrap();
        let message_id = HexTxHash::new(example_tx_hash);

        let mut bad_message = parse_call_contract_log(message_id, &expected_message_cell).unwrap();
        bad_message.destination_address = String::from("Bad String");
        print!("Bad message is {:?}", bad_message);

        let result = verify_call_contract(&mock_client, &correct_gateway, &bad_message).await;
        assert_eq!(result, false);
    }

    /*#[tokio::test]
    async fn should_accept_correct_signer_rotation() {
        let mock_client = MockTonClient;
        print!("Setup mock client");

        let correct_gateway = TonAddress::from_base64_url(TEST_GATEWAY_ADDRESS).unwrap();
        let expected_message_cell =
            Arc::new(Cell::from_boc_b64(TEST_EXAMPLE_TX_LOG_SIGNER_ROTATION).unwrap());
        print!("Expected message cell is {:?}", expected_message_cell);

        let example_tx_hash = STANDARD
            .decode(TEST_EXAMPLE_TX_HASH_SIGNER_ROTATION)
            .unwrap();
        let example_tx_hash: [u8; 32] = example_tx_hash.try_into().unwrap();
        let message_id = HexTxHash::new(example_tx_hash);

        let expected_message = parse_rotate_signers_log(&expected_message_cell).unwrap();

        let mut signers = BTreeMap::new();
        let string = "axelar1a7pv7s2ngj0t29jrpecck93af7dhnrpfy295x4".into();
        let signer;
        signers.insert(string, signer);
        let threshold = 10_u128.into();
        let created_at = 0_u64;

        let verifier_set = VerifierSet {
            signers,
            threshold,
            created_at,
        };

        let verifier_set_confirmation = VerifierSetConfirmation {
            message_id,
            verifier_set,
        };
        print!("Expected message is {:?}", verifier_set_confirmation);

        let result =
            verify_verifier_set(&mock_client, &correct_gateway, &verifier_set_confirmation).await;
        assert_eq!(result, true);
    }

    #[allow(clippy::arithmetic_side_effects)]
    pub fn build_verifier_set(key_type: KeyType, signers: &[TestSigner]) -> VerifierSet {
        let mut total_weight = Uint128::zero();
        let participants = signers
            .iter()
            .map(|signer| {
                total_weight += Uint128::one();
                (
                    Participant {
                        address: signer.address.clone(),
                        weight: Uint128::one().try_into().unwrap(),
                    },
                    PublicKey::try_from((key_type, signer.pub_key.clone())).unwrap(),
                )
            })
            .collect::<Vec<_>>();

        VerifierSet::new(participants, total_weight.mul_ceil((2u64, 3u64)), 0)
    }*/
}
