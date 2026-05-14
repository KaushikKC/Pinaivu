# Deploying DeAIEscrow (EVM)

Works on any EVM chain: Base, Arbitrum, Ethereum, or the ARC/Circles testnet.

## Prerequisites

```bash
curl -L https://foundry.paradigm.xyz | bash
foundryup
```

## Deploy to any EVM testnet

```bash
cd contracts/evm

forge create DeAIEscrow.sol:DeAIEscrow \
  --rpc-url  <RPC_URL>          \
  --private-key <YOUR_KEY_HEX>  \
  --broadcast
```

The command prints `Deployed to: 0x...` — copy that address.

### Example: Base Sepolia testnet

```bash
forge create DeAIEscrow.sol:DeAIEscrow \
  --rpc-url  https://sepolia.base.org \
  --private-key $PRIVATE_KEY \
  --broadcast
```

### Example: ARC / Circles EVM testnet

Replace `<ARC_RPC>` with the chain's RPC endpoint and `<CHAIN_ID>` accordingly:

```bash
forge create DeAIEscrow.sol:DeAIEscrow \
  --rpc-url  <ARC_RPC> \
  --private-key $PRIVATE_KEY \
  --broadcast
```

## Wire up the Rust daemon

Once deployed, add to `~/.pinaivu/config.toml`:

```toml
[[settlement.adapters]]
id               = "evm-<CHAIN_ID>"          # e.g. "evm-8453" for Base mainnet
rpc_url          = "<RPC_URL>"
contract_address = "0x<DEPLOYED_ADDRESS>"
chain_id         = <CHAIN_ID>
price_per_1k     = 10
token_id         = "native"
signer_key_hex   = "<32_BYTE_PRIVATE_KEY_HEX>"
```

Then restart the daemon — it will log `settlement adapters: [evm-<CHAIN_ID>, free]` at startup.

## Verify on-chain (optional)

```bash
forge verify-contract <DEPLOYED_ADDRESS> DeAIEscrow.sol:DeAIEscrow \
  --chain-id <CHAIN_ID> \
  --etherscan-api-key $ETHERSCAN_API_KEY
```
