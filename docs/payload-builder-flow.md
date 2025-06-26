# Reth-Malachite Payload Builder Flow

This document describes the integration between Malachite consensus and Reth's execution engine for block production and validation.

## Overview

The integration follows the standard Ethereum Engine API pattern:
1. **Malachite requests block** → Consensus requests a new block proposal
2. **FCU with attributes** → Fork Choice Update triggers payload building
3. **Get payload** → Retrieve the built block from the payload builder
4. **Propose value** → Return the block to consensus for voting
5. **Commit with new_payload + FCU** → Validate and finalize the decided block

## Architecture Components

### Core Components

- **State struct** (`src/app/state.rs`): Central coordinator between consensus and execution
- **payload_builder_handle**: Manages block building through Reth's payload builder service
- **engine_handle**: Handles Engine API calls (FCU, new_payload) to the execution layer
- **PayloadStore**: Wrapper around payload_builder_handle for convenient payload retrieval

### Initialization

From `src/bin/reth-malachite.rs`:

```rust
// Get handles from the launched Reth node
let payload_builder_handle = node.payload_builder_handle.clone();
let app_handle = node.add_ons_handle.beacon_engine_handle.clone();

// Create State with both handles
let state = State::from_provider(
    ctx.clone(),
    config,
    genesis.clone(),
    address,
    Arc::new(provider),
    app_handle,            // Engine handle for FCU/new_payload
    payload_builder_handle, // For building blocks
).await?;
```

## Block Production Flow

### 1. Consensus Requests Block

When Malachite needs a new block proposal (`src/consensus/handler.rs`):

```rust
AppMsg::GetValue { height, round, timeout: _, reply } => {
    // First check if we already have a value for this height/round
    match state.get_previously_built_value(height, round).await? {
        Some(proposal) => {
            // Reuse previously built value
            reply.send(Ok(proposal)).unwrap();
        }
        None => {
            // Build new value via state.propose_value()
            match state.propose_value(height, round).await {
                Ok(proposal) => {
                    reply.send(Ok(proposal)).unwrap();
                }
                Err(e) => {
                    reply.send(Err(e)).unwrap();
                }
            }
        }
    }
}
```

### 2. Building the Block

The `State::propose_value()` method orchestrates block building:

```rust
pub async fn propose_value(&self, height: Height, round: Round) -> Result<LocallyProposedValue<MalachiteContext>> {
    // 1. Create payload attributes for the new block
    let payload_attrs = PayloadAttributes {
        timestamp,
        prev_randao: B256::ZERO,
        suggested_fee_recipient: self.config.fee_recipient,
        withdrawals: Some(vec![]),
        parent_beacon_block_root: Some(B256::ZERO),
    };

    // 2. Send FCU to trigger payload building
    let forkchoice_state = ForkchoiceState {
        head_block_hash: parent_hash,
        safe_block_hash: parent_hash,
        finalized_block_hash: self.get_finalized_hash().await?,
    };

    let fcu_response = self.engine_handle.fork_choice_updated(
        forkchoice_state,
        Some(payload_attrs),  // This triggers payload building!
        EngineApiMessageVersion::V3,
    ).await?;

    // 3. Get the payload ID from FCU response
    let payload_id = fcu_response.payload_id
        .ok_or_else(|| eyre::eyre!("No payload ID returned from FCU"))?;

    // 4. Get the built payload (waits for pending transactions)
    let payload = self.payload_store
        .resolve_kind(payload_id, PayloadKind::WaitForPending)
        .await
        .ok_or_else(|| eyre::eyre!("No payload found for id {:?}", payload_id))??;

    // 5. Create and store the proposal
    let sealed_block = payload.block();
    let value = Value::new(sealed_block.clone_block());
    let locally_proposed = LocallyProposedValue::new(height, round, value.clone());
    self.store_built_proposal(proposed_value).await?;
    
    Ok(locally_proposed)
}
```

Key points:
- `PayloadKind::WaitForPending` ensures we wait for transactions to be included
- The payload builder runs asynchronously after FCU is called with attributes
- Built proposals are cached to avoid rebuilding for the same height/round

## Block Commit Flow

### 3. Consensus Decides on Value

When consensus reaches agreement (`src/consensus/handler.rs`):

```rust
AppMsg::Decided { certificate, block, reply } => {
    // Commit the decided value
    match state.commit(certificate.clone(), block.clone()).await {
        Ok(()) => {
            reply.send(Ok(())).unwrap();
        }
        Err(e) => {
            reply.send(Err(e)).unwrap();
        }
    }
}
```

### 4. Validating and Finalizing

The `State::commit()` method validates and finalizes the block:

```rust
pub async fn commit(&self, certificate: CommitCertificate<MalachiteContext>, ...) -> Result<()> {
    // 1. Find the decided value
    let value = /* find value matching certificate.value_id */;

    // 2. Store decided value for persistence
    self.store.store_decided_value(certificate, value.clone()).await?;

    // 3. Validate block via new_payload
    let sealed_block = SealedBlock::seal_slow(value.block.clone());
    let payload = <EthEngineTypes as PayloadTypes>::block_to_payload(sealed_block);
    let payload_status = self.engine_handle.new_payload(payload).await?;

    if payload_status.status != PayloadStatusEnum::Valid {
        return Err(eyre::eyre!("Invalid payload status: {:?}", payload_status));
    }

    // 4. Make block canonical via FCU (instant finality)
    let block_hash = block.header.hash_slow();
    let forkchoice_state = ForkchoiceState {
        head_block_hash: block_hash,
        safe_block_hash: block_hash,      // Instant finality
        finalized_block_hash: block_hash,  // Instant finality
    };

    let fcu_response = self.engine_handle.fork_choice_updated(
        forkchoice_state,
        None,  // No new payload to build
        EngineApiMessageVersion::V3,
    ).await?;

    // Update state and metrics
    self.last_finalized_height.store(height.as_u64(), Ordering::Relaxed);
    
    Ok(())
}
```

Key points:
- `new_payload` validates the block execution
- All three fork choice states (head, safe, finalized) are set to the same block due to Malachite's instant finality
- The block becomes canonical and finalized in a single operation

## Syncing Flow

For blocks received during sync, there's a separate validation path:

```rust
pub async fn validate_synced_block(&self, certificate: CommitCertificate<MalachiteContext>, block: Block) -> Result<()> {
    // Validate via new_payload
    let sealed_block = SealedBlock::seal_slow(block.clone());
    let payload = <EthEngineTypes as PayloadTypes>::block_to_payload(sealed_block);
    let payload_status = self.engine_handle.new_payload(payload).await?;

    if payload_status.status != PayloadStatusEnum::Valid {
        return Err(eyre::eyre!("Invalid payload status during sync: {:?}", payload_status));
    }

    // Store for later finalization
    self.store.store_decided_value(certificate, value).await?;
    
    Ok(())
}
```

## Error Handling

The integration includes comprehensive error handling:

1. **Invalid Payload**: Checked after `new_payload` calls
2. **Missing Payload ID**: Verified after FCU with attributes
3. **Payload Build Timeout**: Handled by PayloadStore with configurable timeout
4. **Fork Choice Errors**: Propagated from engine API calls

## Metrics and Monitoring

The system tracks:
- Last finalized height
- Block production success/failure
- Engine API response times
- Payload builder performance

## Configuration

Key configuration options in `State`:
- `fee_recipient`: Address to receive block rewards
- `engine_handle`: Connection to Reth execution engine
- `payload_builder_handle`: Connection to payload builder service