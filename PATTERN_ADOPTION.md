# PATTERN_ADOPTION

ap-kit is a pure accounting crate: supplier bills, input VAT recoverability,
and the three-way posting split. It has no network, storage, agent, or
concurrency surface, so capability, limit, and sandbox patterns are omitted
under the format's rule 6. Checked with
`pattern-standards/scripts/adoption-check.py`.

| Pattern | Status | Evidence |
|---|---|---|
| T27 | adopted | tests/ledger.rs:50 (every split posts to a real double_entry ledger and must balance), src/bill.rs:406 (identity pinned in tests: expensed() plus deductible_tax equals payable()) |
| P11 | adopted | src/recoverability.rs:121 (the non-deductible share is tax minus deductible, never an independent second calculation), src/bill.rs:573 (conservation pinned across mixed cuts) |
| T1 | partial | src/recoverability.rs:33 (category vocabulary lives in enums; arithmetic paths never re-check it), src/bill.rs:266 (cross-field rules gate at validate before issue; fields stay plain data) |
