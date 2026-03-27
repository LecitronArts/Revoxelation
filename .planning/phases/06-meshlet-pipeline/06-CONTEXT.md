# Phase 6: Meshlet Pipeline - Context

**Gathered:** 2026-03-27
**Status:** Ready for planning

<domain>
## Phase Boundary

将 greedy mesh 输出拆分为 meshlet（64v/124t 簇），实现 per-meshlet GPU 剔除（背面+视锥+Hi-Z），构建 compute+indirect draw 软件模拟路径和 VK_EXT_mesh_shader 硬件路径（双实现+启动时选择），以及 Nanite 风格 DAG LOD 过渡。

**不包含：**
- PBR 光照 / 阴影（Phase 7）
- 玩家移动与碰撞（Phase 8）
- 方块放置/破坏（Phase 9）

</domain>

<decisions>
## Implementation Decisions

### Meshlet 分簇参数

- **上限：** 64 顶点 / 124 三角形（Nanite 标准，VK_EXT_mesh_shader 硬件上限兼容）
- **分簇算法：** 使用 meshoptimizer（meshopt crate）的 build_meshlets() API，内置空间聚类、顶点去重
- **执行位置：** CPU 端 rayon 并行，集成在 greedy mesh 的 rayon job 内（build_greedy_mesh → meshoptimizer 分簇 → 输出 MeshletMesh）
- **辅助数据：** 每个 meshlet 预计算包围球（center + radius）和朝向锥（cone axis + cutoff），由 meshoptimizer 计算

### Compute 软件模拟 vs Mesh Shader 路径

- **双路径并行开发：** 抽象 MeshletPipeline trait，compute+indirect draw 和 VK_EXT_mesh_shader 各自实现
- **Fallback 时机：** 应用启动时检查 VK_EXT_mesh_shader 支持，选择实现，运行期不切换
- **目标 GPU：** 现代 GPU（RTX 20+ / RDNA2+），mesh shader 路径是主要性能目标
- **Compute 路径：** compute shader 做剔除+compaction → indirect draw 提交可见 meshlet
- **Mesh Shader 路径：** task shader 做剔除 → mesh shader 输出可见 meshlet 的顶点/图元

### Per-meshlet 剔除粒度与模式

- **两级级联：** 保留现有 chunk 级 cull_pipeline（Phase 4 的视锥+Hi-Z）作为第一级；通过的 chunk 再进入 meshlet 级剔除
- **三种剔除模式：** 背面剔除（朝向锥）、视锥剔除（包围球）、Hi-Z 遮挡剔除（包围球投影 vs 深度金字塔）
- **独立开关：** 每种模式各有独立 runtime bool / push constant，可在 egui HUD 中实时开关
- **Compaction 策略：** subgroup ballot + atomicAdd（workgroup 内局部 compact 后每组一次 atomic），减少全局 atomic 争用

### LOD 过渡与接缝策略

- **LOD 粒度：** Per-meshlet LOD（Nanite 风格），同一 chunk 内不同 meshlet 可在不同 LOD 级别
- **DAG 简化：** Nanite 风格 DAG，每级 LOD 的 meshlet 组是上一级的简化版本，共享边界顶点保证无缝
- **初始实现：** 2 级 DAG（LOD0 原始 meshlet → LOD1 简化 meshlet），验证后扩展到更多级别
- **过渡方式：** Alpha dither 淡入淡出，相邻 LOD 级别的 meshlet 在 1-2 帧内 dither 过渡
- **接缝处理：** 废弃 Phase 3 的 border skirt 策略，改用 Nanite DAG 简化的边界顶点共享机制（DAG 上下级 meshlet 组共享边界顶点，天然无缝）

### GPU 内存布局

- **Meshlet-local 存储：** 每个 meshlet 自带顶点数据（64v × 8B）+ u8 本地索引（124t × 3B），打包在统一 meshlet SSBO 中
- **ChunkPool 重构：** 现有 3 buffer（vertex、index、scene_buffer）合并为 1 个 meshlet SSBO。废弃大 VB/IB 槽位池，改用 meshlet 粒度的动态分配
- **动态容量：** 延续 Phase 5 的倍增策略（初始容量 → 2× 增长）

### 更新管线与 MeshSync

- **分簇集成：** rayon job 内 build_greedy_mesh → meshoptimizer 分簇 → 输出 MeshletMesh，MeshSync 收到的已是分簇后数据
- **RenderDelta 替换：** RenderDelta::Upsert 载荷从 PackedMesh 改为 MeshletMesh，MeshSync 直接生成 meshlet SSBO 更新
- **增量更新：** 只 remesh 变化的 chunk，meshlet SSBO 中对应 chunk 的区段被替换

### 调试与可视化

- **Meshlet 统计面板：** 在现有 egui HUD / perf_counters 上集成，显示：总 meshlet 数、可见 meshlet 数、剔除率（按背面/视锥/Hi-Z 分别统计）、meshlet SSBO 内存用量
- **剔除开关 UI：** 三种剔除模式的独立开关在 egui 面板中暴露

### Claude's Discretion

- meshoptimizer build_meshlets 的具体调用参数（cone_weight 等）
- Meshlet SSBO 内部的具体内存区域排列和对齐方式
- DAG 简化的具体顶点合并/边折叠算法选择
- Compute 路径的 workgroup size 和 dispatch 策略
- Alpha dither 的具体 pattern 和帧数
- MeshletPipeline trait 的具体 API 设计

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets

- `build_greedy_mesh`（`src/meshing/greedy.rs`）：meshlet 分簇集成点，输出从 PackedMesh 变为 MeshletMesh
- `ChunkPool`（`src/renderer/chunk_pool.rs`）：Phase 6 重构为 meshlet SSBO 管理，Phase 5 的动态容量增长逻辑可复用
- `ChunkCullPipeline`（`src/renderer/cull_pipeline.rs`）：保留为第一级 chunk 剔除，meshlet 剔除在其后级联
- `BindlessTable`（`src/renderer/bindless.rs`）：set 0 绑定管理，Phase 6 注册 meshlet SSBO
- `ChunkMeshPipeline`（`src/renderer/mesh_pipeline.rs`）：Phase 6 重构为 MeshletPipeline trait + 双实现
- `HiZPyramid`（`src/renderer/hiz.rs`）：meshlet 级 Hi-Z 剔除复用现有深度金字塔
- `StagingRing`（`src/renderer/staging_ring.rs`）：meshlet SSBO 上传复用
- `perf_counters`（`src/renderer/perf_counters.rs`）：meshlet 统计扩展点
- `meshopt` crate：需要添加到 Cargo.toml 依赖

### Established Patterns

- 所有 GPU 资源通过 `gpu-allocator` 分配
- 双缓冲帧（2 frames in flight），fence 同步
- GLSL 源码在 `shaders/`，`build.rs` 编译为 SPIR-V
- push constants 用于 camera uniforms（80 bytes）
- subgroup 操作需要 Vulkan 1.1+（已有 1.2 硬要求）

### Integration Points

- `src/meshing/greedy.rs` — build_greedy_mesh：输出类型变更，集成 meshoptimizer
- `src/meshing/packing.rs` — PackedMesh：可能被 MeshletMesh 替代或包装
- `src/renderer/chunk_pool.rs` — ChunkPool：从 3 buffer 重构为 meshlet SSBO
- `src/renderer/cull_pipeline.rs` — 保留但输出变为 meshlet 剔除输入
- `src/renderer/mesh_pipeline.rs` — 重构为 MeshletPipeline trait
- `src/renderer/submit.rs` — 提交路径适配 meshlet 剔除 + indirect draw / mesh shader
- `src/renderer/bindless.rs` — BindlessTable 注册 meshlet SSBO
- `src/runtime/scheduler.rs` — MeshSync arm：接收 MeshletMesh 而非 PackedMesh
- `shaders/` — 新增 meshlet_cull.comp、meshlet_draw.vert/frag（compute 路径）或 meshlet.task/meshlet.mesh（mesh shader 路径）
- `build.rs` — 新增 shader 编译

</code_context>

<specifics>
## Specific Ideas

- 架构方向明确对标 Nanite：per-meshlet LOD、DAG 简化、meshlet-local 存储、alpha dither 过渡
- meshoptimizer 作为分簇库，零维护成本，业界标准（Nanite 也基于类似算法）
- 废弃 Phase 3 的 border skirt 策略，DAG 共享边界顶点天然消除接缝
- 两级级联剔除（chunk → meshlet）平衡剔除精度和 dispatch 开销
- subgroup ballot + atomicAdd 作为 compaction 策略，减少全局 atomic 争用
- 双路径通过抽象 trait 实现，启动时选择，运行期不切换

</specifics>

<deferred>
## Deferred Ideas

- 3 级以上 DAG LOD（LOD0→LOD1→LOD2→...）— 2 级验证通过后扩展
- 剔除结果可视化（红色线框渲染被剔除 meshlet）— 调试优化阶段
- meshlet bounding sphere 线框渲染 — 调试优化阶段
- LOD 级别颜色编码渲染模式 — 调试优化阶段
- 运行时 mesh shader ↔ compute 路径切换（A/B 测试）— 性能调优阶段

</deferred>

---

*Phase: 06-meshlet-pipeline*
*Context gathered: 2026-03-27*
