# UTA — Agent Guide

You are building **UTA**: a dynamically typed language in Rust that runs `.web` scripts, serves local HTML, and renders HTML via tag macros.

## Source of truth
1. **`README.md`** — language syntax and builtins (keep in sync with code).
2. **`AGENTS.md`** (this file) — engine architecture and agent workflow.
3. **`.cursor/rules/uta.mdc`** — Cursor rules (always applied in this repo).

If behavior is missing from the spec, stop and ask:
`[Spec Ambiguity Encountered]: <gap>. Please provide guidance.`

## Architecture (mandatory)

```
.web file → Lexer → Parser → AST → Evaluator → builtins / HTTP
```

| Module | Role |
|--------|------|
| `src/lexer.rs` | Char-by-char tokens. No regex on source. |
| `src/parser.rs` | Recursive descent; blocks end with `end;` |
| `src/ast.rs` | `Statement`, `Expression`, `RuntimeValue` |
| `src/eval.rs` | Environment, builtins, HTTP server |
| `src/main.rs` | CLI: validate `.web`, parse, run |

## Implemented builtins (use in scripts/tests)

```uta
start_server(port: <expr>, file_descriptor: <expr>, is_project: <expr> [, time_out: <expr>]);
end_server(port: <expr>);
time_out(time: <expr>);
sleep(time: <expr>);
start_coroutine(routine_name: <expr>, func: <expr>);
end_coroutine(routine_name: <expr>);
```

- Arguments are **always named** (`port: 8080`, not positional).
- `time_out` on `start_server`: milliseconds; `0` or omitted = no auto-stop.
- `file_descriptor`: path relative to the **script file's folder**.
- Unknown `name(...)` calls → HTML tag macro (`h1("text")` → `<h1>text</h1>`).

## Variables
- `const name = <expr>;` — immutable binding.
- Variables used before definition → runtime error (not silent null).
- Lookup: child scope → parent chain (`Arc<Mutex<Environment>>`).

## Constraints checklist
- [ ] Reject non-`.web` files at CLI.
- [ ] Parse and execute the **actual file contents** (no mock AST).
- [ ] `sleep` / `start_coroutine` on background threads.
- [ ] `start_server` binds in a background thread; main waits until server stops.
- [ ] `SO_REUSEADDR` + stop in-process server on same port before rebind.
- [ ] Verify `index.html` (or given path) exists before binding.

## Verification workflow (run before saying "fixed")

```powershell
cd D:\practice\uta
cargo build
cargo test
# Free port 8080 if a previous run is still active:
netstat -ano | findstr :8080
cargo run -- script.web
# Browser: http://127.0.0.1:8080 — expect "Hallo" from index.html
```

## Common failures

| Symptom | Cause | Fix |
|---------|--------|-----|
| Port already in use | Previous `cargo run` still running | Kill PID from `netstat` or `Stop-Process` |
| Could not read index.html | Wrong cwd or path | Use path relative to `.web` file; run from repo root |
| Undefined variable | Typo or const not defined yet | Define `const` before use |
| Parse error on `:` | Named arg parsing bug | Use `port: value` syntax; see parser tests |

## Interaction mode
- Outline plan before large structural changes.
- Prefer minimal, focused diffs.
- After engine changes, update `README.md`, sample `.web` scripts, and tests together.
