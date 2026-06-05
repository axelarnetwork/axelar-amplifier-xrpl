---
title: Firewalling and Flow Limits
sidebar_position: 5
---

# Firewalling and Flow Limits

The Axelar Amplifier protocol's hub-and-spoke topology is the basis of a security property called **firewalling**: a single compromised chain cannot exfiltrate more of a token from the bridged system than has legitimately been deposited into it. This page explains how the property is enforced and when it applies.

## The hub-and-spoke topology

Every ITS-routed cross-chain transfer passes through the [ITS Hub](https://github.com/axelarnetwork/axelar-amplifier/tree/main/contracts/interchain-token-service) on Axelar. No two connected chains exchange tokens directly: a transfer from chain A to chain B is always two GMP legs glued together at the Hub:

```
A's ITS edge  →  axelarnet-gateway  →  ITS Hub  →  axelarnet-gateway  →  B's ITS edge
              ────── leg 1 ──────                 ────── leg 2 ──────
```

This shape means the Hub sees every interchain transfer in the ecosystem, exactly once. That central observation point is what lets the Hub enforce a global invariant per `(chain, tokenId)`.

## The Hub supply invariant

Per `(chain, tokenId)` the Hub stores a `TokenInstance` ([`contracts/interchain-token-service/src/state.rs`](https://github.com/axelarnetwork/axelar-amplifier/blob/main/contracts/interchain-token-service/src/state.rs)):

```rust
pub struct TokenInstance {
    pub supply: TokenSupply,
    pub decimals: u8,
}

pub enum TokenSupply {
    /// The total token supply bridged to this chain.
    /// ITS Hub will not allow bridging back more than this amount of the
    /// token from the corresponding chain.
    Tracked(Uint256),
    Untracked,
}
```

On every `InterchainTransfer` the Hub runs `apply_to_transfer`:

```rust
interceptors::subtract_supply_amount(storage, &source_chain, &transfer)?;
let transfer = interceptors::apply_scaling_factor_to_amount(...)?;
interceptors::add_supply_amount(storage, &destination_chain, &transfer)?;
```

The `subtract_supply_amount` interceptor calls `supply.checked_sub(amount)` on the source chain's `TokenInstance`. If `Tracked` and the subtraction would underflow, the Hub raises:

```rust
#[error("token supply invariant violated for token {token_id} on chain {chain}")]
TokenSupplyInvariantViolated { token_id: TokenId, chain: ChainNameRaw },
```

and the entire transfer reverts. If `Untracked`, the subtraction is a no-op.

The invariant maintained per `(chain, token)` when `Tracked` is therefore:

> Total bridged INTO the chain − Total bridged OUT of the chain  ≥  0

Equivalently: **the Hub will not let more of a token leave a chain via ITS than has been bridged in via ITS.** This is firewalling.

### Why this protects against a single-chain failure

Suppose a chain's ITS edge is compromised, or a relayer crafts a fraudulent inbound report. The bad message reaches the Hub from that chain claiming a large outbound transfer (i.e., trying to "pull" tokens out of the bad chain to elsewhere). The Hub looks up the bad chain's `TokenInstance.supply`, attempts `checked_sub(fraud_amount)` from it, finds the counter does not cover it, and reverts. **No tokens move.** The fraud is contained to the compromised chain; the broader bridged system is untouched.

This is the property meant by "firewalling": one chain's failure does not become every chain's failure. The Hub is the chokepoint that enforces local conservation.

## Edge Chain Flow Limits

Each edge chain's ITS provides the ability to configure per-window flow limits ("X tokens per Y hours") on every token it bridges. Since XRPL has no smart contracts, it cannot host this property. Its counterparts on the other side of the bridge can, though: in the XRPL to XRPL-EVM round-trip example, where XRP is bridged between the two chains, the XRPL-EVM edge can throttle the amount of XRP that flows in either direction within a 6-hour window, because XRPL-EVM is an EVM chain and runs the standard Solidity ITS.

### How a limit is set on the XRPL-EVM edge

On EVM chains, the flow limit is configured by calling [`setFlowLimits`](https://github.com/axelarnetwork/interchain-token-service/blob/main/contracts/InterchainTokenService.sol#L606) on the `InterchainTokenService` contract:

```solidity
// InterchainTokenService.sol (Solidity ITS edge)

/**
 * @notice Used to set a flow limit for a token manager that has the service as its operator.
 * @param tokenIds   An array of the tokenIds of the tokenManagers to set the flow limits of.
 * @param flowLimits The flowLimits to set.
 */
function setFlowLimits(bytes32[] calldata tokenIds, uint256[] calldata flowLimits)
    external
    onlyRole(uint8(Roles.OPERATOR))
{
    uint256 length = tokenIds.length;
    if (length != flowLimits.length) revert LengthMismatch();

    for (uint256 i; i < length; ++i) {
        deployedTokenManager(tokenIds[i]).setFlowLimit(flowLimits[i]);
    }
}
```

The call takes parallel arrays of `tokenIds` and `flowLimits` and forwards each one to the corresponding `TokenManager`. Internally, each `TokenManager.setFlowLimit` writes the new cap to its own storage slot, and from that point on, every inbound or outbound transfer is gated against the new value.
