# Access Control for Contract Messages

Each execute message in the Amplifier contracts is annotated with a permission scope. The vocabulary is defined by the
`msgs-derive` macro and a message can have any of the following:

- **`Any`**: anyone can call it. Default for user-facing methods.
- **`Governance`**: only the configured governance address.
- **`Admin`**: only the configured admin address.
- **`Elevated`**: admin **or** governance.
- **`Specific(role)`**: only an address that holds the named role. Common variants in this codebase: `Specific(gateway)`,
  `Specific(verifier)`, `Specific(prover)`, `Specific(authorized)`.
- **`Proxy(role)`**: a single trusted contract may also call this on behalf of the named role. Used today for
  `Proxy(coordinator)` so the coordinator can register chains/callers on behalf of governance.

Only execute messages with restricted access are listed below. Messages tagged `Any` are omitted.

## Router

### Governance
```rust
RegisterChain {
    chain: ChainName,
    gateway_address: Address,
    msg_id_format: MessageIdFormat,
},

UpgradeGateway {
    chain: ChainName,
    contract_address: Address,
},
```

### Elevated
```rust
FreezeChains {
    chains: HashMap<ChainName, GatewayDirection>,
},

UnfreezeChains {
    chains: HashMap<ChainName, GatewayDirection>,
},

DisableRouting,

EnableRouting,
```

### Specific(gateway)
Only a registered chain gateway may call this:
```rust
RouteMessages(Vec<Message>)
```

## Voting Verifier

### Governance
```rust
UpdateVotingThreshold {
    new_voting_threshold: MajorityThreshold,
}
```

## Multisig Prover

### Governance
```rust
UpdateSigningThreshold {
    new_signing_threshold: MajorityThreshold,
},

UpdateAdmin {
    new_admin_address: String,
}
```

### Elevated
```rust
UpdateVerifierSet
```

## Multisig

### Governance
```rust
AuthorizeCallers {
    contracts: HashMap<String, ChainName>,
}
```

### Elevated
```rust
UnauthorizeCallers {
    contracts: HashMap<String, ChainName>,
},

DisableSigning,

EnableSigning,
```

### Specific(authorized)
Only a contract previously authorized via `AuthorizeCallers` may call this, and only for the chain name it was
authorized for:
```rust
StartSigningSession {
    verifier_set_id: String,
    msg: HexBinary,
    chain_name: ChainName,
    sig_verifier: Option<String>,
}
```

## Service Registry

### Governance
```rust
RegisterService {
    service_name: String,
    coordinator_contract: String,
    min_num_verifiers: u16,
    max_num_verifiers: Option<u16>,
    min_verifier_bond: nonempty::Uint128,
    bond_denom: String,
    unbonding_period_days: u16,
    description: String,
},

UpdateService {
    service_name: String,
    updated_service_params: UpdatedServiceParams,
},

AuthorizeVerifiers {
    verifiers: Vec<String>,
    service_name: String,
},

UnauthorizeVerifiers {
    verifiers: Vec<String>,
    service_name: String,
},

JailVerifiers {
    verifiers: Vec<String>,
    service_name: String,
}
```

### Specific(verifier)
Called by the verifier address itself:
```rust
RegisterChainSupport {
    service_name: String,
    chains: Vec<ChainName>,
},

DeregisterChainSupport {
    service_name: String,
    chains: Vec<ChainName>,
}
```

## Coordinator

### Governance
```rust
RegisterProverContract {
    chain_name: ChainName,
    new_prover_addr: String,
}
```

### Specific(prover)
Only a prover address that was previously registered via `RegisterProverContract` may call this, and only with respect
to the chain it was registered for:
```rust
SetActiveVerifiers {
    verifiers: HashSet<String>,
}
```

## Rewards

### Governance
```rust
UpdatePoolParams { params: Params, pool_id: PoolId },

CreatePool { params: Params, pool_id: PoolId },
```
