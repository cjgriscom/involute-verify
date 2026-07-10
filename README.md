# involute-verify

Linux CLI (and library) for verifying [INVOLUTE](https://involute.chandler.io) competitive solve recordings.

This workspace contains:

| Crate | Role |
|-------|------|
| [`involute-verify`](crates/involute-verify/) | Verifier binary + library |
| [`timecheck`](crates/timecheck/) | drand + RFC 3161 helpers (vendored from Hyperspeedcube) |

Puzzle definitions are downloaded at runtime from `https://involute.chandler.io/puzzles/{id}/{version}/` and cached under `~/.cache/involute-verify`.

## Build

```bash
cargo build -p involute-verify --release
```

Requires OpenSSL on `PATH` at runtime.

## Usage

```bash
cargo run -p involute-verify -- path/to/recording.json
```

See [`crates/involute-verify/README.md`](crates/involute-verify/README.md) for cache layout, flags, and trust model.

## Spec

Recording wire format: [`docs/solve-recording.md`](docs/solve-recording.md).

## Attribution

`timecheck` is vendored from [Hyperspeedcube](https://github.com/HactarCE/Hyperspeedcube)
by **Andrew Farkas** ([HactarCE](https://github.com/HactarCE)), with small TSA
extensions for moda multi-CA verification. Details:
[`crates/timecheck/README.md`](crates/timecheck/README.md).

The underlying timing scheme is described by
[5225225](https://www.5snb.club/posts/2025/cheating-resistant-racing-games/).

## License

MIT OR Apache-2.0
