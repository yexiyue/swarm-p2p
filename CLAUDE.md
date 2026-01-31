# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`swarm-p2p` is a shared Rust P2P networking library extracted from the SwarmDrop project. It provides libp2p-based networking capabilities for decentralized applications, intended to be reused across multiple apps in the swarm-apps ecosystem.

**Crate:** `swarm-p2p-core`

## Build Commands

```bash
# Build the library
cargo build

# Run tests
cargo test

# Run a specific test
cargo test test_name

# Lint with Clippy
cargo clippy

# Format code
cargo fmt

# Check without building
cargo check
```

## Architecture

This is a Rust workspace with a single `core` crate. The library re-exports `libp2p` with pre-configured features suitable for P2P file transfer and device discovery applications.

### libp2p Features Enabled

- **Transport:** `tcp`, `quic`, `noise` (encryption), `yamux` (multiplexing), `dns`
- **Discovery:** `mdns` (LAN), `kad` (Kademlia DHT)
- **NAT Traversal:** `dcutr` (hole punching), `relay`, `autonat`
- **Protocols:** `identify`, `ping`, `gossipsub`
- **Runtime:** `tokio`

### Intended Capabilities

The library is designed to provide:
- LAN device discovery via mDNS
- Cross-network peer discovery via Kademlia DHT
- NAT traversal using DCUtR and relay fallback
- Encrypted transport layer

## Consuming This Library

This library is used by the `swarmdrop` Tauri application at `D:\workspace\swarmdrop`. When making changes here, consider the impact on dependent projects.

```toml
# Example usage in dependent Cargo.toml
[dependencies]
swarm-p2p-core = { path = "../swarm-p2p/core" }
```

## Rust Edition

This project uses Rust **2024 edition**.
