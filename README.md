# involute-verify

Linux CLI (and library) for verifying [INVOLUTE](https://involute.chandler.io) competitive solve recordings.

This workspace contains:

| Crate | Role |
|-------|------|
| [`involute-verify`](crates/involute-verify/) | Verifier binary + library |
| [`timecheck`](crates/timecheck/) | drand + RFC 3161 helpers (vendored from Hyperspeedcube) |

Puzzle definitions are downloaded at runtime from `https://involute.chandler.io/puzzles/{id}/{version}/` and cached under `~/.cache/involute-verify`.

## Download

Prebuilt Linux x86_64 binaries are published on [GitHub Releases](https://github.com/cjgriscom/involute-verify/releases). Download the `involute-verify` asset, make it executable, and run it directly:

```bash
chmod +x involute-verify
./involute-verify path/to/recording.json
```

CI workflow runs also expose the same binary as a workflow artifact named **`involute-verify-linux-x86_64`** (Actions → workflow run → Artifacts).

Requires OpenSSL on `PATH` at runtime.

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

See [`crates/involute-verify/README.md`](crates/involute-verify/README.md) for cache layout and trust model.

## Spec

Solution STM counting: [`docs/stm.md`](docs/stm.md).

## Attribution

`timecheck` is vendored from [Hyperspeedcube](https://github.com/HactarCE/Hyperspeedcube)
by **Andrew Farkas** ([HactarCE](https://github.com/HactarCE)), with small TSA
extensions for moda multi-CA verification. Details:
[`crates/timecheck/README.md`](crates/timecheck/README.md).

The underlying timing scheme is described by
[5225225](https://www.5snb.club/posts/2025/cheating-resistant-racing-games/).

## License

MIT OR Apache-2.0
