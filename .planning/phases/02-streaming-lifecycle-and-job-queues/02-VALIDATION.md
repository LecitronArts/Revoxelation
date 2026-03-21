---
phase: 2
slug: streaming-lifecycle-and-job-queues
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-21
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`cargo test`) |
| **Config file** | `Cargo.toml` — `[dev-dependencies]` |
| **Quick run command** | `cargo test -p revoxelation 2>&1 | tail -20` |
| **Full suite command** | `cargo test -p revoxelation -- --test-threads=1 2>&1` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p revoxelation 2>&1 | tail -20`
- **After every plan wave:** Run `cargo test -p revoxelation -- --test-threads=1 2>&1`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 2-01-01 | 01 | 1 | STRM-01 | unit | `cargo test chunk_coord` | ❌ W0 | ⬜ pending |
| 2-01-02 | 01 | 1 | STRM-01 | unit | `cargo test octree` | ❌ W0 | ⬜ pending |
| 2-01-03 | 01 | 1 | STRM-01 | unit | `cargo test sse` | ❌ W0 | ⬜ pending |
| 2-01-04 | 01 | 1 | STRM-02 | unit | `cargo test chunk_state` | ❌ W0 | ⬜ pending |
| 2-01-05 | 01 | 1 | STRM-02 | unit | `cargo test revision_id` | ❌ W0 | ⬜ pending |
| 2-02-01 | 02 | 2 | STRM-03 | unit | `cargo test job_queue` | ❌ W0 | ⬜ pending |
| 2-02-02 | 02 | 2 | STRM-03 | unit | `cargo test cancel` | ❌ W0 | ⬜ pending |
| 2-02-03 | 02 | 2 | STRM-03 | unit | `cargo test backpressure` | ❌ W0 | ⬜ pending |
| 2-02-04 | 02 | 2 | STRM-01,02,03 | integration | `cargo test streaming_frame` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/streaming/tests/chunk_coord_tests.rs` — ChunkCoordLod coord math, equality, hashing
- [ ] `src/streaming/tests/octree_tests.rs` — octree node insert, SSE traversal stubs
- [ ] `src/streaming/tests/sse_tests.rs` — SSE formula correctness, NaN guard
- [ ] `src/streaming/tests/chunk_state_tests.rs` — 7-state + Error transitions, invalid edge rejection
- [ ] `src/streaming/tests/revision_id_tests.rs` — revision increments only on Active/Inactive entry
- [ ] `src/streaming/tests/job_queue_tests.rs` — enqueue, evict-lowest-SSE, capacity enforcement
- [ ] `src/streaming/tests/cancel_tests.rs` — queue removal, AtomicBool cancel flag set/check
- [ ] `src/streaming/tests/backpressure_tests.rs` — per-frame submit cap, queue-full eviction
- [ ] `src/streaming/tests/streaming_frame_tests.rs` — full frame: WorldUpdate diff + MeshSync drain

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Visual chunk pop-in/pop-out during movement | STRM-01 | Requires running renderer + player movement | Run binary, fly through world, verify no missing chunk holes at SSE boundary |
| SSE threshold tuning (1px vs 2px vs 4px) | STRM-01 | Perceptual quality judgment | Run binary, compare LOD switch distance at different thresholds |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
