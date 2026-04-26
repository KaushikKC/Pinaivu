# Pinaivu Node Operator Guide

Run your GPU and earn by serving AI inference on the Pinaivu decentralised network.

---

## Table of Contents

1. [Requirements](#1-requirements)
2. [Install](#2-install)
3. [Set Up Ollama and Pull a Model](#3-set-up-ollama-and-pull-a-model)
4. [Initialise Your Node](#4-initialise-your-node)
5. [Start the Node](#5-start-the-node)
6. [Verify You Are Connected](#6-verify-you-are-connected)
7. [Test Inference](#7-test-inference)
8. [Keeping Your Node Running](#8-keeping-your-node-running)
9. [Uninstall / Clean Reset](#9-uninstall--clean-reset)
10. [Troubleshooting](#10-troubleshooting)

---

## 1. Requirements

| Item | Minimum |
|---|---|
| OS | macOS 12+, Ubuntu 20.04+, Windows 11 |
| RAM | 8 GB (16 GB recommended) |
| Storage | 10 GB free (models are 1–8 GB each) |
| Internet | Stable broadband — upload speed matters for inference |
| Ollama | v0.1.30 or later |

> **No public IP or port forwarding required.** Pinaivu routes inference through the P2P network automatically.

---

## 2. Install

### Mac / Linux (one command)

```bash
curl -fsSL https://github.com/KaushikKC/Pinaivu/releases/latest/download/install.sh | sh
```

This downloads the correct binary for your OS and architecture and places it at `/usr/local/bin/pinaivu`.

### Manual download

| Platform | Download |
|---|---|
| Mac — Apple Silicon (M1/M2/M3) | `pinaivu-macos-apple-silicon` |
| Mac — Intel | `pinaivu-macos-intel` |
| Linux x86_64 | `pinaivu-linux-x86_64` |
| Windows x86_64 | `pinaivu-windows-x86_64.exe` |

Download from: https://github.com/KaushikKC/Pinaivu/releases/latest

```bash
# Mac / Linux after downloading:
chmod +x pinaivu-*
sudo mv pinaivu-* /usr/local/bin/pinaivu
```

### Verify installation

```bash
pinaivu --version
```

---

## 3. Set Up Ollama and Pull a Model

Pinaivu uses [Ollama](https://ollama.com) to run models locally.

### Install Ollama

```bash
# Mac
brew install ollama

# Linux
curl -fsSL https://ollama.com/install.sh | sh
```

Or download the desktop app from https://ollama.com.

### Start Ollama

```bash
ollama serve
```

Leave this running in the background (or it starts automatically as a service on Mac).

### Pull a model

Pick one based on your RAM:

| Model | Size | RAM needed | Quality |
|---|---|---|---|
| `gemma3:1b` | 815 MB | 4 GB | Good for testing |
| `gemma3:4b` | 3.3 GB | 8 GB | Good balance |
| `llama3.1:8b` | 4.7 GB | 16 GB | High quality |
| `deepseek-r1:7b` | 4.7 GB | 16 GB | High quality reasoning |

```bash
# Example — pick the one that fits your machine
ollama pull gemma3:1b
```

Verify the model downloaded:

```bash
ollama list
```

---

## 4. Initialise Your Node

```bash
pinaivu init
```

This automatically:
- Detects all models you have in Ollama and sets the best one as default
- Detects your public IP address
- Tests whether your API port (4002) is reachable from the internet
- Writes a complete config to `~/.pinaivu/config.toml`

Example output:

```
Initialising Pinaivu node...

  Checking Ollama... found 2 model(s)
    ✓ gemma3:1b
    ✓ llama3.1:8b
  Default model set to: gemma3:1b

  Detecting public IP... 38.134.139.170
  Testing port 4002 reachability... blocked (NAT/firewall)
  → Your node can still earn — inference routes via P2P automatically.
    To enable direct connections: forward port 4002 on your router.

  Writing config to ~/.pinaivu/config.toml...

✓ Node ready!

  Start your node:   pinaivu start
  List models:       pinaivu models
  Node status:       pinaivu status
```

> **Already initialised?** Use `pinaivu init --force` to regenerate the config from scratch.

---

## 5. Start the Node

```bash
pinaivu start
```

You should see:

```
INFO pinaivu: starting pinaivu version=0.1.4 mode=Network
INFO p2p::service: P2P listening listen_addr=/ip4/0.0.0.0/tcp/7771
INFO p2p::service: peer connected peer_id=12D3KooWBoxCV...   ← AWS bootstrap
INFO pinaivu::api: inference API server listening port=4002
INFO pinaivu: daemon running — press Ctrl-C to stop
```

The line `peer connected peer_id=12D3KooWBoxCV...` confirms you are connected to the global Pinaivu network through the bootstrap relay.

---

## 6. Verify You Are Connected

In a second terminal:

```bash
# Check the node is running
curl -s http://localhost:7770/health | jq .

# List models your node is advertising
pinaivu models

# See peers connected to you
curl -s http://localhost:4002/v1/peers | jq '.[].peer_id'
```

You should see at least one peer (the AWS bootstrap node).

---

## 7. Test Inference

Send a local inference request to confirm your node works:

```bash
curl -s -X POST http://localhost:4002/v1/infer \
  -H "Content-Type: application/json" \
  -d '{
    "model_id": "gemma3:1b",
    "prompt": "What is 2+2? Answer briefly.",
    "max_tokens": 50
  }'
```

You should see tokens streaming back line by line:

```
{"token":"4","is_final":false}
{"token":"\n","is_final":false}
{"token":"","is_final":true}
```

---

## 8. Keeping Your Node Running

### macOS — keep running in background

```bash
# Run in background, logs go to ~/pinaivu.log
nohup pinaivu start > ~/pinaivu.log 2>&1 &

# Watch the logs
tail -f ~/pinaivu.log

# Stop it
pkill pinaivu
```

### Linux — systemd service (auto-start on boot)

```bash
# Download the service file
sudo curl -fsSL https://raw.githubusercontent.com/KaushikKC/Pinaivu/main/deploy/pinaivu.service \
  -o /etc/systemd/system/pinaivu.service

# Create the system user the service runs as
sudo useradd -r -s /sbin/nologin pinaivu
sudo mkdir -p /etc/pinaivu /var/lib/pinaivu/data

# Copy your config
sudo cp ~/.pinaivu/config.toml /etc/pinaivu/config.toml
# Edit data_dir to use the system path:
sudo sed -i 's|~/.pinaivu/data|/var/lib/pinaivu/data|' /etc/pinaivu/config.toml
sudo chown -R pinaivu:pinaivu /var/lib/pinaivu

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable pinaivu
sudo systemctl start pinaivu

# Check status
sudo systemctl status pinaivu
sudo journalctl -u pinaivu -f
```

---

## 9. Uninstall / Clean Reset

### Full uninstall

```bash
# 1. Stop the node if running
pkill pinaivu 2>/dev/null || true
sudo systemctl stop pinaivu 2>/dev/null || true
sudo systemctl disable pinaivu 2>/dev/null || true

# 2. Remove the binary
sudo rm -f /usr/local/bin/pinaivu

# 3. Remove all node data and config
rm -rf ~/.pinaivu

# 4. Remove systemd service (Linux only)
sudo rm -f /etc/systemd/system/pinaivu.service
sudo systemctl daemon-reload
sudo rm -rf /etc/pinaivu /var/lib/pinaivu
```

### Fresh reinstall (keep Ollama models)

Run the above uninstall, then follow the guide from Step 2.

### Reset config only (keep identity / keypair)

```bash
# Regenerates config but keeps your P2P keypair and node identity
pinaivu init --force
```

### Reset everything including P2P identity

```bash
# Your node gets a new peer ID — existing peers won't recognise you
rm -rf ~/.pinaivu
pinaivu init
```

---

## 10. Troubleshooting

### "Config not found. Run `pinaivu init` first."

```bash
pinaivu init
```

---

### "Ollama not running" during init or start

Make sure Ollama is running:

```bash
ollama serve
```

Or open the Ollama desktop app. Then retry `pinaivu start`.

---

### Node starts but shows 0 peers

Check that you can reach the internet:

```bash
curl -s https://api.ipify.org
```

If that works but peers are still 0, wait 10–15 seconds — the bootstrap connection can take a moment on cold start.

---

### "Address already in use" on port 7771

Another process is using the P2P port. Either stop the conflicting process or change the port:

```bash
# Find what's using it
lsof -i :7771

# Or change the port in config
nano ~/.pinaivu/config.toml
# Change: listen_port = 7771  →  listen_port = 7772
```

---

### "Address already in use" on port 4001

This is usually IPFS. The default P2P port changed to 7771 in v0.1.2+. If you installed an older version:

```bash
pinaivu init --force   # regenerates config with correct ports
```

---

### Port 4002 blocked — will I still earn?

Yes. As of v0.1.4, inference is routed through the P2P network automatically. Port 4002 does not need to be publicly reachable. Clients route through the gossipsub relay.

---

### Model not found / inference fails

Check Ollama has the model:

```bash
ollama list
```

If the model shown in your config is not in the list, pull it:

```bash
ollama pull gemma3:1b
```

Or run `pinaivu init --force` and it will auto-select a model you actually have.

---

### Check node logs

```bash
# If running in foreground: logs print to terminal directly
# If running in background:
tail -f ~/pinaivu.log

# systemd:
sudo journalctl -u pinaivu -f
```

Set `log_level = "debug"` in `~/.pinaivu/config.toml` for verbose output.
