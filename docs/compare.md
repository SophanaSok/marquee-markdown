# Ledger

A small library for append-only logs. An entry is written once and read back by
replaying the file from the start.

> Written for a talk. The API is illustrative, not shipping.

> [!NOTE]
> Entries are never rewritten in place. Compaction writes a new file and swaps
> it in.

## Opening a log

```rust
pub fn open(path: &Path, mode: Mode, cache: Cache) -> Result<Ledger, OpenError> {
    Ledger::with_capacity(path, mode, cache, DEFAULT_CAPACITY)
}
```

| Mode | Reads | Writes |
| --- | --- | --- |
| `Replay` | yes | no |
| `Append` | yes | yes |

---

- Entries are addressed by byte offset, never by index.
- `Ledger::flush` returns once the write has reached the disk.
- The [on-disk format](https://example.org/ledger/format) is documented separately.
