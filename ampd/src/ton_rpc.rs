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
use tonlib_core::cell::{Cell, CellParser, TonCellError};
use tonlib_core::tlb_types::traits::TLBObject;
use tonlib_core::TonAddress;
use tracing::info;

use crate::handlers::ton_verify_msg::{FetchingError, Message, TonClient};
use crate::handlers::ton_verify_verifier_set::VerifierSetConfirmation;

trait CellTo {
    fn cell_to_string(self) -> String;

    fn cell_to_buffer(self) -> Vec<u8>;
}

const BYTES_PER_CELL: usize = 96;

impl CellTo for Arc<Cell> {
    fn cell_to_buffer(self) -> Vec<u8> {
        // we have to revert the chain of cells
        let mut current_cell = Some(self);
        let mut u8_vec = vec![];

        while let Some(cell) = current_cell {
            let mut parser = cell.parser();
            for _ in 0..BYTES_PER_CELL {
                let next_byte = match parser.load_uint(8) {
                    Ok(internal) => internal.to_bytes_be()[0],
                    Err(_) => break, // this means we are done
                };
                u8_vec.push(next_byte);
            }
            match parser.next_reference() {
                Ok(r) => current_cell = Some(r),
                _ => break,
            }
        }
        u8_vec
    }

    fn cell_to_string(self) -> String {
        String::from_utf8_lossy(&self.cell_to_buffer()).into()
    }
}

#[derive(PartialEq, Debug)]
struct WeightedSigners {
    dict: HashMap<u16, WeightedSigner>,
    threshold: u128,
    nonce: u128,
}

impl WeightedSigners {
    pub fn new(dict: HashMap<u16, WeightedSigner>, threshold: u128, nonce: u128) -> Self {
        WeightedSigners {
            dict,
            threshold,
            nonce,
        }
    }
}

impl TryFrom<VerifierSet> for WeightedSigners {
    type Error = String;

    fn try_from(verifier_set: VerifierSet) -> Result<Self, Self::Error> {
        let mut dict = HashMap::new();

        for (index_str, signer) in verifier_set.signers {
            let index: u16 = index_str.parse().map_err(|_| "Invalid index key")?;
            let signer_bytes = match signer.pub_key {
                PublicKey::Ed25519(ref hex) => hex.to_vec(),
                _ => return Err("Unsupported public key type".to_string()),
            };

            dict.insert(
                index,
                WeightedSigner {
                    signer: signer_bytes
                        .try_into()
                        .map_err(|_| "Expected 32-byte public key")?,
                    weight: signer.weight.u128(),
                    signature: [0; 64],
                },
            );
        }

        Ok(WeightedSigners::new(
            dict,
            verifier_set.threshold.u128(),
            verifier_set.created_at as u128,
        ))
    }
}

#[derive(Clone, Debug, Copy, PartialEq)]
struct WeightedSigner {
    signer: [u8; 32],
    weight: u128,
    signature: [u8; 64],
}

impl WeightedSigner {
    pub fn new(signer: [u8; 32], weight: u128, signature: [u8; 64]) -> Self {
        WeightedSigner {
            signer,
            weight,
            signature,
        }
    }
}

fn key_reader(key: &BigUint) -> Result<u16, TonCellError> {
    Ok(key.to_u16().unwrap())
}

fn val_reader(parser: &mut CellParser) -> Result<WeightedSigner, TonCellError> {
    let signer_bytes = parser.load_bits(256)?;
    let signer: [u8; 32] = signer_bytes
        .try_into()
        .map_err(|_| TonCellError::InternalError("Failed to convert signer bytes".to_string()))?;

    let weight = parser.load_uint(128)?;
    let weight = weight.to_u128().unwrap();

    let signature_bytes = parser.load_bits(512)?;
    let signature: [u8; 64] = signature_bytes.try_into().map_err(|_| {
        TonCellError::InternalError("Failed to convert signature bytes".to_string())
    })?;

    Ok(WeightedSigner::new(signer, weight, signature))
}

fn parse_rotate_signers_log(
    cell: &Arc<Cell>,
) -> error_stack::Result<WeightedSigners, FetchingError> {
    let mut parser = cell.parser();

    let dict = parser
        .load_dict(16, key_reader, val_reader)
        .map_err(|_| FetchingError::InvalidCall)?;
    let threshold = parser
        .load_uint(128)
        .map_err(|_| FetchingError::InvalidCall)?;
    let nonce = parser
        .load_uint(256)
        .map_err(|_| FetchingError::InvalidCall)?;

    let derived_weighted_signers =
        WeightedSigners::new(dict, threshold.to_u128().unwrap(), nonce.to_u128().unwrap());

    Ok(derived_weighted_signers)
}

fn parse_call_contract_log(
    message_id: HexTxHash,
    cell: &Arc<Cell>,
) -> error_stack::Result<Message, FetchingError> {
    let mut parser = cell.parser();
    let destination_chain = parser
        .next_reference()
        .map_err(|_| FetchingError::InvalidCall)?;

    let destination_chain = destination_chain.cell_to_string();

    let destination_address = parser
        .next_reference()
        .map_err(|_| FetchingError::InvalidCall)?;

    let destination_address = destination_address.cell_to_string();

    let payload = parser
        .next_reference()
        .map_err(|_| FetchingError::InvalidCall)?;

    let _ = payload.cell_to_buffer();

    let source_address = parser
        .load_address()
        .map_err(|_| FetchingError::InvalidCall)?;

    let payload_hash: [u8; 32] = parser
        .load_bits(256)
        .map_err(|_| FetchingError::InvalidCall)?
        .try_into()
        .map_err(|_| FetchingError::InvalidCall)?;

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

pub struct MockTonClient;

#[async_trait]
impl TonClient for MockTonClient {
    async fn verify_verifier_set(
        &self,
        _verifier_set_confirmation: &VerifierSetConfirmation,
        _gateway: &TonAddress,
    ) -> error_stack::Result<bool, FetchingError> {
        Ok(true)
    }

    async fn get_tx(
        &self,
        _tx_hash: &HexTxHash,
        _gateway: &TonAddress,
    ) -> error_stack::Result<Message, FetchingError> {
        Ok(Message {
            message_id: HexTxHash::from_str(
                "949b738e28e46ca279bd339f6967f973f4e1f8f025252be8222c97e115d24e6d",
            )
            .unwrap(),
            destination_address: "0x72D489FC91f33011EC46Efa78d37E02dCC335453".to_string(),
            destination_chain: ChainName::from_str("eth-sepolia").unwrap(),
            source_address: "0QCJitE8BZ8qOmlXagEMQa8jm0h7xVXqvrmliU3rESmTMCkR"
                .parse()
                .unwrap(),
            payload_hash: H256::from_str(
                "72a1d814ac3bc3f32c851b71e1189910e10506aaabf6416303f84d385a736493",
            )
            .unwrap(),
        })
    }
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

const OP_SIGNERS_ROTATED_LOG: &str = "0x0000002A";
const OP_CALL_CONTRACT_STR: &str = "0x00000009";

#[async_trait]
impl TonClient for TonRpcClient {
    async fn verify_verifier_set(
        &self,
        verifier_set_confirmation: &VerifierSetConfirmation,
        gateway: &TonAddress,
    ) -> error_stack::Result<bool, FetchingError> {
        let mut data: HashMap<String, String> = HashMap::new();
        data.insert(
            "hash".to_string(),
            verifier_set_confirmation
                .message_id
                .tx_hash_as_hex_no_prefix()
                .to_string(),
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

        // access result["transactions"][0]["account"] and check if it matches the gateway address
        if let Some(address) = result
            .get("transactions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("account"))
            .and_then(|v| v.as_str())
        {
            if let Ok(address) = TonAddress::from_hex_str(address) {
                if address != *gateway {
                    info!("Call contract was emitted on a contract that is not the gateway");
                    return Err(report!(FetchingError::InvalidCall));
                }
            } else {
                info!("Failed to decode gateway address");
                return Err(report!(FetchingError::Client));
            }
        } else {
            info!("Failed to get gateway address");
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
                info!("Call contract aborted");
                return Err(report!(FetchingError::InvalidCall));
            }
        } else {
            info!("Failed to get aborted value");
            return Err(report!(FetchingError::Client));
        }

        // access result["transactions"][0]["in_msg"]["opcode"] and check if it is OP_CALL_CONTRACT
        if let Some(opcode) = result
            .get("transactions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("in_msg"))
            .and_then(|v| v.get("opcode"))
            .and_then(|v| v.as_str())
        {
            if opcode != OP_SIGNERS_ROTATED_LOG {
                info!("Opcode is different from OP_CALL_CONTRACT");
                return Err(report!(FetchingError::InvalidCall));
            }
        } else {
            info!("Failed to get log at expected index");
            return Err(report!(FetchingError::InvalidCall));
        }

        // access result["transactions"][0]["out_msgs"][0]["message_content"]["body"], load it as a cell and check if it matches the given values
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
            if let Ok(log) = Cell::from_boc_b64(log).and_then(|c| Arc::from_cell(&c)) {
                if let Ok(derived_weighted_signers) = parse_rotate_signers_log(&log) {
                    let expected_weighted_signers =
                        WeightedSigners::try_from(verifier_set_confirmation.verifier_set.clone())
                            .unwrap();

                    return Ok(derived_weighted_signers == expected_weighted_signers);
                } else {
                    info!("Failed to load event body as a contract call data");
                    return Err(report!(FetchingError::InvalidCall));
                }
            } else {
                info!("Failed to load event body as a cell");
                return Err(report!(FetchingError::InvalidCall));
            }
        } else {
            info!("Failed to load event body");
            return Err(report!(FetchingError::InvalidCall));
        }
    }

    async fn get_tx(
        &self,
        tx_hash: &HexTxHash,
        gateway: &TonAddress,
    ) -> error_stack::Result<Message, FetchingError> {
        let mut data: HashMap<String, String> = HashMap::new();
        data.insert("hash".to_string(), tx_hash.to_string());

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

        // access result["transactions"][0]["account"] and check if it matches the gateway address
        if let Some(address) = result
            .get("transactions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("account"))
            .and_then(|v| v.as_str())
        {
            if let Ok(address) = TonAddress::from_hex_str(address) {
                if address != *gateway {
                    info!("Call contract was emitted on a contract that is not the gateway");
                    return Err(report!(FetchingError::InvalidCall));
                }
            } else {
                info!("Failed to decode gateway address");
                return Err(report!(FetchingError::Client));
            }
        } else {
            info!("Failed to get gateway address");
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
                info!("Call contract aborted");
                return Err(report!(FetchingError::InvalidCall));
            }
        } else {
            info!("Failed to get aborted value");
            return Err(report!(FetchingError::Client));
        }

        // access result["transactions"][0]["in_msg"]["opcode"] and check if it is OP_CALL_CONTRACT
        if let Some(opcode) = result
            .get("transactions")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("in_msg"))
            .and_then(|v| v.get("opcode"))
            .and_then(|v| v.as_str())
        {
            if opcode != OP_CALL_CONTRACT_STR {
                info!("Opcode is different from OP_CALL_CONTRACT");
                return Err(report!(FetchingError::InvalidCall));
            }
        } else {
            info!("Failed to get log at expected index");
            return Err(report!(FetchingError::InvalidCall));
        }

        // access result["transactions"][0]["out_msgs"][0]["message_content"]["body"], load it as a cell and check if it matches the given values
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
            if let Ok(log) = Cell::from_boc_b64(log).and_then(|c| Arc::from_cell(&c)) {
                // parse it now
                if let Ok(result) = parse_call_contract_log(tx_hash.clone(), &log) {
                    return Ok(result);
                } else {
                    info!("Failed to load event body as a contract call data");
                    return Err(report!(FetchingError::InvalidCall));
                }
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

    use cosmwasm_std::{Addr, HexBinary, Uint128};
    use multisig::key::PublicKey;
    use multisig::msg::Signer;
    use multisig::verifier_set::VerifierSet;
    use tonlib_core::cell::Cell;
    use tonlib_core::tlb_types::traits::TLBObject;

    use crate::ton_rpc::{parse_rotate_signers_log, WeightedSigners};

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
}
