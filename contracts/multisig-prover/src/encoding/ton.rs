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
use ton_utils::{
    build_approve_messages_body, build_signer_rotation_body, cell_to_boc_hex,
    compute_approve_messages_hash, compute_signer_rotation_hash,
};
use tonlib_core::cell::Cell;

use crate::error::ContractError;
use crate::payload::Payload;

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

pub fn payload_digest(
    domain_separator: &Hash,
    current_set: &VerifierSet,
    payload: &Payload,
) -> Result<Hash, ContractError> {
    let hash = match payload {
        Payload::Messages(msgs) => {
            compute_approve_messages_hash(msgs, current_set, domain_separator)
        }
        Payload::VerifierSet(candidate_set) => {
            compute_signer_rotation_hash(candidate_set, current_set, domain_separator)
        }
    };

    Ok(hash)
}

pub fn encode_execute_data(
    verifier_set: &VerifierSet,
    signatures: Vec<SignerWithSig>,
    payload: &Payload,
) -> Result<HexBinary, ContractError> {
    let cell_payload = match payload {
        Payload::Messages(msgs) => build_approve_messages_body(msgs, verifier_set, signatures)
            .map_err(|_| ContractError::TonError)?,
        Payload::VerifierSet(candidate_set) => {
            build_signer_rotation_body(candidate_set, verifier_set, signatures)
                .map_err(|_| ContractError::TonError)?
        }
    };

    let cell_hex = cell_to_boc_hex(cell_payload).map_err(|_| ContractError::TonError)?;

    Ok(HexBinary::from_hex(&cell_hex).map_err(|_| ContractError::TonError)?)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;

    use axelar_wasm_std::{nonempty, Participant};
    use cosmwasm_std::{Addr, HexBinary, Uint128};
    use itertools::Itertools;
    use multisig::key::{KeyTyped, Signature};
    use multisig::msg::{Signer, SignerWithSig};
    use multisig::verifier_set::VerifierSet;
    use router_api::{CrossChainId, Message};

    use super::{encode_execute_data, payload_digest};
    use crate::Payload;

    #[test]
    fn should_encode_approve_messages() {
        let verifier_set = curr_ton_verifier_set();
        let payload = Payload::Messages(ton_messages());

        let sigs: Vec<_> = vec![
            "6dce1b2f0a4e14c81d7ed24326d16cb38a596dd34318f0c28f84041cef8537331750c6273898dd8ea3d614ab5101cab27eb36f00940c0dbdcf2111bc8bd79f0f",
            "cbda5213e3a30172fb88b4038c39d223be29bc4656a36c4078ec7eaaaf8e4e496d72a0ba3af1e1d53f1d988d7bf68ab6968654bf031b915fa660245c09a81c07",
            "d3ab834fc46a5eaccdfdf36b28cc1df759b26d98520303fe5643ee1480cb17f65758314043e9734922c200f14b236a898618679d3a0cc2a203ee094f470efa0f",
        ].into_iter().map(|sig| HexBinary::from_hex(sig).unwrap()).collect();

        let signers_with_sigs = signers_with_sigs(verifier_set.signers.values(), sigs);

        let encoded_execute_data =
            encode_execute_data(&verifier_set, signers_with_sigs, &payload).unwrap();

        assert_eq!(encoded_execute_data.to_string(), "b5ee9c72410210010002750002080000002801020161800000000000000000000000000000018000000000000000000000000000000000000000000000000000000000000000c0030101c0040202ce05060102d007020120080900e1479b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad04966400000000000000000000000000000001d3ab834fc46a5eaccdfdf36b28cc1df759b26d98520303fe5643ee1480cb17f65758314043e9734922c200f14b236a898618679d3a0cc2a203ee094f470efa0f8044056570de287d73cd1cb6092bb8fdee6173974955fdef345ae579ee9f475ea74320a0b0c0d00e100e841effcf3842f875c374639d2f02659f9358c26e94357c777219904954c6e000000000000000000000000000000005b7386cbc2938532075fb490c9b45b2ce2965b74d0c63c30a3e101073be14dccc5d43189ce263763a8f5852ad44072ac9facdbc02503036f73c8446f22f5e7c3e000e110f37008f48b57e7841f4681a4d15f4d747443adf4871c8464bd5bd779019974c000000000000000000000000000000072f69484f8e8c05cbee22d00e30e7488ef8a6f1195a8db101e3b1faaabe393925b5ca82e8ebc78754fc766235efda2ada5a1952fc0c6e457e9980917026a0701e000883078666638323263383838303738353966663232366235386532346632343937346137306630346239343432353031616533386664363635623363363866333833342d30001267616e616368652d31005430783532343434663138333541646330323038366333374362323236353631363035653245313639396202000e0f00404686a2c066c784a915f3e01c853d3195ed254c948e21adbb3e4a9b3f5f3c74d70006746f6eb488ecd4");
    }

    #[test]
    fn should_encode_rotate_signers() {
        let verifier_set = curr_ton_verifier_set();

        let mut new_ton_set = curr_ton_verifier_set();
        new_ton_set.created_at += 1;
        let payload = Payload::VerifierSet(new_ton_set);

        let sigs: Vec<_> = vec![
            "63d43de6b5780ea29849a82b9b616c3a7c8c5332e5a1b34408c2745eabf07e2c7d539134e3480e3b3e1ac689f9fff05047b9bb1ecf1208732cf53a39ae0fe701",
            "e619721e05b552e3090dc4a48624ada4ff91a4a4fbd94e2347f1e9716d95c9f1e43c29c1613838ef7beb2daec16c036d0942cc274d15ea64020c5fd94af94205",
            "9b7265c9660f8dd37e99e6c8e4e5fc020a1f0ddb9d55c3f352e826990af144485903cb41b47d6091f7c753ff5de667414be03bfe6a1d3f06513d949005a3500c",
        ].into_iter().map(|sig| HexBinary::from_hex(sig).unwrap()).collect();

        let signers_with_sigs = signers_with_sigs(verifier_set.signers.values(), sigs);

        let encoded_execute_data =
            encode_execute_data(&verifier_set, signers_with_sigs, &payload).unwrap();

        assert_eq!(encoded_execute_data.to_string(), "b5ee9c72410208010001c10002080000001401020040ab98abb510250ae97f3834f06829b35e08d6711dd57753b9c16307aadb4e5d5c0161800000000000000000000000000000018000000000000000000000000000000000000000000000000000000000000000c0030202ce0405020120060700e1479b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664000000000000000000000000000000019b7265c9660f8dd37e99e6c8e4e5fc020a1f0ddb9d55c3f352e826990af144485903cb41b47d6091f7c753ff5de667414be03bfe6a1d3f06513d949005a3500c800e100e841effcf3842f875c374639d2f02659f9358c26e94357c777219904954c6e0000000000000000000000000000000058f50f79ad5e03a8a6126a0ae6d85b0e9f2314ccb9686cd102309d17aafc1f8b1f54e44d38d2038ecf86b1a27e7ffc1411ee6ec7b3c4821ccb3d4e8e6b83f9c06000e110f37008f48b57e7841f4681a4d15f4d747443adf4871c8464bd5bd779019974c000000000000000000000000000000079865c87816d54b8c243712921892b693fe469293ef65388d1fc7a5c5b65727c790f0a70584e0e3bdefacb6bb05b00db4250b309d3457a99008317f652be5081608ba483f1");
    }

    #[test]
    fn should_compute_correct_approve_messages_hash() {
        let domain_separator = ton_domain_separator();
        let verifier_set = curr_ton_verifier_set();
        let payload = Payload::Messages(ton_messages());

        let payload_digest = payload_digest(&domain_separator, &verifier_set, &payload).unwrap();

        assert_eq!(
            hex_encode(payload_digest.to_vec().as_slice()),
            "e8214b369d9f4b11f4f0f7d2c921b56e9a34033e8f7f8599abe819b3cb7de2e0"
        );
    }

    #[test]
    fn should_compute_correct_signer_rotation() {
        let domain_separator = ton_domain_separator();
        let verifier_set = curr_ton_verifier_set();

        let mut new_ton_set = curr_ton_verifier_set();
        new_ton_set.created_at += 1;
        let payload = Payload::VerifierSet(new_ton_set);

        let payload_digest = payload_digest(&domain_separator, &verifier_set, &payload).unwrap();

        assert_eq!(
            hex_encode(payload_digest.to_vec().as_slice()),
            "6fda89e65b639ef93667446d1b083a1038be0d3f004db53886fd26b3970b98ba"
        );
    }

    fn signers_with_sigs<'a>(
        signers: impl Iterator<Item = &'a Signer>,
        sigs: Vec<HexBinary>,
    ) -> Vec<SignerWithSig> {
        signers
            .sorted_by(|s1, s2| Ord::cmp(&s1.pub_key, &s2.pub_key))
            .zip(sigs)
            .map(|(signer, sig)| {
                signer.with_sig(Signature::try_from((signer.pub_key.key_type(), sig)).unwrap())
            })
            .collect()
    }

    fn curr_ton_verifier_set() -> VerifierSet {
        let pub_keys = vec![
            "03A107BFF3CE10BE1D70DD18E74BC09967E4D6309BA50D5F1DDC8664125531B8",
            "43CDC023D22D5F9E107D1A0693457D35D1D10EB7D21C721192F56F5DE40665D3",
            "79B5562E8FE654F94078B112E8A98BA7901F853AE695BED7E0E3910BAD049664",
        ];

        ton_verifier_set_from_pub_keys(&pub_keys)
    }

    fn ton_verifier_set_from_pub_keys(pub_keys: &[&str]) -> VerifierSet {
        let participants: Vec<(_, _)> = (0..pub_keys.len())
            .map(|i| {
                (
                    Participant {
                        address: Addr::unchecked(format!("verifier{i}")),
                        weight: nonempty::Uint128::one(),
                    },
                    multisig::key::PublicKey::Ed25519(HexBinary::from_hex(pub_keys[i]).unwrap()),
                )
            })
            .collect();
        VerifierSet::new(participants, Uint128::from(3u128), 1)
    }

    fn ton_messages() -> Vec<Message> {
        vec![Message {
            cc_id: CrossChainId::new(
                "ganache-1",
                "0xff822c88807859ff226b58e24f24974a70f04b9442501ae38fd665b3c68f3834-0",
            )
            .unwrap(),
            source_address: "0x52444f1835Adc02086c37Cb226561605e2E1699b"
                .parse()
                .unwrap(),
            destination_address:
                "0:4686a2c066c784a915f3e01c853d3195ed254c948e21adbb3e4a9b3f5f3c74d7"
                    .parse()
                    .unwrap(),
            destination_chain: "ton".parse().unwrap(),
            payload_hash: HexBinary::from_hex(
                "56570de287d73cd1cb6092bb8fdee6173974955fdef345ae579ee9f475ea7432", // keccak256("0x1234");
            )
            .unwrap()
            .to_array::<32>()
            .unwrap(),
        }]
    }

    fn ton_domain_separator() -> [u8; 32] {
        HexBinary::from_hex("6973c72935604464b28827141b0a463af8e3487616de69c5aa0c785392c9fb9f")
            .unwrap()
            .to_array()
            .unwrap()
    }

    fn hex_encode(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut output, b| {
            // write! returns a Result; we can ignore the error here
            let _ = write!(&mut output, "{:02x}", b);
            output
        })
    }
}
