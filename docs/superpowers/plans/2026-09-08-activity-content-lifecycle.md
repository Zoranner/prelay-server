# Activity Content Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every activity content record leave its in-flight state through one lifecycle, while preserving partial text for failed streams and preventing new permanent `capturing` rows.

**Architecture:** `StreamRecordState` remains the owner of stream observation and activity timing, but content finalization becomes one idempotent operation used by successful completion, upstream stream errors, empty streams, and downstream cancellation. The storage layer exposes an explicit terminal transition that writes the captured draft as `pending`; it does not infer success from content state. Existing historical rows are handled by a versioned startup data migration and are never reconstructed with invented text.

**Tech Stack:** Rust 2021, Tokio streams, Axum response bodies, SeaORM, PostgreSQL/SQLite test storage, existing activity content normalization and redaction.

**Spec:** The approved chat design in the current task: unify activity/content lifecycle, preserve partial content on terminal failures, keep `capturing` only for active streams, and repair historical stale rows separately.

## Global Constraints

- Keep `identity_activities` as the activity summary source and `activity_contents` as the content source; do not add a parallel activity entity.
- Keep `pending` as the post-capture content state used by the existing downstream processing path.
- Do not fabricate missing historical input or output text.
- Do not modify the production database during implementation or tests.
- Preserve existing protocol response bytes and activity status semantics.
- Keep credentials and provider keys out of diagnostics, tests, logs, and generated SQL.
- Run `cargo fmt --all` and `cargo clippy --all-targets --all-features -- -D warnings` after Rust changes; report unrelated existing blockers without masking them.

### Task 1: Define the terminal content transition

**Files:**
- Modify: `src/storage/activity_contents.rs`
- Modify: `src/storage/recordings.rs`
- Test: `tests/activity_contents.rs`

**Interfaces:**
- Consumes: `ActivityContentDraft` produced by the existing normalization pipeline.
- Produces: one storage operation that updates an existing activity content row to `pending`, or inserts it as `pending` when the row was not created.

- [ ] **Step 1: Write failing storage tests**

Add tests that create an activity content row as `capturing`, complete it with a draft, and assert that all content fields are replaced and the status becomes `pending`. Add a second test that completes a missing row and asserts that exactly one `pending` row is inserted.

- [ ] **Step 2: Run the focused storage tests**

Run:

```text
cargo test --test activity_contents
```

Expected: the new transition assertions fail before the implementation changes.

- [ ] **Step 3: Implement the explicit terminal transition**

Keep insertion and completion in the storage content module, but make the terminal operation the only path that changes `capturing` to `pending`. Preserve `activity_id` uniqueness and update the existing row atomically.

- [ ] **Step 4: Run the focused storage tests again**

Run:

```text
cargo test --test activity_contents
```

Expected: all activity content storage tests pass.

### Task 2: Unify stream lifecycle finalization

**Files:**
- Modify: `src/observability/stream_stats/state.rs`
- Modify: `src/observability/stream_stats/tests.rs`

**Interfaces:**
- Consumes: stream items, `StreamStatsSnapshot`, optional `RawStreamContentCapture`, and the activity identifier owned by `StreamRecordState`.
- Produces: one terminal finalization path that updates the activity summary when needed and always attempts to move persisted content out of `capturing`.

- [ ] **Step 1: Write failing lifecycle tests**

Add tests for:

```text
record_stream_with_activity_content_preserves_partial_content_after_upstream_error
record_first_chunk_with_activity_content_finishes_empty_stream_content
record_stream_with_activity_content_finishes_content_when_activity_status_is_failed
```

Each test must consume the returned stream to its terminal state and assert the stored content status is `pending`, the input text is retained, and the captured output is retained up to the observed boundary.

- [ ] **Step 2: Run the focused stream tests**

Run:

```text
cargo test observability::stream_stats --lib
```

Expected: the new failure-path tests fail because `record_stream_end` currently returns when `self.failed` and the empty-stream path leaves `capturing`.

- [ ] **Step 3: Implement one terminal finalizer**

Add a private finalization method on `StreamRecordState` that:

1. Is safe to call once from any terminal path.
2. Finishes the optional protocol capture.
3. Combines captured input and output through the existing normalization policy.
4. Propagates capture incompleteness into `is_truncated`.
5. Calls the storage terminal transition even when the activity status is `failed`.
6. Logs storage failure with the existing sanitized stream-storage event.

Change `record_stream_end` so activity success updates occur only for non-failed streams, while content finalization always runs for inserted streams. Change the empty-stream path to use the same finalizer after creating the failed activity record.

- [ ] **Step 4: Run the focused stream tests**

Run:

```text
cargo test observability::stream_stats --lib
```

Expected: all stream lifecycle tests pass and existing protocol bytes remain unchanged.

### Task 3: Cover downstream cancellation and stale in-flight rows

**Files:**
- Modify: `src/observability/stream_stats/state.rs`
- Modify: `src/observability/stream_stats/record.rs`
- Modify: `src/storage/activity_contents.rs`
- Create: `docs/operations/activity-content-recovery.sql`
- Test: `tests/activity_contents.rs`

**Interfaces:**
- Consumes: the stream wrapper lifecycle and the existing `activity_contents` timestamps.
- Produces: a bounded recovery operation for rows whose activity is already terminal and content is still `capturing`; no fabricated content.

- [ ] **Step 1: Write failing cancellation and recovery tests**

Add a test that drops a partially consumed recording and verifies the lifecycle contract used by the implementation. Add a storage query test that selects only `capturing` rows whose parent activity is terminal and older than the configured recovery threshold.

- [ ] **Step 2: Run the focused tests**

Run:

```text
cargo test --test activity_contents
cargo test observability::stream_stats --lib
```

Expected: the new recovery-selection assertion fails before the query/helper exists.

- [ ] **Step 3: Implement cancellation-safe ownership**

Ensure the recorder owns a terminal state marker and does not report a stream as complete until the underlying stream reaches EOF or yields an error. For downstream cancellation, preserve the already persisted `capturing` row for the recovery query rather than claiming a successful finalization without observing the terminal upstream state.

This task must not spawn an unbounded background task from `Drop` and must not attempt asynchronous database work from `Drop`.

- [ ] **Step 4: Add the versioned startup data migration**

Create `src/schema/activity_content.rs` and call it from `schema::initialize`. Use the existing `prelay_schema_migrations` table with version `activity_content_lifecycle_v1`. Run the update in one transaction for both PostgreSQL and SQLite, changing only `capturing` rows whose parent activity is `success` or `failed` and whose `updated_at` is older than five minutes. Clear active lease fields, preserve all stored text and hashes, and insert the migration version only after the update succeeds.

- [ ] **Step 5: Run focused tests and migration static checks**

Run:

```text
cargo test --test activity_contents
cargo test observability::stream_stats --lib
rg -n "activity_content_lifecycle_v1|UPDATE activity_contents|capturing|pending|activity_id" src/schema/activity_content.rs
```

Expected: tests pass and the migration contains only the bounded recovery predicates.

### Task 4: Verify all production stream entry points

**Files:**
- Inspect: `src/routes/v1/chat/candidate.rs`
- Inspect: `src/routes/v1/messages/chat.rs`
- Inspect: `src/routes/v1/messages/native.rs`
- Inspect: `src/routes/v1/messages/responses.rs`
- Inspect: `src/routes/v1/responses/chat.rs`
- Inspect: `src/routes/v1/responses/native.rs`
- Inspect: `src/routes/v1/responses/anthropic.rs`
- Inspect: `src/routes/v1/images/candidate.rs`
- Modify: relevant route tests only if a lifecycle gap is proven

**Interfaces:**
- Consumes: the unified stream recorder from Task 2.
- Produces: evidence that each streaming protocol uses the same terminal lifecycle or an explicit documented exception for best-effort image logging.

- [ ] **Step 1: Enumerate every stream recorder call**

Run:

```text
rg -n "record_stream|record_first_chunk" src/routes src/observability
```

Confirm that every streaming route is mapped to one recorder and that no route directly inserts a `capturing` row.

- [ ] **Step 2: Add only route-level regression tests for proven gaps**

Use existing route fixtures and assert both the activity status and content status after success, upstream interruption, and empty response where the route can produce those cases.

- [ ] **Step 3: Run route-focused verification**

Run the smallest relevant existing route test targets and record any failures caused by unrelated provider catalog work separately.

### Task 5: Full verification and database recheck

**Files:**
- Modify: none unless verification exposes a task-scoped defect.

- [ ] **Step 1: Run Rust formatting**

Run:

```text
cargo fmt --all
cargo fmt --all -- --check
```

- [ ] **Step 2: Run focused and full tests**

Run:

```text
cargo test --test activity_contents
cargo test observability::stream_stats --lib
cargo test --all-targets --all-features
```

Report failures by whether they are task-scoped or pre-existing workspace failures.

- [ ] **Step 3: Run strict Clippy**

Run:

```text
cargo clippy --all-targets --all-features -- -D warnings
```

Do not add `#[allow]` for unrelated warnings.

- [ ] **Step 4: Recheck the temporary PostgreSQL database after deployment**

Using the already confirmed connection parameters, run read-only aggregate SQL before any recovery update:

```text
SELECT status, COUNT(*) FROM activity_contents GROUP BY status ORDER BY status;
SELECT a.status, COUNT(*) FROM identity_activities a LEFT JOIN activity_contents c ON c.activity_id = a.id WHERE c.status = 'capturing' GROUP BY a.status;
```

The new server startup migration performs the bounded update automatically. Verify the migration version and post-startup counts; do not run a second manual update.

- [ ] **Step 5: Review the final diff**

Run:

```text
git diff --check
git diff --stat -- src/observability/stream_stats src/storage/activity_contents.rs src/storage/recordings.rs tests/activity_contents.rs docs/operations/activity-content-recovery.sql
```

Confirm no unrelated user changes are staged, reverted, or included.
