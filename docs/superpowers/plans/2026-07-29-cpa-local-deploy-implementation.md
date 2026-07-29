# CPA Local Deploy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a POSIX `sh` deployment repository that installs and manages CPA and cpa-usage-keeper as macOS Apple Silicon user LaunchAgents.

**Architecture:** Shell scripts in `macos/` source small, testable library files. Runtime state lives only in `$HOME/Library/Application Support/cpa-stack`; generated launchd wrappers and plists reference stable runtime paths while release activation uses atomically replaced `current/` symlinks.

**Tech Stack:** POSIX `sh`, macOS `launchctl`/`plutil`, `curl`, `tar`, `shasum`, `sqlite3`, shell test harness.

## Global Constraints

- Support only `Darwin` with `arm64`; fail before any mutation on another platform.
- Do not require zsh, Homebrew, Docker, sudo, or third-party test dependencies.
- Default-bind CPA to `127.0.0.1:8317` and Keeper to `127.0.0.1:18080`.
- Never commit, copy, or overwrite real credentials, auth files, databases, logs, archives, or release binaries.
- Require verified official GitHub Release checksums before activation.
- Services are user LaunchAgents named `io.cpa-local.cli-proxy-api` and `io.cpa-local.usage-keeper`.
- All state-changing scripts must be idempotent.

---

### Task 1: Repository skeleton and test harness

**Files:**
- Create: `.gitignore`, `README.md`, `config/cpa.config.yaml.example`, `config/keeper.env.example`
- Create: `tests/test_helper.sh`, `tests/test_common.sh`, `tests/run.sh`

**Interfaces:**
- Produces `tests/run.sh`, which runs every `tests/test_*.sh` with POSIX `sh` and exits nonzero on assertion failure.

- [ ] **Step 1: Write failing test harness expectations**

```sh
# tests/test_common.sh
. "$(dirname "$0")/test_helper.sh"
assert_command_fails sh "$ROOT/macos/lib/common.sh"
```

- [ ] **Step 2: Run the test and verify it fails because the library is absent**

Run: `sh tests/run.sh`
Expected: failure mentioning `macos/lib/common.sh`.

- [ ] **Step 3: Create minimal repository files and harness**

`test_helper.sh` defines `assert_eq`, `assert_contains`, `assert_command_fails`, temporary-directory setup and cleanup. `tests/run.sh` finds and executes `test_*.sh`. Templates contain only safe defaults and `__REQUIRED__` for required secrets.

- [ ] **Step 4: Run the harness**

Run: `sh tests/run.sh`
Expected: PASS after Task 2 creates the library.

- [ ] **Step 5: Commit**

```bash
git add .gitignore README.md config tests
git commit -m "chore: scaffold local deployment repository"
```

### Task 2: Common library, configuration validation, and runtime bootstrap

**Files:**
- Create: `macos/lib/common.sh`, `macos/lib/config.sh`
- Modify: `tests/test_common.sh`
- Create: `tests/test_config.sh`

**Interfaces:**
- Produces `require_macos_arm64`, `require_commands`, `runtime_root`, `ensure_runtime_layout`, `validate_private_file`, `require_no_placeholder`, and `validate_config`.
- Consumes `CPA_STACK_ROOT` as an optional test/install root override.

- [ ] **Step 1: Write failing platform/layout tests**

```sh
assert_command_fails env UNAME_S=Linux UNAME_M=x86_64 sh -c '. macos/lib/common.sh; require_macos_arm64'
assert_eq 700 "$(stat -f '%Lp' "$TMP_ROOT/keeper")"
```

- [ ] **Step 2: Verify the failures**

Run: `sh tests/run.sh`
Expected: missing functions and runtime layout failures.

- [ ] **Step 3: Implement minimal POSIX helpers**

Use injectable `UNAME_S`/`UNAME_M` for tests, defaulting to `uname`. `ensure_runtime_layout` sets `umask 077`, creates all specified directories, and applies `600`/`700`. `validate_config` rejects missing files, `__REQUIRED__`, invalid key ports, and inappropriate permissions.

- [ ] **Step 4: Run all tests**

Run: `sh tests/run.sh`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add macos/lib tests
git commit -m "feat: add runtime and config validation helpers"
```

### Task 3: Verified release installation and atomic activation

**Files:**
- Create: `macos/lib/releases.sh`
- Create: `tests/test_releases.sh`

**Interfaces:**
- Produces `install_release SERVICE VERSION ARCHIVE CHECKSUM_FILE` and `activate_release SERVICE VERSION`.
- `install_release` returns nonzero without changing `current/SERVICE` when checksum, archive layout, or binary self-check fails.

- [ ] **Step 1: Write failing activation tests**

```sh
old=$(readlink "$TMP_ROOT/current/cli-proxy-api")
assert_command_fails install_release cli-proxy-api 1.2.3 "$BAD_ARCHIVE" "$BAD_SUMS"
assert_eq "$old" "$(readlink "$TMP_ROOT/current/cli-proxy-api")"
```

- [ ] **Step 2: Verify red**

Run: `sh tests/test_releases.sh`
Expected: command/function missing.

- [ ] **Step 3: Implement download-independent core**

Parse exact expected checksum line, verify with `shasum -a 256`, extract to a temporary destination, validate executable and `--help`, then replace a temporary symlink with `mv`. Keep GitHub API fetching in a separate `fetch_latest_release SERVICE` function with fixed repositories and asset patterns.

- [ ] **Step 4: Verify green**

Run: `sh tests/run.sh`
Expected: PASS, including checksum mismatch preserving the old link.

- [ ] **Step 5: Commit**

```bash
git add macos/lib/releases.sh tests/test_releases.sh
git commit -m "feat: install verified CPA releases atomically"
```

### Task 4: Launchd templates, runtime wrappers, and lifecycle libraries

**Files:**
- Create: `macos/launchd/*.plist.in`, `macos/lib/launchd.sh`, `macos/lib/lifecycle.sh`
- Create: `tests/test_launchd.sh`, `tests/test_lifecycle.sh`

**Interfaces:**
- Produces `render_plists`, `bootstrap_service`, `kickstart_service`, `stop_service`, `set_disabled`, `clear_disabled`, and runtime wrapper files at `bin/run-cli-proxy-api` / `bin/run-cpa-usage-keeper`.

- [ ] **Step 1: Write failing lifecycle/template tests**

```sh
set_disabled cli
assert_file "$TMP_ROOT/state/cli.disabled"
clear_disabled cli
assert_file_absent "$TMP_ROOT/state/cli.disabled"
render_plists
plutil -lint "$TMP_PLIST"
```

- [ ] **Step 2: Verify red**

Run: `sh tests/test_launchd.sh && sh tests/test_lifecycle.sh`
Expected: missing functions/templates.

- [ ] **Step 3: Implement wrappers and plists**

Wrappers exit 0 when their disabled marker exists, otherwise `exec` the current binary with its config argument. Plists use `RunAtLoad`, `ThrottleInterval=10`, `KeepAlive` with `SuccessfulExit=false`, labels from the global constraints, and runtime log paths. Abstract `launchctl` behind `LAUNCHCTL` for dry-run tests.

- [ ] **Step 4: Verify green**

Run: `sh tests/run.sh`
Expected: PASS and rendered plists pass `plutil -lint` on macOS.

- [ ] **Step 5: Commit**

```bash
git add macos/launchd macos/lib tests
git commit -m "feat: add launchd lifecycle support"
```

### Task 5: User commands and end-to-end dry-run behavior

**Files:**
- Create: `macos/install.sh`, `macos/start.sh`, `macos/stop.sh`, `macos/restart.sh`, `macos/status.sh`, `macos/update.sh`, `macos/uninstall.sh`
- Create: `tests/test_commands.sh`

**Interfaces:**
- `install.sh [--init]`, `start.sh [cli|keeper]`, `stop.sh [cli|keeper]`, `restart.sh [cli|keeper]`, `status.sh`, `update.sh [cli|keeper] [--rollback VERSION]`, `uninstall.sh [--purge]`.

- [ ] **Step 1: Write failing CLI target and dry-run tests**

```sh
assert_command_fails sh macos/start.sh unknown
assert_contains "first run install.sh" "$(CPA_STACK_ROOT="$TMP_ROOT" sh macos/start.sh cli 2>&1)"
assert_contains "cli: disabled" "$(CPA_STACK_ROOT="$TMP_ROOT" sh macos/status.sh 2>&1)"
```

- [ ] **Step 2: Verify red**

Run: `sh tests/test_commands.sh`
Expected: scripts absent.

- [ ] **Step 3: Implement commands minimally**

`install.sh --init` creates configuration only when absent and reads secrets with `stty -echo` restored by trap. Plain install validates pre-created files. `update.sh` backs up Keeper using `sqlite3 .backup` before activation. Commands honor `CPA_STACK_ROOT`, `LAUNCHCTL`, and `CPA_DRY_RUN=1` for tests. Purge requires a literal interactive `DELETE` confirmation and refuses under dry-run.

- [ ] **Step 4: Verify green**

Run: `sh tests/run.sh && find macos -name '*.sh' -exec sh -n {} +`
Expected: all tests and syntax checks PASS.

- [ ] **Step 5: Commit**

```bash
git add macos tests
git commit -m "feat: add macOS CPA service commands"
```

### Task 6: Documentation, full verification, and local acceptance guide

**Files:**
- Modify: `README.md`
- Create: `tests/test_readme.sh`

- [ ] **Step 1: Write failing documentation test**

```sh
assert_contains 'sh macos/install.sh --init' "$(cat README.md)"
assert_contains 'uninstall.sh --purge' "$(cat README.md)"
```

- [ ] **Step 2: Verify red**

Run: `sh tests/test_readme.sh`
Expected: required user instructions absent.

- [ ] **Step 3: Write concise operator documentation**

Document prerequisites, both initialization paths, launchd behavior, start/stop/restart/status/update/rollback/uninstall commands, manual migration, local-only security defaults, sensitive-data rules, test command, and an explicit local acceptance checklist.

- [ ] **Step 4: Run verification**

Run: `sh tests/run.sh && find macos tests -name '*.sh' -exec sh -n {} + && git diff --check`
Expected: all commands exit 0.

- [ ] **Step 5: Commit**

```bash
git add README.md tests/test_readme.sh
git commit -m "docs: document local CPA deployment"
```
