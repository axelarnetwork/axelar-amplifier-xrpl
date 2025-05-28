use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Error};
use async_trait::async_trait;
use axelar_wasm_std::msg_id::HexTxHash;
use error_stack::{report, ResultExt};
use ethers_core::types::H256;
use router_api::ChainName;
use serde_json::Value;
use tonlib_core::cell::Cell;
use tonlib_core::TonAddress;
use tracing::info;

use crate::handlers::ton_verify_msg::{FetchingError, Message, TonClient};

trait CellTo {
    fn cell_to_string(self) -> Result<String, Error>;

    fn cell_to_buffer(self) -> Result<Vec<u8>, Error>;
}

const BYTES_PER_CELL: usize = 96;

impl CellTo for Arc<Cell> {
    fn cell_to_buffer(self) -> Result<Vec<u8>, Error> {
        // we have to revert the chain of cells
        let mut current_cell = Some(self);
        let mut u8_vec = vec![];

        while let Some(cell) = current_cell {
            let mut parser = cell.parser();
            for _ in 0..BYTES_PER_CELL {
                let next_byte: u8;
                match parser.load_uint(8) {
                    Ok(internal) => next_byte = internal.to_bytes_be()[0],
                    Err(_) => break, // this means we are done
                }
                u8_vec.push(next_byte);
            }
            match parser.next_reference() {
                Ok(r) => current_cell = Some(r),
                _ => break,
            }
        }
        Ok(u8_vec)
    }

    fn cell_to_string(self) -> Result<String, Error> {
        Ok(String::from_utf8(self.cell_to_buffer()?)?)
    }
}

fn parse_call_contract_log(
    message_id: HexTxHash,
    cell: &Arc<Cell>,
) -> error_stack::Result<Message, FetchingError> {
    let mut parser = cell.parser();
    let destination_chain = parser
        .next_reference()
        .map_err(|_| FetchingError::InvalidCall)?;

    let destination_chain = destination_chain
        .cell_to_string()
        .map_err(|_| FetchingError::InvalidCall)?;

    let destination_address = parser
        .next_reference()
        .map_err(|_| FetchingError::InvalidCall)?;

    let destination_address = destination_address
        .cell_to_string()
        .map_err(|_| FetchingError::InvalidCall)?;

    let payload = parser
        .next_reference()
        .map_err(|_| FetchingError::InvalidCall)?;

    let _ = payload
        .cell_to_buffer()
        .map_err(|_| FetchingError::InvalidCall)?;

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

    return Ok(Message {
        message_id,
        payload_hash: H256::from(payload_hash),
        destination_address,
        destination_chain,
        source_address,
    });
}

// Mock implementation that always returns success
pub struct MockTonClient;

#[async_trait]
impl TonClient for MockTonClient {
    async fn get_tx(
        &self,
        _tx_hash: &HexTxHash,
        _gateway: &TonAddress,
    ) -> error_stack::Result<Message, FetchingError> {
        // Always return Some("success") to simulate a successful transaction
        Ok(Message {
            message_id: HexTxHash::from_str(
                "949b738e28e46ca279bd339f6967f973f4e1f8f025252be8222c97e115d24e6d".into(),
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

// Real implementation that you can use with actual TON RPC
pub struct TonRpcClient {
    rpc_url: String,
    // Add any other fields you need
}

impl TonRpcClient {
    pub fn new(rpc_url: &str) -> Self {
        TonRpcClient {
            rpc_url: rpc_url.to_owned(),
        }
    }
}

use std::any;
use std::collections::hash_map::Iter;
use std::collections::HashMap;

use reqwest::{Client, Response}; // TODO: remove that
use tonlib_core::tlb_types::tlb::TLB;

const OP_CALL_CONTRACT_STR: &str = "0x00000009";

#[async_trait]
impl TonClient for TonRpcClient {
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
            .get(&format!("{}/{}", self.rpc_url, method))
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
            if transactions.len() == 0 {
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
