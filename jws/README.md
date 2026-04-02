# Arai A2A *JWS*

JWS ([RFC 7515](https://datatracker.ietf.org/doc/html/rfc7515)) signature verification for [A2A](https://a2a-protocol.org/) agent cards.

Agent operators sign their cards so callers can verify integrity—important when cards are cached in registries or proxied through intermediaries. This crate provides the verification side using pure-Rust cryptography from the [RustCrypto](https://github.com/RustCrypto) project. No native dependencies, no FFI, Wasm-compatible.

## What's included

- **`AgentCardVerify`**—extension trait that adds `verify_signatures()` to `AgentCard`
- **`verify_detached_jws()`**—verify a single JWS segment against a known payload
- **`parse_jwks()`**—parse a JWKS document into a `Vec<VerificationKey>`
- **`VerificationKey`**—construct from JWK, raw EC point, or RSA components
- **Supported algorithms**—ES256 (ECDSA P-256) and RS256 (RSASSA-PKCS1-v1_5). RSA keys below 2048 bits are rejected.

## Usage

```toml
[dependencies]
a2a = "0.0.0"
a2a-jws = "0.0.0"
```

```rust
use a2a_jws::{AgentCardVerify, parse_jwks};

let keys = parse_jwks(&jwks_json)?;
card.verify_signatures(&keys)?;
```

For the core protocol types, see [`a2a`](../a2a).

## License

Copyright [Responsible Engineering Ab](https://responsible.engineering), governed by [Omnifi Foundation](https://omnifi.foundation).

[MPL-2.0](../LICENSE.txt)
