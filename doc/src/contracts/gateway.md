# Gateway

> **Not used on the XRPL side.** XRPL traffic uses the [XRPL Gateway](xrpl_gateway.md), which absorbs the gateway role plus the ITS edge, the token-id registry, and gas accounting (responsibilities that on other chains live in separate contracts). The generic `gateway` contract documented here is the template deployed for every other connected chain, so a cross-chain message from XRPL to, say, XRPL-EVM still passes through XRPL-EVM's generic gateway on the destination side.

The name `gateway` used in this documentation refers to those entities which reside
on axelar chain, which can also be called internal gateways. On the other hand we have
external gateways, which are gateways deployed on external chains connected to Axelar.

The gateway contract is how messages enter the amplifier protocol.
Here are the steps taken throughout the lifecycle of
a message:
1. User sends a message to the external gateway. We call this an incoming message.
2. Incoming messages are sent to the gateway via `VerifyMessages`.
3. The gateway calls `VerifyMessages` on the verifier, which submits the messages for verification (or just returns true if already verified).
4. The messages are verified asynchronously, and the verification status is stored in the verifier.
5. Once the messages are verified, `RouteMessages` is called at the gateway, which forwards the verified messages to the router.
6. The router forwards each message to the gateway registered to the destination chain specified in the message.
7. The prover retrieves the messages from the gateway, organizes them into a payload and submits the payload for signing.
8. The relayer sends the signed payload to the external gateway.


## Gateway graph
```mermaid
flowchart TD
subgraph Axelar
    Sg{"Source Gateway"}
    Dg{"Destination Gateway"}
    Rt{"Router"}
end
Rl{"Relayer"}
Eg{"External Gateway"}

Rl -- "Listen for Message:M1 Event" --> Eg
Rl -- "M1" --> Sg
Sg -- "M1" --> Rt
Rt -- "M1" --> Dg
```


## Interface

```Rust
pub struct InstantiateMsg {
    pub verifier_address: String,
    pub router_address: String,
}

pub enum ExecuteMsg {
    // Trigger verification at the linked voting verifier for any of the given messages
    // that is still unverified. Permission: Any.
    VerifyMessages(Vec<Message>),

    // Forward the given messages to the next step of the routing layer. If the messages
    // are coming in from an external chain, they must already be verified.
    // Permission: Any.
    RouteMessages(Vec<Message>),
}

pub enum QueryMsg {
    // Messages stored for delivery to the chain corresponding to this gateway, queried
    // by the multisig prover during proof construction.
    OutgoingMessages(Vec<CrossChainId>),
}
```

The gateway only needs to know the address of the two contracts it works with: the voting verifier and the router.
