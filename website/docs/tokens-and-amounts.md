---
title: Supported Values and Tokens
sidebar_position: 3
---

# Supported Values and Tokens on XRPL

This page explains everything a developer or integrator needs to know about which tokens the bridge supports, how amounts are represented on XRPL, how amounts get converted when bridging in either direction, and what XRPL-side constraints (trust lines, reserves, currency codes) you have to satisfy. The relevant source lives in [`packages/xrpl-types`](https://github.com/axelarnetwork/axelar-amplifier/tree/main/packages/xrpl-types), the [XRPL Gateway](contracts/xrpl_gateway.md), and the [XRPL Multisig Prover](contracts/xrpl_multisig_prover.md).

## XRP vs IOU: the two value primitives

XRPL has exactly two primitive value types:

* **Native XRP**, the chain's native currency. Always denominated in [**drops**](glossary.md#drop) as a 64-bit unsigned integer. 1 XRP = 10<sup>6</sup> drops, so XRP behaves as a 6-decimal token from the bridge's point of view.
* **[IOU](glossary.md#iou-issued-currency) (issued currency)**, XRPL's primitive for everything that is not XRP. An IOU is uniquely identified by the pair `(currency_code, issuer_account)`. Holders track their balance through a [trust line](glossary.md#trust-line) opened against the issuer.

The bridge exposes both as cross-chain assets. They share the same on-chain message envelope (the `Amount` field of an XRPL `Payment`), but their internal representation and the conversion rules between an XRPL amount and an Axelar `Uint256` are different.

In the Rust types these are unified by a single enum (`packages/xrpl-types/src/types.rs`):

```Rust
pub enum XRPLPaymentAmount {
    Drops(u64),                          // native XRP, in drops
    Issued(XRPLToken, XRPLTokenAmount),  // IOU: token id + a mantissa/exponent amount
}

pub struct XRPLToken {
    pub issuer: XRPLAccountId,
    pub currency: XRPLCurrency,
}

pub struct XRPLTokenAmount {
    mantissa: u64,
    exponent: i64,
}
```

## XRP: native amounts in drops

XRP amounts are 64-bit unsigned integers of drops. There is a protocol-level cap on the maximum drops in circulation (and therefore on any single transfer):

| Constant | Value | Source |
| --- | --- | --- |
| `XRP_DECIMALS` | `6` | `packages/xrpl-types/src/types.rs` |
| `XRP_MAX_UINT` | `100_000_000_000_000_000` (= 10<sup>17</sup> drops, 100 billion XRP) | same |

When the bridge converts an amount coming from a smart-contract chain (delivered as `Uint256` by the ITS Hub) into an XRPL drops amount, the multisig prover's [`compute_xrpl_amount`](https://github.com/axelarnetwork/axelar-amplifier-xrpl/blob/main/contracts/xrpl-multisig-prover/src/contract/execute.rs) checks `source_amount <= XRP_MAX_UINT` and otherwise returns `InvalidTransferAmount`. Any value above the cap is rejected at proof construction time.

## IOU: 54-bit mantissa, 8-bit exponent

XRPL's IOU amounts are a custom floating-point format, [documented here](https://xrpl.org/docs/references/protocol/binary-format#token-amount-format). The bridge stores them as `(mantissa: u64, exponent: i64)` and serializes them into XRPL's wire format with the following constraints (`packages/xrpl-types/src/types.rs`):

| Constant | Value |
| --- | --- |
| `MIN_MANTISSA` | `1_000_000_000_000_000` (= 10<sup>15</sup>) |
| `MAX_MANTISSA` | `10_000_000_000_000_000 - 1` (= 10<sup>16</sup> - 1) |
| `MIN_EXPONENT` | `-96` |
| `MAX_EXPONENT` | `80` |
| `XRPL_ISSUED_TOKEN_DECIMALS` | `15` (the assumed decimals for the canonical Uint256 → IOU conversion) |

The on-wire binary layout for a non-zero IOU amount is:

```
1 bit  : 1 ("not XRP")
1 bit  : sign (always 1, positive)
8 bits : exponent + 97
54 bits: mantissa
```

(The mantissa is always normalized to `MIN_MANTISSA <= mantissa <= MAX_MANTISSA`, so it fits in 54 bits even though it spans roughly 15 to 16 decimal digits.)

For a zero amount XRPL uses the special encoding `0x8000000000000000`.

The bridge never produces non-positive or non-canonical IOU amounts: zeros pass through as the canonical zero encoding, and any amount that would normalize outside the `[MIN_MANTISSA, MAX_MANTISSA]` window after exponent adjustment is rejected.

## Canonicalization: turning a Uint256 into an XRPL IOU amount

When the [XRPL Multisig Prover](contracts/xrpl_multisig_prover.md) is delivering tokens to XRPL (Flow C), the ITS Hub hands it an integer amount in the source chain's decimals as a `Uint256`. The prover converts it to an `XRPLTokenAmount` via `canonicalize_token_amount(amount: Uint256, decimals: u8)`:

1. Start with the raw integer mantissa and an exponent of `-decimals` (so the represented value is `mantissa * 10^(-decimals)`).
2. While `mantissa < MIN_MANTISSA` and `exponent > MIN_EXPONENT`, multiply by 10 and subtract 1 from the exponent.
3. While `mantissa > MAX_MANTISSA`, divide by 10 and add 1 to the exponent (overflow if `exponent` would exceed `MAX_EXPONENT`).
4. If `exponent < MIN_EXPONENT`, the value rounds to 0 and the result is the canonical zero.
5. If `exponent > MAX_EXPONENT` or the mantissa cannot be normalized into the window, the operation errors.

Source: [`canonicalize_mantissa`](https://github.com/axelarnetwork/axelar-amplifier/blob/main/packages/xrpl-types/src/types.rs#L1158) and [`canonicalize_token_amount`](https://github.com/axelarnetwork/axelar-amplifier/blob/main/packages/xrpl-types/src/types.rs#L1207), both in `packages/xrpl-types/src/types.rs`.

For the reverse direction (XRPL inbound), the [XRPL Gateway](contracts/xrpl_gateway.md) calls `scale_to_decimals(amount: XRPLTokenAmount, destination_decimals: u8)` to convert the IOU `(mantissa, exponent)` into a `Uint256` aligned to whatever decimal precision the destination chain registered for the token with the ITS Hub. The same function returns zero if the input mantissa is zero, multiplies by `10^(exponent + destination_decimals)` when the result is integer, and divides otherwise (silently flooring sub-precision dust).

## Currency codes

XRPL currency codes are 160-bit values (20 bytes) but the wire format accepts them in two human-readable forms:

| Form | Regex | Notes |
| --- | --- | --- |
| **3-char ASCII** | `^[A-Za-z0-9\?\!@#\$%\^&\*<>\(\)\{\}\[\]\|]{3}$` | The legacy XRPL form. The 3 characters are placed at bytes [12..15] of the 20-byte value; all other bytes are zero. |
| **40-char hex** | `^[A-Fa-f0-9]{40}$` (excluding any string starting with `00`) | The "non-standard" 160-bit currency code form. Used for codes longer than 3 characters or with characters outside the legacy ASCII set. |

Two values are explicitly reserved and rejected by `XRPLCurrency::new`:

* The literal ASCII `XRP` (`0x...58 52 50...`), which would collide with native XRP at the wire-format level.
* The all-zero 20-byte value (no currency).

The bridge enforces these as `XRPLError::ReservedCurrency`. Source: [`XRPLCurrency::is_reserved`](https://github.com/axelarnetwork/axelar-amplifier/blob/main/packages/xrpl-types/src/types.rs).

## Token IDs

The bridge identifies each cross-chain token with the standard ITS 32-byte [`tokenId`](glossary.md#token-id). On XRPL specifically the [`xrpl-gateway`](contracts/xrpl_gateway.md) maintains three registries:

| Map | Purpose |
| --- | --- |
| `XRPL_TOKEN_TO_LOCAL_TOKEN_ID: Map<XRPLToken, TokenId>` | Lookups for **XRPL-native IOUs**: the issuer is some XRPL account other than the multisig account, and the token has been registered via `RegisterLocalToken`. |
| `XRPL_CURRENCY_TO_REMOTE_TOKEN_ID: Map<XRPLCurrency, TokenId>` | Lookups for **remote-origin tokens**: the XRPL representation is an IOU issued by the multisig account itself, registered via `RegisterRemoteToken`. |
| `TOKEN_ID_TO_XRPL_TOKEN: Map<TokenId, XRPLToken>` | Reverse lookup. |

Native XRP has its own special token id computed once at gateway instantiation: `tokenId(salt = keccak256("XRP" zero-padded to 20 bytes))` with deployer `rrrrrrrrrrrrrrrrrrrrrhoLvTp` (XRPL's canonical null-account). This is exposed via the gateway's `XrpTokenId` query.

## Supported token categories

Across the whole bridge, three categories of tokens can move to and from XRPL:

| Category | XRPL representation | How a user receives it on XRPL | How tokens are "minted" on XRPL outbound delivery |
| --- | --- | --- | --- |
| **Native XRP** | Drops | Drops sent by the multisig account | Multisig account pays drops out of its own balance (sourced from inbound transfers) |
| **XRPL-native IOU** (origin = XRPL, issuer ≠ multisig) | IOU `(currency, issuer)` | Multisig account sends IOU it received on inbound transfers; multisig must have a `TrustSet` to the issuer. | Held in the multisig account's balance, sent out via `Payment` of that IOU |
| **Remote-origin token** (origin = another chain) | IOU `(currency, multisig_issuer)` | Multisig account mints the IOU to the recipient. Recipient must already have a `TrustSet` against the multisig account for that currency code. | New IOU created on the fly; no external balance needed |

## Trust line setup

XRPL holders track IOU balances via [trust lines](glossary.md#trust-line). Two scenarios apply:

* **For XRPL-native IOUs** (issuer is some external XRPL account), the multisig account itself must have a `TrustSet` open against the issuer before it can hold the IOU and pay it back out. Operators trigger this by calling `TrustSet { token_id }` on the [XRPL Multisig Prover](contracts/xrpl_multisig_prover.md), which builds and signs an XRPL `TrustSet` transaction.
* **For remote-origin tokens** (issuer is the multisig account), the **recipient on XRPL** must have a `TrustSet` open against the multisig account before any inbound delivery can succeed. Without it the XRPL ledger rejects the `Payment`. This is a user-side requirement; the bridge does not open trust lines on the recipient's behalf.

Each trust line is also an "owned object" on the XRPL ledger and therefore consumes an [owner reserve](glossary.md#reserve) on the holder. 

## Address formats

XRPL accounts are referenced two ways in the bridge:

* In the on-XRPL world, as **classic addresses**: base58 strings starting with `r`, for example `rPEPPER7kfTD9w2To4CQk6UCfuHM9c6GDY`. Underneath this is a 20-byte account id (`RIPEMD160(SHA256(public_key))`).
* Inside the on-Axelar contracts, as the type-safe [`XRPLAccountId`](https://github.com/axelarnetwork/axelar-amplifier/blob/main/packages/xrpl-types/src/types.rs) (20 bytes). Serialization to/from the classic-address string form is handled by the `xrpl_account_id_string` serde module.

When a non-XRPL chain wants to send tokens to an XRPL recipient (Flow C), it uses the recipient's classic-address bytes encoded as the ITS `destination_address`. The [XRPL Multisig Prover](contracts/xrpl_multisig_prover.md) parses the bytes back into an `XRPLAccountId` at proof construction time and rejects malformed addresses.

## Decimal handling across chains

The ITS Hub tracks per-chain `decimals` for every token in the [`TokenInstance`](https://github.com/axelarnetwork/axelar-amplifier/tree/main/contracts/interchain-token-service) map. Each chain registers its native decimals when the token is first deployed or linked there:

| Chain category | Decimals registered with the ITS Hub | Notes |
| --- | --- | --- |
| XRPL (XRP) | 6 | Matches the drops representation. |
| XRPL (IOU) | 15 | The `XRPL_ISSUED_TOKEN_DECIMALS` constant. Used as the assumed precision for the `Uint256` ↔ mantissa/exponent conversion. |
| Smart-contract chains | Whatever each chain registers (e.g., 18 for most EVM ERC-20s, 6 for USDC variants, 9 for Solana SPL, …) | Defined per token, per chain. |

When a transfer crosses a decimal boundary, the ITS Hub rescales the integer amount before forwarding. For XRPL the actual canonicalization into the mantissa/exponent form (or the truncation into drops) happens at the XRPL Multisig Prover, using the per-chain decimals it queries from the gateway. The reverse direction (XRPL outbound, Flows A and B) is symmetric: the XRPL Gateway queries the destination chain's decimals via the ITS Hub before scaling and wrapping the message.
