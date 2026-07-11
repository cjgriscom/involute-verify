# Solution STM

`solution_stm` is the net [Slice Turn Metric](https://hypercubing.xyz/notation/#turn-metrics) move count reported by verification for a solve recording. It is recomputed by the verifier from the commited events.

## Algorithm

1. **Apply undos** over the committed events, ignoring non-move events (`showSolved`, highlighter, animation speed, inspection/clock markers, `solved`, etc.):
   - `move` — append the move name to a sequence
   - `undo` — remove the last move from that sequence (no-op if empty)
2. **Collapse consecutive identical moves** by walking the sequence with a stack:
   - if the new move equals the top of the stack, pop (the pair cancels)
   - otherwise push
3. **`solution_stm`** is the length of the resulting stack.

Each distinct move is always an involution, so they are self-canceling. 

## Examples

| Move sequence | After collapse | `solution_stm` |
|---------------|----------------|----------------|
| `B A A C` | `B C` | 2 |
| `B A A A C` | `B A C` | 3 |
| `B A A B C` | `C` | 1 |

Walk-through for `B A A B C`:

1. push `B` → `[B]`
2. push `A` → `[B, A]`
3. `A` matches top → pop → `[B]`
4. `B` matches top → pop → `[]`
5. push `C` → `[C]` → length **1**

## Implementation

Implemented by `count_solution_stm` in `crates/involute-verify/src/verify.rs`, and exposed as [`SolveVerification::solution_stm`](../crates/involute-verify/src/verification.rs).
