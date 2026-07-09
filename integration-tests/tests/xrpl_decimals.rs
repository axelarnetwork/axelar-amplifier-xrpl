use std::str::FromStr;

use axelar_wasm_std::msg_id::HexTxHash;
use axelar_wasm_std::nonempty;
use cosmwasm_std::{HexBinary, Uint256};
use ethers_core::utils::keccak256;
use integration_tests::contract::Contract;
use router_api::{Address, CrossChainId, Message};
use xrpl_types::msg::{WithPayload, XRPLInterchainTransferMessage, XRPLMessage};
use xrpl_types::types::{
    canonicalize_token_amount, XRPLAccountId, XRPLCurrency, XRPLPaymentAmount, XRPLToken,
    XRPLUnsignedTx,
};

pub mod test_utils;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct StoredTxInfo {
    status: xrpl_types::types::XRPLTxStatus,
    unsigned_tx: XRPLUnsignedTx,
    original_cc_id: Option<CrossChainId>,
}

fn transfer_message(
    token_id: interchain_token_service::TokenId,
    source_address: &Address,
    destination_address: &XRPLAccountId,
    amount: nonempty::Uint256,
) -> interchain_token_service::Message {
    interchain_token_service::Message::InterchainTransfer(
        interchain_token_service::InterchainTransfer {
            token_id,
            source_address: nonempty::HexBinary::try_from(HexBinary::from(
                source_address.as_bytes(),
            ))
            .unwrap(),
            destination_address: nonempty::HexBinary::try_from(HexBinary::from(
                destination_address.to_string().as_bytes(),
            ))
            .unwrap(),
            amount,
            data: None,
        },
    )
}

#[test]
fn interchain_transfer_towards_xrpl_uses_xrpl_decimals_not_source_decimals() {
    let test_utils::XRPLDestinationTestCase {
        mut protocol,
        source_chain,
        axelarnet,
        its_hub,
        xrpl,
        verifiers,
        ..
    } = test_utils::setup_xrpl_destination_test_case();

    let token_id = interchain_token_service::TokenId::new([7u8; 32]);
    assert_ne!(token_id, xrpl.xrp_token_id);
    let xrpl_currency: XRPLCurrency = "USD".to_string().try_into().unwrap();

    let res = xrpl.gateway.execute(
        &mut protocol.app,
        xrpl.admin.clone(),
        &xrpl_gateway::msg::ExecuteMsg::RegisterRemoteToken {
            token_id,
            xrpl_currency,
        },
    );
    assert!(res.is_ok(), "register remote token: {:?}", res);

    test_utils::override_token_supply_for_chain(
        &mut protocol.app,
        its_hub.contract_addr.clone(),
        source_chain.chain_name.clone(),
        token_id,
        interchain_token_service::TokenSupply::Tracked(Uint256::MAX),
        6,
    );
    test_utils::override_token_supply_for_chain(
        &mut protocol.app,
        its_hub.contract_addr.clone(),
        xrpl.chain_name.clone(),
        token_id,
        interchain_token_service::TokenSupply::Tracked(Uint256::zero()),
        18,
    );

    let source_address: Address = "0x95181d16cfb23Bc493668C17d973F061e30F2EAF"
        .to_string()
        .try_into()
        .unwrap();
    let destination_address: XRPLAccountId =
        XRPLAccountId::from_str("raNVNWvhUQzFkDDTdEw3roXRJfMJFVJuQo").unwrap();

    let source_amount_raw: u128 = 1_000_000;
    let scaled_amount_raw: u128 = 1_000_000_000_000_000_000;
    let source_amount = nonempty::Uint256::try_from(Uint256::from(source_amount_raw)).unwrap();
    let scaled_amount = nonempty::Uint256::try_from(Uint256::from(scaled_amount_raw)).unwrap();

    let send_to_hub = interchain_token_service::HubMessage::SendToHub {
        message: transfer_message(
            token_id,
            &source_address,
            &destination_address,
            source_amount,
        ),
        destination_chain: xrpl.chain_name.clone().into(),
    }
    .abi_encode();

    let wrapped_msg = Message {
        cc_id: CrossChainId {
            source_chain: source_chain.chain_name.clone().into(),
            message_id: "0xaff42a67c474758ce97bd9b69c395c6dc6019707b400e06c30b0878a9357b2ea-7"
                .to_string()
                .try_into()
                .unwrap(),
        },
        source_address: source_chain.its_address.clone(),
        destination_address: Address::try_from(its_hub.contract_addr.to_string()).unwrap(),
        destination_chain: axelarnet.chain_name.clone(),
        payload_hash: keccak256(send_to_hub.clone()),
    };
    let wrapped_msgs = vec![wrapped_msg.clone()];

    let (poll_id, expiry) =
        test_utils::verify_messages(&mut protocol.app, &source_chain.gateway, &wrapped_msgs);
    test_utils::vote_success(
        &mut protocol.app,
        &source_chain.voting_verifier,
        wrapped_msgs.len(),
        &verifiers,
        poll_id,
    );
    test_utils::advance_at_least_to_height(&mut protocol.app, expiry);
    test_utils::end_poll(&mut protocol.app, &source_chain.voting_verifier, poll_id);
    test_utils::route_messages(&mut protocol.app, &source_chain.gateway, &wrapped_msgs);

    let its_hub_msg_id = test_utils::execute_axelarnet_gateway_message(
        &mut protocol.app,
        &axelarnet.gateway,
        wrapped_msg.cc_id.clone(),
        send_to_hub,
    );
    test_utils::route_axelarnet_gateway_messages(&mut protocol, &axelarnet.gateway, wrapped_msgs);

    let its_hub_msg_ids =
        vec![CrossChainId::new(axelarnet.chain_name.clone(), its_hub_msg_id).unwrap()];
    let routable_msgs = test_utils::routable_messages_from_axelarnet_gateway(
        &mut protocol.app,
        &axelarnet.gateway,
        &its_hub_msg_ids,
    );
    assert_eq!(routable_msgs.len(), 1);

    let receive_from_hub = interchain_token_service::HubMessage::ReceiveFromHub {
        source_chain: source_chain.chain_name.clone().into(),
        message: transfer_message(
            token_id,
            &source_address,
            &destination_address,
            scaled_amount,
        ),
    }
    .abi_encode();

    let session_id = test_utils::construct_xrpl_payment_proof_and_sign(
        &mut protocol,
        &xrpl.multisig_prover,
        routable_msgs.first().unwrap().clone(),
        &verifiers,
        receive_from_hub,
    );

    let proof = test_utils::xrpl_proof(&mut protocol.app, &xrpl.multisig_prover, &session_id);
    assert!(matches!(
        proof.status,
        xrpl_multisig_prover::msg::ProofStatus::Completed { .. }
    ));

    const TX_INFO: cw_storage_plus::Map<&axelar_wasm_std::hash::Hash, StoredTxInfo> =
        cw_storage_plus::Map::new("unsigned_tx_hash_to_tx_info");
    let storage = protocol
        .app
        .contract_storage(&xrpl.multisig_prover.contract_addr);
    let stored = TX_INFO
        .load(&*storage, &proof.unsigned_tx_hash.tx_hash)
        .unwrap();
    drop(storage);

    let amount = match stored.unsigned_tx {
        XRPLUnsignedTx::Payment(payment) => payment.amount,
        other => panic!("expected a Payment tx, got {:?}", other),
    };

    match amount {
        XRPLPaymentAmount::Issued(_token, token_amount) => {
            assert_eq!(
                token_amount,
                canonicalize_token_amount(Uint256::from(scaled_amount_raw), 18).unwrap(),
                "issued amount must be canonicalized with XRPL decimals"
            );
            assert_ne!(
                token_amount,
                canonicalize_token_amount(Uint256::from(scaled_amount_raw), 6).unwrap(),
                "issued amount uses XRPL decimals"
            );
        }
        other => panic!("expected an issued IOU amount, got {:?}", other),
    }
}

#[test]
fn round_trip_xrpl_source_xrpl_preserves_balance() {
    let test_utils::XRPLDestinationTestCase {
        mut protocol,
        source_chain,
        axelarnet,
        its_hub,
        xrpl,
        verifiers,
        ..
    } = test_utils::setup_xrpl_destination_test_case();

    let token_id = interchain_token_service::TokenId::new([8u8; 32]);
    assert_ne!(token_id, xrpl.xrp_token_id);
    let xrpl_currency: XRPLCurrency = "USD".to_string().try_into().unwrap();

    let res = xrpl.gateway.execute(
        &mut protocol.app,
        xrpl.admin.clone(),
        &xrpl_gateway::msg::ExecuteMsg::RegisterRemoteToken {
            token_id,
            xrpl_currency,
        },
    );
    assert!(res.is_ok(), "register remote token: {:?}", res);

    let source_supply0 = Uint256::from(1_000_000_000u128);
    test_utils::override_token_supply_for_chain(
        &mut protocol.app,
        its_hub.contract_addr.clone(),
        source_chain.chain_name.clone(),
        token_id,
        interchain_token_service::TokenSupply::Tracked(source_supply0),
        6,
    );
    test_utils::override_token_supply_for_chain(
        &mut protocol.app,
        its_hub.contract_addr.clone(),
        xrpl.chain_name.clone(),
        token_id,
        interchain_token_service::TokenSupply::Tracked(Uint256::zero()),
        18,
    );

    let source_user: Address = "0x95181d16cfb23Bc493668C17d973F061e30F2EAF"
        .to_string()
        .try_into()
        .unwrap();
    let source_user_hex = "95181d16cfb23Bc493668C17d973F061e30F2EAF";
    let xrpl_user: XRPLAccountId =
        XRPLAccountId::from_str("raNVNWvhUQzFkDDTdEw3roXRJfMJFVJuQo").unwrap();

    let amount_source: u128 = 1_000_000;
    let amount_xrpl: u128 = 1_000_000_000_000_000_000;

    let inbound_payload = interchain_token_service::HubMessage::SendToHub {
        message: transfer_message(
            token_id,
            &source_user,
            &xrpl_user,
            nonempty::Uint256::try_from(Uint256::from(amount_source)).unwrap(),
        ),
        destination_chain: xrpl.chain_name.clone().into(),
    }
    .abi_encode();
    let inbound_msg = Message {
        cc_id: CrossChainId {
            source_chain: source_chain.chain_name.clone().into(),
            message_id: "0xaff42a67c474758ce97bd9b69c395c6dc6019707b400e06c30b0878a9357b2ea-1"
                .to_string()
                .try_into()
                .unwrap(),
        },
        source_address: source_chain.its_address.clone(),
        destination_address: Address::try_from(its_hub.contract_addr.to_string()).unwrap(),
        destination_chain: axelarnet.chain_name.clone(),
        payload_hash: keccak256(inbound_payload.clone()),
    };
    let inbound_msgs = vec![inbound_msg.clone()];
    let (poll_id, expiry) =
        test_utils::verify_messages(&mut protocol.app, &source_chain.gateway, &inbound_msgs);
    test_utils::vote_success(
        &mut protocol.app,
        &source_chain.voting_verifier,
        1,
        &verifiers,
        poll_id,
    );
    test_utils::advance_at_least_to_height(&mut protocol.app, expiry);
    test_utils::end_poll(&mut protocol.app, &source_chain.voting_verifier, poll_id);
    test_utils::route_messages(&mut protocol.app, &source_chain.gateway, &inbound_msgs);
    test_utils::execute_axelarnet_gateway_message(
        &mut protocol.app,
        &axelarnet.gateway,
        inbound_msg.cc_id.clone(),
        inbound_payload,
    );

    let xrpl_supply_after_inbound: interchain_token_service::TokenSupply = {
        let inst: Option<interchain_token_service::TokenInstance> = protocol
            .app
            .wrap()
            .query_wasm_smart(
                its_hub.contract_addr.clone(),
                &interchain_token_service::msg::QueryMsg::TokenInstance {
                    chain: xrpl.chain_name.clone().into(),
                    token_id,
                },
            )
            .unwrap();
        inst.unwrap().supply
    };
    let source_supply_after_inbound: interchain_token_service::TokenSupply = {
        let inst: Option<interchain_token_service::TokenInstance> = protocol
            .app
            .wrap()
            .query_wasm_smart(
                its_hub.contract_addr.clone(),
                &interchain_token_service::msg::QueryMsg::TokenInstance {
                    chain: source_chain.chain_name.clone().into(),
                    token_id,
                },
            )
            .unwrap();
        inst.unwrap().supply
    };

    let iou = canonicalize_token_amount(Uint256::from(amount_xrpl), 18).unwrap();
    let xrpl_token: XRPLToken = xrpl
        .gateway
        .query(
            &protocol.app,
            &xrpl_gateway::msg::QueryMsg::XrplToken(token_id),
        )
        .unwrap();
    let tx_id = HexTxHash::new([2u8; 32]);

    let xrpl_transfer = XRPLInterchainTransferMessage {
        tx_id: tx_id.clone(),
        source_address: xrpl_user.clone(),
        destination_chain: source_chain.chain_name.clone().into(),
        destination_address: nonempty::String::try_from(source_user_hex).unwrap(),
        payload_hash: None,
        transfer_amount: XRPLPaymentAmount::Issued(xrpl_token, iou),
        gas_fee_amount: XRPLPaymentAmount::Drops(10),
    };
    let xrpl_msg = XRPLMessage::InterchainTransferMessage(xrpl_transfer.clone());
    let xrpl_msg_id = xrpl_transfer.cc_id(xrpl.chain_name.clone().into());

    let outbound_its = interchain_token_service::Message::InterchainTransfer(
        interchain_token_service::InterchainTransfer {
            token_id,
            source_address: nonempty::HexBinary::try_from(HexBinary::from(
                xrpl_user.to_string().as_bytes(),
            ))
            .unwrap(),
            destination_address: nonempty::HexBinary::try_from(
                HexBinary::from_hex(source_user_hex).unwrap(),
            )
            .unwrap(),
            amount: nonempty::Uint256::try_from(Uint256::from(amount_xrpl)).unwrap(),
            data: None,
        },
    );
    let outbound_payload = interchain_token_service::HubMessage::SendToHub {
        message: outbound_its,
        destination_chain: source_chain.chain_name.clone().into(),
    }
    .abi_encode();
    let outbound_msg = Message {
        cc_id: CrossChainId {
            source_chain: xrpl.chain_name.clone().into(),
            message_id: tx_id.to_string().try_into().unwrap(),
        },
        source_address: Address::from_str(&xrpl.its_address).unwrap(),
        destination_address: Address::try_from(its_hub.contract_addr.to_string()).unwrap(),
        destination_chain: axelarnet.chain_name.clone(),
        payload_hash: keccak256(outbound_payload.clone()),
    };

    let (poll_id, expiry) =
        test_utils::verify_xrpl_messages(&mut protocol.app, &xrpl.gateway, &vec![xrpl_msg.clone()]);
    test_utils::vote_success(
        &mut protocol.app,
        &xrpl.voting_verifier,
        1,
        &verifiers,
        poll_id,
    );
    test_utils::advance_at_least_to_height(&mut protocol.app, expiry);
    test_utils::end_poll(&mut protocol.app, &xrpl.voting_verifier, poll_id);
    test_utils::xrpl_route_incoming_messages(
        &mut protocol.app,
        &xrpl.gateway,
        &vec![WithPayload::new(xrpl_msg, None)],
    );

    let executable = test_utils::executable_messages_from_axelarnet_gateway(
        &mut protocol.app,
        &axelarnet.gateway,
        &[xrpl_msg_id],
    );
    assert_eq!(executable.len(), 1);
    match executable.first().unwrap() {
        axelarnet_gateway::ExecutableMessage::Approved(approved) => {
            assert_eq!(
                *approved, outbound_msg,
                "gateway routed an unexpected outbound amount"
            );
        }
        other => panic!("expected an approved message, got {:?}", other),
    }

    test_utils::execute_axelarnet_gateway_message(
        &mut protocol.app,
        &axelarnet.gateway,
        outbound_msg.cc_id.clone(),
        outbound_payload,
    );

    let return_payload = interchain_token_service::HubMessage::SendToHub {
        message: transfer_message(
            token_id,
            &source_user,
            &xrpl_user,
            nonempty::Uint256::try_from(Uint256::from(amount_source)).unwrap(),
        ),
        destination_chain: xrpl.chain_name.clone().into(),
    }
    .abi_encode();
    let return_msg = Message {
        cc_id: CrossChainId {
            source_chain: source_chain.chain_name.clone().into(),
            message_id: "0xaff42a67c474758ce97bd9b69c395c6dc6019707b400e06c30b0878a9357b2ea-2"
                .to_string()
                .try_into()
                .unwrap(),
        },
        source_address: source_chain.its_address.clone(),
        destination_address: Address::try_from(its_hub.contract_addr.to_string()).unwrap(),
        destination_chain: axelarnet.chain_name.clone(),
        payload_hash: keccak256(return_payload.clone()),
    };
    let return_msgs = vec![return_msg.clone()];
    let (poll_id, expiry) =
        test_utils::verify_messages(&mut protocol.app, &source_chain.gateway, &return_msgs);
    test_utils::vote_success(
        &mut protocol.app,
        &source_chain.voting_verifier,
        1,
        &verifiers,
        poll_id,
    );
    test_utils::advance_at_least_to_height(&mut protocol.app, expiry);
    test_utils::end_poll(&mut protocol.app, &source_chain.voting_verifier, poll_id);
    test_utils::route_messages(&mut protocol.app, &source_chain.gateway, &return_msgs);
    test_utils::execute_axelarnet_gateway_message(
        &mut protocol.app,
        &axelarnet.gateway,
        return_msg.cc_id.clone(),
        return_payload,
    );

    let source_instance: Option<interchain_token_service::TokenInstance> = protocol
        .app
        .wrap()
        .query_wasm_smart(
            its_hub.contract_addr.clone(),
            &interchain_token_service::msg::QueryMsg::TokenInstance {
                chain: source_chain.chain_name.clone().into(),
                token_id,
            },
        )
        .unwrap();
    let xrpl_instance: Option<interchain_token_service::TokenInstance> = protocol
        .app
        .wrap()
        .query_wasm_smart(
            its_hub.contract_addr.clone(),
            &interchain_token_service::msg::QueryMsg::TokenInstance {
                chain: xrpl.chain_name.clone().into(),
                token_id,
            },
        )
        .unwrap();

    assert_eq!(
        xrpl_instance.unwrap().supply,
        xrpl_supply_after_inbound,
        "XRPL balance not restored after the XRPL -> source -> XRPL round trip"
    );
    assert_eq!(
        source_instance.unwrap().supply,
        source_supply_after_inbound,
        "source-chain balance not restored after the XRPL -> source -> XRPL round trip"
    );
    assert_eq!(
        xrpl_supply_after_inbound,
        interchain_token_service::TokenSupply::Tracked(Uint256::from(amount_xrpl))
    );
    assert_eq!(
        source_supply_after_inbound,
        interchain_token_service::TokenSupply::Tracked(
            source_supply0 - Uint256::from(amount_source)
        )
    );
}
