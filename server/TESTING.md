# Test coverage

Run all server integration tests, the headless TUI smoke, or the entire workspace.
Scheduler tests are included in the P0 integration coverage without a separate
execution priority.

```sh
cargo test --workspace --locked integration_tests
cargo test --workspace --locked tests::workflow
cargo test --workspace --locked
```

`src/tests/` contains 34 workflow tests grouped by subject. Importing the actual
TUI API module also runs its existing 8 client unit tests in the server target.
The TUI has one additional headless workflow smoke test in
`../tui/src/tests/workflow.rs`.

## Coverage audit

| Core functionality / agreed scenario | Coverage |
| --- | --- |
| P0/P1 schedule start, stop, refill, next occurrence, manual suppression | `scheduler.rs`: exact boundaries and ordered starts/resumes |
| P0/P1 restart catch-up, expired deadlines, schedule edits, weekly/overnight windows | `scheduler.rs`: persisted state and injected clock; existing scheduler unit tests also cover DST calculations |
| P0/P2 schedule RPC failures, lost pause responses, overlapping controls, boundary crossed during refill | `scheduler.rs`: stateful fault injection and response barriers |
| P0 download add/defer/start, progress, pause/resume, completion and retry | `download.rs`: HTTP, poller, SQLite, GIDs, timestamps and artifacts |
| P0/P1/P2 queue advancement, pause/resume, reorder, move and restart | `queue.rs`: execution order, membership and capacity |
| P0/P1 queue deletion, confirmation and main-queue protection | `queue.rs` and `client.rs`: live state/metadata cleanup with payload preservation |
| P0 deletion with/without files and collision protection | `download.rs`; existing deletion unit tests cover missing files and filesystem errors |
| Core filtered listings and completed-history cleanup | `download.rs`: scoped cleanup preserves pending work, other queues and payloads |
| P1 signed URL category routing, default/override options and finetune edits | `sources.rs`: routing, actual RPC options and persisted edits |
| P1 torrent multipart save/restart/start | `sources.rs`: saved metainfo reaches `addTorrent`, completion is reconciled |
| P1 magnet metadata handoff | `sources.rs`: original record adopts child GID, routes destination and completes without duplicate records |
| P1 managed/unmanaged daemon lifecycle | `lifecycle.rs`: idle blockers, forced shutdown, ownership and rejection of mutations after shutdown |
| P2 aria2 availability and recovery | `lifecycle.rs`: health degradation and poller recovery preserve progress/GID without duplicate additions |
| P2 engine process supervision | Existing `aria2_process.rs` tests: owned-child termination, respawn backoff and fast-exit retry limits; TUI daemon tests cover startup/readiness/recovery |
| P2 concurrent resume/delete/poll | `download.rs`: deleted records and live entries cannot be resurrected |
| P2 malformed input, missing IDs, malformed/oversized torrent uploads | `validation.rs`: HTTP errors without unintended records or transfers |
| Core TUI API contracts | `client.rs`: real server success/error decoding and queue confirmation outcomes |
| P2 TUI workflow smoke | TUI `tests/workflow.rs`: effect worker, HTTP client, app update and TestBackend rendering through add/progress/pause/resume/delete/error |
| Clipboard import, modal navigation, key routing and responsive UI | Existing TUI unit/render tests; headless smoke provides cross-component coverage for the main download workflow |

The agreed P0, P1 and P2 application scenarios are covered at the levels above.
This is a coverage inventory, not a claim that every possible input or external
integration has been verified.

## Fixture and limits

The server fixture uses production HTTP routes/middleware, a temporary SQLite
file, an injected scheduling clock, and one full production poller iteration per
tick. Only aria2 is replaced: an authenticated stateful JSON-RPC HTTP mock records
calls and simulates progress, completion, URI/torrent starts, magnet resolution,
RPC/transport failures, lost pause responses and barrier-controlled delays.

Synthetic payload/control files verify artifact ownership and deletion; they do
not verify real downloading, peer discovery or byte integrity. Restart tests
reopen Ario while retaining mock transfers. They do not demonstrate actual aria2
session restoration after both processes restart. Engine process supervision is
covered separately with controlled child processes, not a real aria2 executable.

The headless TUI smoke uses a scripted HTTP server and the real app/renderer;
server compatibility is tested separately against actual Ario routes. It does
not verify physical terminal input/raw-mode setup, OS clipboard integration or
opening files in external applications. No real aria2 installation or external
network is required; tests require permission to bind loopback listeners.
