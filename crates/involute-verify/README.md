# involute-verify

Linux CLI verifier for **INVOLUTE** competitive solve recordings.

Given a recording JSON from [involute.chandler.io](https://involute.chandler.io), this tool checks:

1. **drand** — scramble randomness comes from a verified [drand](https://drand.love/) quicknet round
2. **Scramble** — baseline regenerates bit-for-bit from the seed (ChaCha12 + weak-scramble rejection)
3. **Replay** — timestamped moves reach a solved state within the TSA commitment window
4. **Commitment** — SHA-256 digest matches the recording’s claimed digest
5. **TSA** — RFC 3161 timestamp verifies with OpenSSL against moda trust anchors

Progress goes to **stderr**. Use `--json` for a machine-readable `SolveVerification` report on **stdout**.

## Download

Prebuilt Linux x86_64 binaries: [GitHub Releases](https://github.com/cjgriscom/involute-verify/releases) (`involute-verify` asset). Workflow artifact name: **`involute-verify-linux-x86_64`**.

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

## Usage

```
involute-verify [OPTIONS] <RECORDING>
```

| Argument / option | Description |
|-----------------|-------------|
| `<RECORDING>` | Path to an Involute solve recording JSON file |
| `--json` | Print `SolveVerification` JSON on stdout (progress still on stderr) |
| `-q`, `--quiet` | Suppress progress messages on stderr |
| `--cache-dir <DIR>` | Cache directory (default: `~/.cache/involute-verify`) |
| `--puzzle-base-url <URL>` | Base URL for versioned puzzle artifacts (default: `https://involute.chandler.io/puzzles`) |
| `--force-refresh-certs` | Re-probe moda backends even if the cached CA bundle is fresh |
| `-h`, `--help` | Print help |

```bash
involute-verify path/to/recording.json
involute-verify --json path/to/recording.json
involute-verify -q path/to/recording.json
involute-verify --cache-dir /tmp/iv-cache recording.json
involute-verify --force-refresh-certs recording.json
```

Exit code `0` = pass, non-zero = fail.

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

## Library

```rust
use involute_verify::{set_verbose, verify_recording, VerifyOptions};

set_verbose(false);
let report = verify_recording(path, &VerifyOptions::default())?;
```

## TSA trust model

- Trust anchors are **cached** from moda’s active (`default_enabled`) backends and refreshed every 10 days.
- Recording TSRs must include the certificate chain (`certReq: true`); those certs are passed to OpenSSL as `-untrusted` only.
- Certificates from a recording are never promoted to trust anchors (no TOFU).

## License

MIT OR Apache-2.0
