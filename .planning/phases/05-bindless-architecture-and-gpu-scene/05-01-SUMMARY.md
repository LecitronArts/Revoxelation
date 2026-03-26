---
phase: 05-bindless-architecture-and-gpu-scene
plan: 01
subsystem: renderer
tags: [vulkan, ash, descriptor-indexing, bindless, vulkan-1.2]

requires:
  - phase: 04-rendering-foundation-overhaul
    provides: "Device creation, Vulkan instance, ash integration"
provides:
  - "Vulkan 1.2 hard requirement with 7 descriptor indexing features"
  - "PhysicalDeviceVulkan12Features pNext chain on device creation"
  - "Graceful error messages listing specific missing features per GPU"
affects: [05-bindless-architecture-and-gpu-scene, 06-meshlet-pipeline]

tech-stack:
  added: []
  patterns:
    - "Vulkan 1.2 feature probing via get_physical_device_features2 + pNext chain"
    - "PhysicalDeviceFeatures2 wrapping both 1.0 features and Vulkan12Features"

key-files:
  created:
    - tests/phase5_bindless.rs
  modified:
    - src/renderer/device.rs

key-decisions:
  - "Use vk::PhysicalDeviceVulkan12Features (core 1.2 struct), not extension-era VkPhysicalDeviceDescriptorIndexingFeatures"
  - "DeviceCreateInfo chains PhysicalDeviceFeatures2 via push_next; 1.0 features in .features field, 1.2 features via nested push_next"
  - "No fallback path: unsupported GPUs fail fast with descriptive error listing missing feature names and GPU name"

patterns-established:
  - "Vulkan 1.2 feature enforcement: missing_vulkan12_features() returns list of missing feature names"

requirements-completed: [BIND-01]

duration: 4min
completed: 2026-03-26
---

# Phase 5 Plan 01: Vulkan 1.2 Device Upgrade Summary

**Hard-require Vulkan 1.2 descriptor indexing and drawIndirectCount via PhysicalDeviceVulkan12Features pNext chain — no fallback**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-26T04:13:54Z
- **Completed:** 2026-03-26T04:17:19Z
- **Tasks:** 1 (TDD: RED + GREEN)
- **Files modified:** 2

## Accomplishments
- Device creation now probes 7 Vulkan 1.2 features via `get_physical_device_features2` with `PhysicalDeviceVulkan12Features` in pNext chain
- GPUs missing any required feature are skipped with `log::warn` listing specific missing feature names
- If no GPU passes all checks, error includes "Vulkan 1.2 feature(s) missing: {list}. GPU: {name}."
- `DeviceCreateInfo` uses `PhysicalDeviceFeatures2` with `push_next` for both 1.0 and 1.2 features (replaces old `.enabled_features()` call)

## Task Commits

Each task was committed atomically:

1. **Task 1 RED: Failing source-grep tests** - `1b7e488` (test)
2. **Task 1 GREEN: Vulkan 1.2 feature probe + enforcement** - `8b0447a` (feat)

## Files Created/Modified
- `tests/phase5_bindless.rs` - 4 source-grep tests for BIND-01 (feature names, pNext chain, error messages, no fallback)
- `src/renderer/device.rs` - `missing_vulkan12_features()`, updated `pick_physical_device()` with Vulkan 1.2 checks and `PhysicalDeviceFeatures2` pNext device creation

## Decisions Made
- Used core Vulkan 1.2 struct (`PhysicalDeviceVulkan12Features`) per D-01 locked decision
- Error format follows D-04: "Vulkan 1.2 feature(s) missing: {comma-separated list}. GPU: {device_name}."
- Removed `required_device_features_error()` function — error messages now dynamically constructed

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed stale `required_device_features_error()` function**
- **Found during:** Task 1 GREEN (implementation)
- **Issue:** Old function returned a static string for 1.0 features only; no longer accurate with 1.2 enforcement
- **Fix:** Replaced with dynamic error construction that includes both 1.0 and 1.2 feature details
- **Files modified:** src/renderer/device.rs
- **Verification:** All 4 tests pass, cargo build succeeds
- **Committed in:** 8b0447a

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary for correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Vulkan 1.2 features are now hard-required at device selection
- Ready for Plan 05-02: Bindless descriptor set + global resource table

---
*Phase: 05-bindless-architecture-and-gpu-scene*
*Completed: 2026-03-26*
