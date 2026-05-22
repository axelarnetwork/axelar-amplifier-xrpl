# Tickets

XRPL enforces a strict per-account [`Sequence`](glossary.md#sequence) number on every transaction (the same role an EVM nonce plays). For the multi sign account that backs the bridge, this would normally mean only one transaction can be in flight at a time, and confirmations would have to land strictly in order. To get around that, the [XRPL Multisig Prover](contracts/xrpl_multisig_prover.md) uses **tickets**: pre-allocated sequence numbers that the multi sign account reserves in advance, drawn from a pool of up to 250.

Tickets let many bridge transactions be signed and submitted in parallel, and let them confirm out of order. Operations that fundamentally have to be serial (pool refill, verifier set rotation, trust line management) still use a plain sequence number.

| Transaction type | Sequence field | Why |
| --- | --- | --- |
| `Payment` | Ticket | Many in flight at once; can confirm out of order |
| `TicketCreate` | Plain sequence | Refills the ticket pool; must be strictly ordered |
| `SignerListSet` | Plain sequence | Verifier set rotation; one at a time |
| `TrustSet` | Plain sequence | One trust line at a time |

## Ticket assignment

At any point a set of **available tickets** is tracked by the prover: ticket numbers that have been created with a `TicketCreate` transaction but that are not known to have been consumed by a transaction that made it to the XRPL ledger. New outbound transactions get assigned available tickets in order until the pool is exhausted.

<div style="margin: 1.5em 0;">
<div style="display: flex; justify-content: center; gap: 24px; font-size: 0.85em; color: #555; margin-bottom: 4px;">
  <span style="width: 168px; text-align: center;">Confirmed tickets</span>
  <span style="width: 280px; text-align: center;">Available tickets</span>
</div>
<div style="display: flex; justify-content: center; gap: 4px; font-family: monospace;">
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx1</div>
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx2</div>
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx3</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">tx4</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">tx5</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">&nbsp;</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">&nbsp;</div>
  <div style="background:#C5E6B3; padding:14px 0; border:1px solid #7CA56A; width:54px; text-align:center;">&nbsp;</div>
</div>
<div style="display: flex; justify-content: center; gap: 4px; font-size: 0.8em; color: #555; margin-top: 2px;">
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:54px; text-align:center;">▲</span>
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:54px; text-align:center;">▲</span>
</div>
<div style="display: flex; justify-content: center; gap: 4px; font-size: 0.8em; color: #555;">
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:118px; text-align:center;">Last assigned ticket</span>
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:118px; text-align:center;">Next sequence number</span>
</div>
</div>

If the number of available tickets falls below a configurable threshold, the prover constructs a new `TicketCreate` transaction (see [Ticket creation flow](#ticket-creation-flow) below).

### Ticket reuse under contention

If transactions are produced faster than the ticket pool can be refilled, ticket assignment wraps around to the first available ticket (similar to a round-robin scheduler). This can lead to multiple in-flight transactions being assigned the same ticket number:

<div style="margin: 1.5em 0;">
<div style="display: flex; justify-content: center; gap: 4px; font-family: monospace;">
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx1</div>
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx2</div>
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx3</div>
  <div style="background:#D4E6F5; padding:6px 0; border:1px solid #6B95C4; width:54px; text-align:center; line-height: 1.2;">tx4<br/>tx8</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">tx5</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">tx6</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">tx7</div>
  <div style="background:#C5E6B3; padding:14px 0; border:1px solid #7CA56A; width:54px; text-align:center;">&nbsp;</div>
</div>
<div style="display: flex; justify-content: center; gap: 4px; font-size: 0.8em; color: #555; margin-top: 2px;">
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:54px; text-align:center;">▲</span>
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:54px; text-align:center;">▲</span>
</div>
<div style="display: flex; justify-content: center; gap: 4px; font-size: 0.8em; color: #555;">
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:118px; text-align:center;">Last assigned ticket</span>
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:118px; text-align:center;">Next sequence number</span>
</div>
</div>

This is safe because of how XRPL handles ticket consumption: only one of the transactions sharing a ticket will ultimately be included in the ledger. Once that happens, the ticket is consumed permanently by that single transaction; every other transaction that tried to use the same ticket will be rejected by the ledger.

<div style="margin: 1.5em 0;">
<div style="display: flex; justify-content: center; gap: 4px; font-family: monospace;">
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx1</div>
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx2</div>
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx3</div>
  <div style="background:#FBD9A8; padding:14px 0; border:1px solid #B8915F; width:54px; text-align:center;">tx8</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">tx5</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">tx6</div>
  <div style="background:#D4E6F5; padding:14px 0; border:1px solid #6B95C4; width:54px; text-align:center;">tx7</div>
  <div style="background:#C5E6B3; padding:14px 0; border:1px solid #7CA56A; width:54px; text-align:center;">&nbsp;</div>
</div>
<div style="display: flex; justify-content: center; gap: 4px; font-size: 0.8em; color: #555; margin-top: 2px;">
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:54px; text-align:center;">▲</span>
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:54px; text-align:center;">▲</span>
</div>
<div style="display: flex; justify-content: center; gap: 4px; font-size: 0.8em; color: #555;">
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:118px; text-align:center;">Last assigned ticket</span>
  <span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span><span style="width:54px;">&nbsp;</span>
  <span style="width:118px; text-align:center;">Next sequence number</span>
</div>
</div>

## Ticket creation flow

XRPL limits the number of available tickets at any given time to **250**. When the size of the prover's ticket pool drops below a configurable threshold, a new `TicketCreate` transaction must be submitted to top it back up.

The flow:

1. A `Payment` transaction is confirmed on the XRPL ledger (and observed on Axelar through the standard `ProverMessage` verification path), which reduces the number of available tickets below the creation threshold.
2. The relayer requests a new `TicketCreate` transaction from the XRPL Multisig Prover.
3. The prover serializes a `TicketCreate` that asks the ledger to create `250 - len(available_tickets)` new tickets. There may actually be fewer usable tickets on XRPL than the prover currently knows about (in-flight transactions that have not been confirmed yet on Axelar), but there can never be more, so this bound is safe.
4. Axelar verifiers sign the new `TicketCreate` transaction through the standard multi-sign session.
5. The relayer submits the signed transaction to XRPL.
6. Once the transaction is included in a validated ledger, the relayer reports the inclusion back to Axelar as a `ProverMessage`.
7. Axelar verifiers vote on whether the XRPL transaction has been finalized.
8. If the poll succeeds, the newly created tickets become available to be assigned to subsequent `Payment` transactions.

## Fee reserve

In addition to the ticket pool, the prover tracks a `FEE_RESERVE` counter (in [XRP drops](glossary.md#drops)) that represents the multi sign account's available XRP for paying transaction fees. Every new transaction the prover builds is gated by `ensure_sufficient_fee_reserve`, which checks the budget against:

```
xrpl_base_reserve + xrpl_owner_reserve * (251 + trust_line_count) + tx_fee
```

The `+ 251` term accounts for the maximum 250 tickets plus the multi sign account's own `SignerList`, each of which counts as an "owned object" on XRPL and contributes to the [reserve](glossary.md#reserve) requirement. The reserve is topped up via `ConfirmAddReservesMessage` after the [XRPL Voting Verifier](contracts/xrpl_voting_verifier.md) confirms an operator-initiated XRP top-up `AddReservesMessage`.
