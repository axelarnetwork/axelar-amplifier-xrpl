use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use axelar_wasm_std::hash::Hash;
use cosmwasm_std::HexBinary;
use error_stack::Result;
use multisig::key::{PublicKey, Signature};
use multisig::msg::SignerWithSig;
use multisig::verifier_set::VerifierSet;
use num_bigint::BigUint;
use router_api::Message;
use sha3::{Digest, Keccak256};
use tonlib_core::cell::{Cell, CellBuilder, TonCellError};
use tonlib_core::tlb_types::traits::TLBObject;
use tonlib_core::TonAddress;

const OP_APPROVE_MESSAGES: usize = 0x00000028;
const OP_START_SIGNER_ROTATION: usize = 0x00000014;
const BYTES_PER_CELL: usize = 96;
const THRESHOLD_BITS: usize = 128;
const NONCE_BITS: usize = 256;
const WEIGHTED_SIGNER_BYTES: usize = 112;
const DICTIONARY_KEY_BITS: usize = 16;
const OPCODE_BITS: usize = 32;
const PAYLOAD_HASH_BITS: usize = 256;
const BITS_PER_BYTE: usize = 8;
const SIGNATURE_BITS: usize = 512;
const SIGNATURE_BYTES: usize = SIGNATURE_BITS / BITS_PER_BYTE;
const SIGNER_PUBKEY_BITS: usize = 256;
const SIGNER_PUBKEY_BYTES: usize = SIGNER_PUBKEY_BITS / BITS_PER_BYTE;

#[allow(clippy::arithmetic_side_effects)]
pub fn build_cell_chain(start_index: usize, buffer: Vec<u8>) -> Result<Cell, TonCellError> {
    let mut builder = CellBuilder::new();
    let end_index = std::cmp::min(start_index + BYTES_PER_CELL, buffer.len());

    // Store bytes in the current cell
    for byte in buffer.iter().take(end_index).skip(start_index) {
        builder.store_uint(BITS_PER_BYTE, &BigUint::from(*byte))?;
    }

    // If there are more bytes, create a reference to the next cell
    if end_index < buffer.len() {
        let next_cell = build_cell_chain(end_index, buffer)?;
        builder.store_reference(&Arc::new(next_cell))?;
    }

    Ok(builder.build()?)
}

fn buffer_to_cell(buffer: Vec<u8>) -> Result<Cell, TonCellError> {
    build_cell_chain(0, buffer)
}

#[derive(Clone, Debug)]
struct TonProof {
    dict: HashMap<u16, WeightedSigner>,
    threshold: u128,
    nonce: u128,
}

impl TonProof {
    pub fn new(set: &VerifierSet, signatures: Vec<SignerWithSig>) -> Self {
        let nonce = set.created_at as u128;
        let threshold = set.threshold.into();

        // todo: convert set.signers to HashMap<u16, WeightedSigner>,
        let dict: HashMap<u16, WeightedSigner> = set
            .signers
            .values()
            .enumerate()
            .map(|(i, signer)| {
                let pub_key_bytes = match &signer.pub_key {
                    PublicKey::Ed25519(key) => key.as_slice().try_into().unwrap(),
                    _ => panic!("Only Ed25519 pubkeys are supported in Ton"),
                };
                let signature_bytes = match &signatures[i].signature {
                    Signature::Ed25519(sig) => sig.as_slice().try_into().unwrap(),
                    _ => panic!("Only Ed25519 signatures are supported in Ton"),
                };
                (
                    u16::try_from(i).unwrap(),
                    WeightedSigner::new(pub_key_bytes, signer.weight.u128(), signature_bytes),
                )
            })
            .collect();

        TonProof {
            dict,
            threshold,
            nonce,
        }
    }

    pub fn to_cell(&self) -> Result<Cell, TonCellError> {
        let mut builder = CellBuilder::new();
        let nonce = BigUint::from(self.nonce);
        let threshold = BigUint::from(self.threshold);

        builder.store_dict(DICTIONARY_KEY_BITS, val_writer_buffer, self.dict.clone())?;
        builder.store_uint(THRESHOLD_BITS, &threshold)?;
        builder.store_uint(NONCE_BITS, &nonce)?;
        let dict_cell = builder.build()?;

        Ok(dict_cell)
    }
}

#[derive(Clone, Debug, Copy)]
struct WeightedSigner {
    signer: [u8; SIGNER_PUBKEY_BYTES],
    weight: u128,
    signature: [u8; SIGNATURE_BYTES],
}

impl WeightedSigner {
    pub fn new(
        signer: [u8; SIGNER_PUBKEY_BYTES],
        weight: u128,
        signature: [u8; SIGNATURE_BYTES],
    ) -> Self {
        WeightedSigner {
            signer,
            weight,
            signature,
        }
    }

    pub fn to_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.signer);
        bytes.extend_from_slice(&self.weight.to_be_bytes());
        bytes.extend_from_slice(&self.signature);
        assert!(bytes.len() == WEIGHTED_SIGNER_BYTES);
        bytes
    }
}

// Custom value writer for WeightedSigner
fn val_writer_buffer(
    builder: &mut CellBuilder,
    val: WeightedSigner,
) -> std::result::Result<(), TonCellError> {
    builder.store_slice(&val.to_bytes())?;
    Ok(())
}

fn construct_proof(
    verifier_set: &VerifierSet,
    signatures: Vec<SignerWithSig>,
) -> Result<Cell, TonCellError> {
    let proof = TonProof::new(verifier_set, signatures);
    proof.to_cell()
}

fn get_arced_cell(inner: &str) -> Result<Arc<Cell>, TonCellError> {
    Ok(Arc::new(buffer_to_cell(inner.as_bytes().to_vec())?))
}

fn message_to_cell(msg: Message) -> Result<Cell, TonCellError> {
    let mut builder = CellBuilder::new();
    builder.store_reference(&get_arced_cell(&msg.cc_id.message_id)?)?;
    builder.store_reference(&get_arced_cell(msg.cc_id.source_chain.as_ref())?)?;
    builder.store_reference(&get_arced_cell(&msg.source_address)?)?;

    let ton_address_hash_buffer = TonAddress::from_str(&msg.destination_address)
        .map_err(|_| TonCellError::InternalError("".to_owned()))?
        .hash_part
        .to_vec();
    let ton_address_hash_buffer_cell = buffer_to_cell(ton_address_hash_buffer)
        .map_err(|_| TonCellError::InternalError("".to_owned()))?;

    let mut last_cell_builder = CellBuilder::new();
    last_cell_builder.store_reference(&Arc::new(ton_address_hash_buffer_cell.clone()))?; // problem this should be the Ton address hash!!! .storeRef(bufferToCell(msg.executableAddress.hash))
    last_cell_builder.store_reference(&get_arced_cell(msg.destination_chain.as_ref())?)?;
    let last_cell = last_cell_builder.build()?;

    builder.store_reference(&Arc::new(last_cell))?;
    builder.store_uint(
        PAYLOAD_HASH_BITS,
        &BigUint::from_bytes_be(&msg.payload_hash),
    )?;

    let res = builder.build()?;
    Ok(res)
}

// Custom value writer for WeightedSigner
fn val_writer_cell(
    builder: &mut CellBuilder,
    val: Message,
) -> std::result::Result<(), TonCellError> {
    builder.store_reference(&Arc::new(
        message_to_cell(val).map_err(|_| TonCellError::InternalError("".to_owned()))?,
    ))?;
    Ok(())
}

#[derive(Debug)]
struct TonMessages {
    dict: HashMap<u16, Message>,
}

impl TonMessages {
    pub fn new(messages: &[Message]) -> Self {
        let msgs_hashmap: HashMap<u16, Message> = messages
            .iter() // Changed from into_iter() to iter()
            .enumerate()
            .map(|(i, msg)| (u16::try_from(i).unwrap(), msg.clone())) // Added clone() since we're borrowing
            .collect();
        TonMessages { dict: msgs_hashmap }
    }

    pub fn to_cell(&self) -> Result<Cell, TonCellError> {
        let mut builder = CellBuilder::new();

        builder.store_dict(DICTIONARY_KEY_BITS, val_writer_cell, self.dict.clone())?;
        let dict_cell = builder.build()?;

        Ok(dict_cell)
    }
}

fn construct_messages(messages: &[Message]) -> Result<Cell, TonCellError> {
    let ton_msgs = TonMessages::new(messages);
    ton_msgs.to_cell()
}

pub fn build_approve_messages_body(
    messages: &[Message],
    verifier_set: &VerifierSet,
    signatures: Vec<SignerWithSig>,
) -> Result<Cell, TonCellError> {
    let proof = construct_proof(verifier_set, signatures)?;
    let messages = construct_messages(messages)?;

    let mut builder = CellBuilder::new();
    builder.store_uint(OPCODE_BITS, &BigUint::from(OP_APPROVE_MESSAGES))?;
    builder.store_reference(&Arc::new(proof))?;
    builder.store_reference(&Arc::new(messages))?;

    Ok(builder.build()?)
}

pub fn build_signer_rotation_body(
    candidate_set: &VerifierSet,
    current_set: &VerifierSet,
    signatures: Vec<SignerWithSig>,
) -> Result<Cell, TonCellError> {
    let proof = construct_proof(current_set, signatures)?;
    let candidate_config_hash = compute_verifier_set_hash(candidate_set);
    let candidate_config_hash_cell = buffer_to_cell(candidate_config_hash.to_vec())?;

    let mut builder = CellBuilder::new();
    builder.store_uint(OPCODE_BITS, &BigUint::from(OP_START_SIGNER_ROTATION))?;
    builder.store_reference(&Arc::new(candidate_config_hash_cell))?;
    builder.store_reference(&Arc::new(proof))?;

    Ok(builder.build()?)
}

fn compute_data_hash(msgs: &[Message]) -> Hash {
    let mut concatenated: Vec<u8> = Vec::new();

    for msg in msgs.iter().cloned() {
        let message_id = msg.cc_id.message_id;
        let source_chain = msg.cc_id.source_chain;
        let source_contract_address = msg.source_address;
        let contract_address = msg.destination_address;
        let destination_chain = msg.destination_chain;
        let payload_hash = msg.payload_hash;

        concatenated.extend(message_id.as_bytes());

        concatenated.extend(source_chain.to_string().as_bytes());
        concatenated.extend(source_contract_address.as_bytes());

        let ton_address_hash_buffer = TonAddress::from_str(&contract_address)
            .map_err(|_| TonCellError::InternalError("".to_owned()))
            .unwrap()
            .hash_part
            .to_vec();

        concatenated.extend(ton_address_hash_buffer);
        concatenated.extend(destination_chain.to_string().as_bytes());
        concatenated.extend(payload_hash.as_slice());
    }

    Keccak256::digest(concatenated).into()
}

#[allow(clippy::arithmetic_side_effects)]
fn compute_verifier_set_hash(verifier_set: &VerifierSet) -> Hash {
    let mut data = Vec::new();
    data.extend(verifier_set.threshold.to_be_bytes());

    // Convert nonce to 256-bit (32 bytes)
    let mut nonce_bytes = [0u8; NONCE_BITS / 8];
    let nonce_be = verifier_set.created_at.to_be_bytes();
    nonce_bytes[NONCE_BITS / 8 - nonce_be.len()..].copy_from_slice(&nonce_be);
    data.extend(nonce_bytes);

    let mut current_hash = Keccak256::digest(&data);

    // Sort the keys lexicographically
    let mut sorted_keys: Vec<&String> = verifier_set.signers.keys().collect();
    sorted_keys.sort();

    for (i, key) in sorted_keys.iter().enumerate() {
        let signer = verifier_set.signers.get(*key).unwrap();

        let mut hasher = Keccak256::new();
        hasher.update((u16::try_from(i).unwrap()).to_be_bytes());
        hasher.update(&signer.pub_key);
        hasher.update(signer.weight.to_be_bytes());
        hasher.update(current_hash);
        current_hash = hasher.finalize();
    }

    current_hash.into()
}

pub fn compute_approve_messages_hash(
    msgs: &[Message],
    verifier_set: &VerifierSet,
    domain_separator: &Hash,
) -> Hash {
    let data_hash = compute_data_hash(msgs);
    let signers_hash = compute_verifier_set_hash(verifier_set);

    let mut result = Vec::new();
    result.extend(data_hash.to_vec());
    result.extend(signers_hash.to_vec());
    result.extend(domain_separator.to_vec());

    Keccak256::digest(result).into()
}

pub fn compute_signer_rotation_hash(
    candidate_set: &VerifierSet,
    current_set: &VerifierSet,
    domain_separator: &Hash,
) -> Hash {
    let candidate_set_hash = compute_verifier_set_hash(candidate_set);
    let current_set_hash = compute_verifier_set_hash(current_set);

    let mut result = Vec::new();

    result.extend(candidate_set_hash.to_vec());
    result.extend(current_set_hash.to_vec());
    result.extend(domain_separator);

    Keccak256::digest(result).into()
}

pub fn cell_to_boc_hex(cell: Cell) -> std::result::Result<String, TonCellError> {
    cell.to_boc_hex(true)
}
