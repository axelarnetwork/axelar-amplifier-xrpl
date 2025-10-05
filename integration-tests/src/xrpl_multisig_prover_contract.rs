use axelar_core_std::query::AxelarQueryMsg;
use axelar_wasm_std::Threshold;
use cosmwasm_std::{instantiate2_address, Addr, Api, DepsMut, Env};
use cw_multi_test::{ContractWrapper, Executor};
use router_api::ChainName;
use xrpl_multisig_prover::contract::{execute, instantiate, query};
use xrpl_types::types::XRPLAccountId;

use crate::contract::Contract;
use crate::protocol::{emptying_deps_mut, Protocol};

#[derive(Clone)]
pub struct XRPLMultisigProverContract {
    pub contract_addr: Addr,
    pub admin_addr: Addr,
}

impl XRPLMultisigProverContract {
    pub fn store_code(protocol: &mut Protocol, creator: Addr) -> u64 {
        let code =
            ContractWrapper::new_with_empty(execute, instantiate, query).with_reply(custom_reply);
        protocol
            .app
            .store_code_with_creator(creator, Box::new(code))
    }

    pub fn predict_instantiate2_address(
        protocol: &Protocol,
        code_id: u64,
        creator: &Addr,
        salt: &[u8],
    ) -> Addr {
        let code_info = protocol.app.wrap().query_wasm_code_info(code_id).unwrap();
        let canonical_creator = protocol
            .app
            .api()
            .addr_canonicalize(creator.as_str())
            .unwrap();
        let canonical_addr =
            instantiate2_address(code_info.checksum.as_slice(), &canonical_creator, salt).unwrap();

        protocol.app.api().addr_humanize(&canonical_addr).unwrap()
    }

    pub fn instantiate_contract(
        protocol: &mut Protocol,
        admin_address: Addr,
        gateway_address: Addr,
        voting_verifier_address: Addr,
        xrpl_chain_name: ChainName,
        xrpl_multisig_address: XRPLAccountId,
        relayer_address: XRPLAccountId,
    ) -> Self {
        let creator = Addr::unchecked("anyone");
        let code_id = Self::store_code(protocol, creator);
        let app = &mut protocol.app;

        let msg = xrpl_multisig_prover::msg::InstantiateMsg {
            admin_address: admin_address.to_string(),
            governance_address: protocol.governance_address.to_string(),
            gateway_address: gateway_address.to_string(),
            multisig_address: protocol.multisig.contract_addr.to_string(),
            coordinator_address: protocol.coordinator.contract_addr.to_string(),
            service_registry_address: protocol.service_registry.contract_addr.to_string(),
            voting_verifier_address: voting_verifier_address.to_string(),
            signing_threshold: Threshold::try_from((2, 3)).unwrap().try_into().unwrap(),
            service_name: protocol.service_name.to_string(),
            chain_name: xrpl_chain_name,
            xrpl_multisig_address,
            verifier_set_diff_threshold: 0,
            xrpl_transaction_fee: 10,
            xrpl_base_reserve: 1000000,
            xrpl_owner_reserve: 200000,
            initial_fee_reserve: 60000000,
            ticket_count_threshold: 1,
            next_sequence_number: 44218446,
            last_assigned_ticket_number: 44218195,
            available_tickets: [vec![], (44218195..44218200).collect::<Vec<_>>()].concat(),
            relayer_address,
        };

        let contract_addr = app
            .instantiate_contract(
                code_id,
                Addr::unchecked("anyone"),
                &msg,
                &[],
                "xrpl_multisig_prover",
                None,
            )
            .unwrap();

        XRPLMultisigProverContract {
            contract_addr,
            admin_addr: admin_address,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn instantiate2_contract(
        protocol: &mut Protocol,
        code_id: u64,
        creator: Addr,
        salt: &[u8],
        admin_address: Addr,
        gateway_address: Addr,
        voting_verifier_address: Addr,
        xrpl_chain_name: ChainName,
        xrpl_multisig_address: XRPLAccountId,
        relayer_address: XRPLAccountId,
    ) -> Self {
        let app = &mut protocol.app;

        let msg = xrpl_multisig_prover::msg::InstantiateMsg {
            admin_address: admin_address.to_string(),
            governance_address: protocol.governance_address.to_string(),
            gateway_address: gateway_address.to_string(),
            multisig_address: protocol.multisig.contract_addr.to_string(),
            coordinator_address: protocol.coordinator.contract_addr.to_string(),
            service_registry_address: protocol.service_registry.contract_addr.to_string(),
            voting_verifier_address: voting_verifier_address.to_string(),
            signing_threshold: Threshold::try_from((2, 3)).unwrap().try_into().unwrap(),
            service_name: protocol.service_name.to_string(),
            chain_name: xrpl_chain_name,
            xrpl_multisig_address,
            verifier_set_diff_threshold: 0,
            xrpl_transaction_fee: 10,
            xrpl_base_reserve: 1000000,
            xrpl_owner_reserve: 200000,
            initial_fee_reserve: 60000000,
            ticket_count_threshold: 1,
            next_sequence_number: 44218446,
            last_assigned_ticket_number: 44218195,
            available_tickets: [vec![], (44218195..44218200).collect::<Vec<_>>()].concat(),
            relayer_address,
        };

        let contract_addr = app
            .instantiate2_contract(
                code_id,
                creator,
                &msg,
                &[],
                "xrpl_multisig_prover",
                admin_address.to_string(),
                salt,
            )
            .unwrap();

        XRPLMultisigProverContract {
            contract_addr,
            admin_addr: admin_address,
        }
    }
}

fn custom_reply(
    mut deps: DepsMut<AxelarQueryMsg>,
    env: Env,
    msg: cosmwasm_std::Reply,
) -> Result<cosmwasm_std::Response, axelar_wasm_std::error::ContractError> {
    xrpl_multisig_prover::contract::reply(emptying_deps_mut(&mut deps), env, msg)
}

impl Contract for XRPLMultisigProverContract {
    type QMsg = xrpl_multisig_prover::msg::QueryMsg;
    type ExMsg = xrpl_multisig_prover::msg::ExecuteMsg;

    fn contract_address(&self) -> Addr {
        self.contract_addr.clone()
    }
}
