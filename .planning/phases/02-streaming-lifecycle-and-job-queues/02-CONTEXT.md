# Phase 2 Context: Streaming Lifecycle and Job Queues

*Context gathered: 2026-03-21*

## Phase Goal

让玩家驱动的区块激活通过明确的生命周期状态和有界后台任务队列工作。活跃集由屏幕空间误差驱动（类 Nanite），而非固定半径。

## Decisions

### 世界与区块尺寸

- **方块大小**: 1/16m（6.25cm）
- **区块大小**: 64³ 格，每边 4m
- **LOD 层级**: 可配置，Phase 2 默认 3 层
  - LOD0: 64³ 格区块，4m 每边（全精度）
  - LOD1: 64³ 格区块，32m 每边（中等精度，每格代表 8 倍大小）
  - LOD2: 64³ 格区块，256m 每边（粗粒度远景）

### 活跃集驱动：屏幕空间误差（SSE）

- **驱动方式**: 屏幕空间误差驱动，不用固定球形半径
- **SSE 阈值**: 2px（可配置常量）——低于阈值则用更精细 LOD
- **数据结构**: 八叉树，每节点携带 `(x, y, z, lod_level: u8)` 坐标
- **误差计算公式**: `sse = (geometric_error * screen_height) / (2 * dist * tan(fov/2))`
  - `geometric_error` = 该 LOD 层区块的世界空间几何误差（单位：米）
  - 距离和 FOV 从当前摄像机参数取
- **视锥体裁剪**: 可配置——裁剪开启时视锥体外区块 SSE 视为 0，不触发加载
- **活跃集重算时机**: 每帧遍历八叉树，SSE 超过阈值的节点标记为需要更精细 LOD

### LOD 切换行为

- **切换策略**: 立即替换——LOD0 加载完成后 LOD1 立即卸载，无共存过渡
- **切换方向**: 摄像机靠近 → LOD 向下精细化（LOD1→LOD0）；摄像机远离 → LOD 向上粗化（LOD0→LOD1）
- **LOD 过渡渲染**: 留给 Phase 3（贪心网格和渲染增量同步）

### 区块生命周期状态机（7 态）

```
Inactive → Queued → Loading → Active
                               ↓            ↓
                          Upgrading    Downgrading   (LOD 切换中)
                               ↓            ↓
                          Unloading → Inactive
         Loading 失败 → Error → (指数退避重试 → Queued | 超限 → Inactive)
```

- **状态列表**:
  1. `Inactive` — 不在活跃集内，无数据
  2. `Queued` — 在加载队列中等待
  3. `Loading` — 后台任务正在执行
  4. `Active` — 数据就绪，可渲染
  5. `Upgrading` — LOD 精细化中（如 LOD1→LOD0）
  6. `Downgrading` — LOD 粗化中（如 LOD0→LOD1）
  7. `Unloading` — 正在卸载
  8. `Error` — 加载失败，等待重试

- **修订号（revision ID）**: 只在进入 `Active` 或 `Inactive` 时递增，其他状态转换不更新
- **Error 重试策略**: 指数退避——第1次立即重试，之后等待时间翻倍；超出最大重试次数（可配置）后进入 `Inactive`

### 任务队列

- **队列深度**: 可配置常量，默认 128
- **队列满策略**: 替换优先级最低的旧任务（SSE 值最小的，即距离摄像机最远/最不重要的）
- **优先级依据**: SSE 值——SSE 越大优先级越高（越需要精细化）
- **执行器**: rayon 线程池（已在依赖中）
- **任务取消**: 立即取消——若任务还在队列中则移除；若已在执行中则设置 `AtomicBool` 取消标记，任务检测后提前退出
- **每帧提交上限**: 可配置，避免单帧提交过多任务导致队列抖动（推荐默认 16 个/帧）

### 与现有 Phase 1 基础设施的集成

- `ChunkLifecycleCommand`（Activate/Deactivate）已存在于事件总线，Phase 2 扩展其语义以携带 `lod_level`
- `WorldUpdate` 阶段（当前为空桩）：在此阶段驱动 SSE 遍历、活跃集差分计算、任务提交
- `MeshSync` 阶段（当前为空桩）：在此阶段处理已完成加载任务的结果，更新区块状态
- Stage 边界保持不变：Input 发布命令，Simulation 处理命令，WorldUpdate/MeshSync 消费结果

## Code Context

- `src/runtime/stages.rs` — Stage 枚举，WorldUpdate 和 MeshSync 为空桩，Phase 2 填充
- `src/runtime/scheduler.rs` — 帧执行循环，WorldUpdate/MeshSync case 需扩展
- `src/runtime/events/command.rs` — `ChunkLifecycleCommand` 需加 `lod_level: u8` 字段
- `src/runtime/events/bus.rs` — EventBus，Phase 2 任务完成事件通过此发布
- `src/runtime/boundaries/world.rs` — WorldBoundaryRegistry，Phase 2 在此注册流式系统
- `src/runtime/boundaries/meshing.rs` — MeshingBoundaryRegistry，Phase 2 在此注册网格同步系统

## Deferred Ideas

- LOD 层间混合过渡（无缝切换，避免闪烁）— Phase 3
- 真正的贪心网格生成 — Phase 3
- 完整 LOD4/LOD5 超远景层 — 后续阶段
- 网络多人同步的区块状态广播 — Phase 7
