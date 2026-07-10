# timecheck

Scheme for cheating-resistant deterministic games using randomness beacons and time stamp authorities

Vendored from [Hyperspeedcube](https://github.com/HactarCE/Hyperspeedcube)'s
[`crates/timecheck`](https://github.com/HactarCE/Hyperspeedcube/tree/main/crates/timecheck),
originally written by **Andrew Farkas** ([HactarCE](https://github.com/HactarCE)).

This is based on [a scheme for cheating-resistant racing games](https://www.5snb.club/posts/2025/cheating-resistant-racing-games/) by [5225225](https://www.5snb.club/).

Hyperspeedcube / `timecheck` is licensed **MIT OR Apache-2.0**, same as this
workspace.

## Modifications in this repository

Relative to upstream Hyperspeedcube `timecheck`, this fork changes only the TSA
path (`src/tsa.rs`) so Involute can verify stamps from
[rfc3161.ai.moda](https://rfc3161.ai.moda) (multi-CA, `certReq` chains):

1. **`Tsa::with_pem`** — construct a verifier with an arbitrary CA PEM bundle
   (not only the pinned FreeTSA constant).
2. **`Tsa::verify_with_untrusted`** — pass openssl `-untrusted` for intermediates
   embedded in a Time Stamp Response.
3. **`Tsa::verify_with_options`** — optional openssl `-partial_chain` when the
   trusted PEM contains intermediates used as anchors.
4. **`Tsa::verify`** — thin wrapper that calls `verify_with_untrusted(..., None)`
   (behavior unchanged for FreeTSA-style verifies).
5. Lint attribute tweak: `#[expect(missing_docs)]` → `#[allow(missing_docs)]`
   for broader rustc compatibility.

