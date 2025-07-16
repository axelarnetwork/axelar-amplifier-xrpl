use axelar_wasm_std::MajorityThreshold;
use cosmwasm_std::testing::MockApi;
use cosmwasm_std::Addr;
use cw_multi_test::{ContractWrapper, Executor};
use router_api::ChainName;
use voting_verifier::contract::{execute, instantiate, query};

use crate::contract::Contract;
use crate::protocol::Protocol;

#[derive(Clone)]
pub struct VotingVerifierContract {
    pub contract_addr: Addr,
    pub code_id: u64,
}

impl VotingVerifierContract {
    pub fn instantiate_contract(
        protocol: &mut Protocol,
        voting_threshold: MajorityThreshold,
        source_chain: ChainName,
    ) -> Self {
        let code = ContractWrapper::new_with_empty(execute, instantiate, query);
        let app = &mut protocol.app;
        let code_id = app.store_code(Box::new(code));

        let contract_addr = app
            .instantiate_contract(
                code_id,
                MockApi::default().addr_make("anyone"),
                &voting_verifier::msg::InstantiateMsg {
                    governance_address: protocol.governance_address.to_string().try_into().unwrap(),
                    service_registry_address: protocol
                        .service_registry
                        .contract_addr
                        .to_string()
                        .try_into()
                        .unwrap(),
                    service_name: protocol.service_name.clone(),
                    source_gateway_address:
                        "0:00194aad8e422bedf43fee746d6d929d369dbab25468a69d513706ea6978b63a"
                            .try_into()
                            .unwrap(),
                    voting_threshold,
                    block_expiry: 10.try_into().unwrap(),
                    confirmation_height: 5,
                    source_chain,
                    rewards_address: protocol
                        .rewards
                        .contract_addr
                        .to_string()
                        .try_into()
                        .unwrap(),
                    msg_id_format: axelar_wasm_std::msg_id::MessageIdFormat::HexTxHash,
                    address_format: axelar_wasm_std::address::AddressFormat::Ton,
                },
                &[],
                "voting_verifier",
                None,
            )
            .unwrap();

        VotingVerifierContract {
            contract_addr,
            code_id,
        }
    }
}

impl Default for VotingVerifierContract {
    fn default() -> Self {
        VotingVerifierContract {
            contract_addr: MockApi::default().addr_make("verifier"),
            code_id: 0,
        }
    }
}

impl Contract for VotingVerifierContract {
    type QMsg = voting_verifier::msg::QueryMsg;
    type ExMsg = voting_verifier::msg::ExecuteMsg;

    fn contract_address(&self) -> Addr {
        self.contract_addr.clone()
    }
}
