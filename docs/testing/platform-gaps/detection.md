# Backend detection and instance selection

## Scope

Table 2 row: **Backend detection and instance selection**.

The assigned gap is remote instance selection through SSH with conflicting inherited environment settings. No other platform row is changed.

## Cases added

1. Override inherited `WORKMUX_BACKEND=tmux` and `HERDR_SOCKET_PATH` with `herdr` and the requested instance. Conflicting tmux, WezTerm, Zellij, Kitty, and Herdr pane settings remain present. Both backend detection functions must select Herdr. The backend must resolve and read the requested socket.
2. Select an explicit Herdr instance through `create_backend_for_instance`, while backend detection still selects the inherited tmux setting. The explicit instance must take precedence over the inherited Herdr socket.
3. Select a missing explicit instance while a valid inherited Herdr instance exists. Both instance resolution and the running check must fail. There must be no fallback to the inherited instance.

The controlled tests count requests: the inherited socket must receive zero requests. Each successful selection must read the selected socket. The missing-instance case must read neither socket.

## Test files

- `src/multiplexer/herdr/remote_detection_tests.rs`: three normal regression tests and one ignored, process-isolated probe.
- `src/multiplexer/herdr/integration/detection_checks.py`: standalone real SSH/live Herdr runner for all three cases.
- `src/multiplexer/herdr/mod.rs`: two-line test module registration.

The ignored probe fails if required inputs are absent. It does not silently pass without its runner.

## Validation

Run from the repository root after the session resumed:

```sh
CARGO_BUILD_JOBS=1 cargo test detection_tests -- --nocapture
```

Exit 0. **4 passed; 0 failed; 1 ignored; 1774 filtered out.** This includes the existing backend precedence/nested-tmux test and all three new controlled tests. The ignored entry is the runner-only probe; each controlled test invokes it in a separate process. The build finished in 44.01 seconds.

```sh
python3 src/multiplexer/herdr/integration/detection_checks.py --test-binary target/debug/deps/workmux-3c4143229c1a27ae
```

Exit 0. Exact output:

```text
PASS live Herdr / loopback SSH: override
PASS live Herdr / loopback SSH: explicit
PASS live Herdr / loopback SSH: missing
3 passed (live Herdr, real loopback SSH; no terminal UI operations)
```

This run also invoked the ignored Rust probe for each of the three cases. For a later build, use the test executable path printed by Cargo; its hash can change.

Additional checks:

```sh
ruff check src/multiplexer/herdr/integration/detection_checks.py
ruff format --check src/multiplexer/herdr/integration/detection_checks.py
rustfmt --edition 2024 --check src/multiplexer/herdr/remote_detection_tests.rs
python3 -m py_compile src/multiplexer/herdr/integration/detection_checks.py
git diff --check
```

All passed. Ruff reported `All checks passed!` and `1 file already formatted`. The other checks produced no errors.

The earlier two-job build did not establish a result. Its first attempt timed out, and the coordinator later terminated the background build because of host load. The results above come from the new one-job build and subsequent tests. No production defect was exposed, and no production behavior was changed.

## Evidence limits and safety

**Controlled evidence:** Rust subprocess tests use two private protocol-22 Unix socket servers. These are controlled API responses, not live terminal evidence. The child environment is cleared before test-owned values are set.

**Live evidence:** The Python runner uses a private loopback OpenSSH daemon, temporary host/client keys, pinned host-key verification, and two real disposable Herdr servers. `SendEnv`/`AcceptEnv` transfers the conflicting values across SSH. The Rust probe checks the conflicts and `SSH_CONNECTION` before selection. The inherited live server snapshot must remain unchanged.

The runner binds SSH only to `127.0.0.1`. It disables password authentication and forwarding. It does not use an existing SSH daemon. All Herdr paths and state directories belong to the fixture. Cleanup stops only test-owned processes. No user workspace, existing tab, or global configuration is changed.

No terminal client is attached, and no terminal layout, focus, or pane operation is tested. This is instance-selection evidence, not proof of live terminal behavior.

## Remaining gaps

- SSH to a separate physical or virtual host, with a different filesystem or operating system, is not tested.
- Interactive SSH login, user-specific startup scripts, and SSH servers that reject environment forwarding are not tested.
- The full test suite was not run. Validation was limited to the assigned row.
- There is no remaining runtime blocker for the tested loopback SSH cases. No separate-host SSH target was used.

## Proposed Table 2 cells

- **Tested:** Backend precedence; nested tmux; explicit instance selection; controlled conflicting-environment selection and no fallback; real loopback SSH selection with forwarded conflicting settings against isolated live Herdr instances.
- **Untested:** Separate-host SSH; interactive login/startup-script effects; servers that reject environment forwarding.
- **Percentage:** 95%, subject to coordinator review.
- **Coverage marker:** `◐` (`class="unknown"`). Do not claim 100%.
