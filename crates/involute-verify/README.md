# involute-verify

Linux CLI verifier for **INVOLUTE** competitive solve recordings.

Given a recording JSON from [involute.chandler.io](https://involute.chandler.io), this tool checks:

1. **drand** — scramble randomness comes from a verified [drand](https://drand.love/) quicknet round
2. **Scramble** — baseline regenerates bit-for-bit from the seed (ChaCha12 + weak-scramble rejection)
3. **Replay** — timestamped moves reach a solved state within the TSA commitment window
4. **Commitment** — SHA-256 digest matches the recording’s claimed digest
5. **TSA** — RFC 3161 timestamp verifies with OpenSSL against moda trust anchors

Progress goes to **stderr**. Use `--json` for a machine-readable report on **stdout**.

## Requirements

| Need | When |
|------|------|
| Linux | Runtime (cache path uses `~/.cache`) |
| [OpenSSL](https://www.openssl.org/) CLI | Runtime (TSA verify + moda CA probe) |
| Network | First run / puzzle cache miss / moda CA refresh (every 10 days) |

No Node.js or puzzle tree is required to **build** the binary.

## Build

```bash
cargo build -p involute-verify --release
```

## Cache layout

Default root: `$XDG_CACHE_HOME/involute-verify` or `~/.cache/involute-verify`.

```
~/.cache/involute-verify/
  moda-cas.pem
  moda-meta.json
  {puzzleId}/{puzzleVersion}/
    data.yaml
    rejection-cache.bin
```

Puzzle files are fetched from:

```
https://involute.chandler.io/puzzles/{id}/{version}/data.yaml
https://involute.chandler.io/puzzles/{id}/{version}/rejection-cache.bin
```

(see also `/puzzles/index.json` for sha256 digests).

## Usage

```bash
involute-verify path/to/recording.json
involute-verify --json path/to/recording.json
involute-verify -q path/to/recording.json
involute-verify --cache-dir /tmp/iv-cache recording.json
involute-verify --force-refresh-certs recording.json
```

Exit code `0` = pass, non-zero = fail.

## Library

```rust
use involute_verify::{set_verbose, verify_recording, VerifyOptions};

set_verbose(false);
let report = verify_recording(path, &VerifyOptions::default())?;
```

## Recording format

Canonical wire format: [`docs/solve-recording.md`](../../docs/solve-recording.md) in this repository.

## TSA trust model

- Trust anchors are **cached** from moda’s active (`default_enabled`) backends and refreshed every 10 days.
- Recording TSRs must include the certificate chain (`certReq: true`); those certs are passed to OpenSSL as `-untrusted` only.
- Certificates from a recording are never promoted to trust anchors (no TOFU).

## License

MIT OR Apache-2.0
