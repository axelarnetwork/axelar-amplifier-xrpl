use std::convert::TryInto;

use async_trait::async_trait;
use axelar_wasm_std::msg_id::HexTxHash;
use axelar_wasm_std::voting::{PollId, Vote};
use cosmrs::cosmwasm::MsgExecuteContract;
use cosmrs::tx::Msg;
use cosmrs::Any;
use error_stack::ResultExt;
use events::Error::EventTypeMismatch;
use events_derive::try_from;
use router_api::ChainName;
use serde::Deserialize;
use tokio::sync::watch::Receiver;
use tonlib_core::TonAddress;
use tracing::{info, info_span};
use valuable::Valuable;
use voting_verifier::msg::ExecuteMsg;

use crate::event_processor::EventHandler;
use crate::handlers::errors::Error;
use crate::handlers::errors::Error::DeserializeEvent;
use crate::types::{Hash, TMAddress};

type Result<T> = error_stack::Result<T, Error>;

mod hex_tx_hash_string {
    use std::str::FromStr;

    use axelar_wasm_std::msg_id::HexTxHash;
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HexTxHash, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        HexTxHash::from_str(&string).map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize, Debug, PartialEq)]
pub struct Message {
    #[serde(with = "hex_tx_hash_string")]
    pub message_id: HexTxHash,
    pub destination_address: String,
    pub destination_chain: ChainName,
    pub source_address: TonAddress,
    pub payload_hash: Hash,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
#[try_from("wasm-messages_poll_started")]
struct PollStartedEvent {
    poll_id: PollId,
    source_chain: ChainName,
    source_gateway_address: TonAddress,
    confirmation_height: u64,
    expires_at: u64,
    messages: Vec<Message>,
    participants: Vec<TMAddress>,
}

use thiserror::Error;

use super::ton_verify_verifier_set::VerifierSetConfirmation;

#[derive(Error, Debug)]
pub enum FetchingError {
    #[error("failed to create client")]
    Client,
    #[error("invalid call")]
    InvalidCall,
    #[error("transaction not found on chain")]
    NotFound,
}

#[async_trait::async_trait]
pub trait TonClient: Send + Sync + 'static {
    async fn get_tx(
        &self,
        tx_hash: &HexTxHash,
        gateway: &TonAddress,
    ) -> error_stack::Result<Message, FetchingError>;

    async fn verify_verifier_set(
        &self,
        verifier_set_confirmation: &VerifierSetConfirmation,
        gateway: &TonAddress,
    ) -> error_stack::Result<bool, FetchingError>;
}

pub struct Handler<C>
where
    C: TonClient,
{
    verifier: TMAddress,
    voting_verifier_contract: TMAddress,
    rpc_client: C,
    latest_block_height: Receiver<u64>,
}

impl<C> Handler<C>
where
    C: TonClient + Send + Sync,
{
    pub fn new(
        verifier: TMAddress,
        voting_verifier_contract: TMAddress,
        rpc_client: C,
        latest_block_height: Receiver<u64>,
    ) -> Self {
        Self {
            verifier,
            voting_verifier_contract,
            rpc_client,
            latest_block_height,
        }
    }

    async fn verify_tx(&self, claimed_message: &Message, gateway: &TonAddress) -> bool {
        match self
            .rpc_client
            .get_tx(&claimed_message.message_id, gateway)
            .await
        {
            Ok(res) => {
                if res == *claimed_message {
                    true
                } else {
                    info!(
                        "Real message {:?} not identical to claimed message {:?}",
                        res, claimed_message
                    );
                    false
                }
            }
            Err(_) => false,
        }
    }

    fn vote_msg(&self, poll_id: PollId, votes: Vec<Vote>) -> MsgExecuteContract {
        MsgExecuteContract {
            sender: self.verifier.as_ref().clone(),
            contract: self.voting_verifier_contract.as_ref().clone(),
            msg: serde_json::to_vec(&ExecuteMsg::Vote { poll_id, votes })
                .expect("vote msg should serialize"),
            funds: vec![],
        }
    }
}

#[async_trait]
impl<C> EventHandler for Handler<C>
where
    C: TonClient + Send + Sync,
{
    type Err = Error;

    async fn handle(&self, event: &events::Event) -> Result<Vec<Any>> {
        if !event.is_from_contract(self.voting_verifier_contract.as_ref()) {
            return Ok(vec![]);
        }

        let PollStartedEvent {
            poll_id,
            source_chain,
            source_gateway_address,
            messages,
            expires_at,
            confirmation_height: _,
            participants,
        } = match event.try_into() as error_stack::Result<_, _> {
            Err(report) if matches!(report.current_context(), EventTypeMismatch(_)) => {
                return Ok(vec![])
            }
            event => event.change_context(DeserializeEvent)?,
        };

        if !participants.contains(&self.verifier) {
            return Ok(vec![]);
        }

        let latest_block_height = *self.latest_block_height.borrow();
        if latest_block_height >= expires_at {
            info!(poll_id = poll_id.to_string(), "skipping expired poll");
            return Ok(vec![]);
        }

        let poll_id_str: String = poll_id.into();
        let source_chain_str: String = source_chain.into();

        let votes = info_span!(
            "verify messages from Ton",
            poll_id = poll_id_str,
            source_chain = source_chain_str,
            message_ids = messages
                .iter()
                .map(|msg| msg.message_id.to_string())
                .collect::<Vec<String>>()
                .as_value(),
        )
        .in_scope(|| async {
            info!("ready to verify messages in poll",);

            let mut votes = Vec::new();

            for m in messages.iter() {
                let success = self.verify_tx(m, &source_gateway_address).await;
                let vote = if success {
                    Vote::SucceededOnChain
                } else {
                    Vote::NotFound
                };
                votes.push(vote);
            }
            info!(
                votes = votes.as_value(),
                "ready to vote for messages in poll"
            );

            votes
        });

        Ok(vec![self
            .vote_msg(poll_id, votes.await)
            .into_any()
            .expect("vote msg should serialize")])
    }
}
