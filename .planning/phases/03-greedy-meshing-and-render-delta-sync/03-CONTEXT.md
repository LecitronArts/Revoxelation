# Phase 3: Greedy Meshing and Render Delta Sync — Context

**Gathered:** 2026-03-21
**Status:** Ready for planning

<domain>
## Phase Boundary

为可见体素表面生成贪心网格，并通过增量（delta）渲染同步路径将变化的区块上传到 GPU，而非每帧全量重传。

**具体包含：**
- 贪心网格生成算法（CPU 端，针对每个活跃区块）
- 邻居失效：区块边界变化时令相邻区块 mesh 失效并触发重新生成
- GPU 槽位池：管理活跃区块的 VB/IB，只上传变化区块
- LOD 接缝：通过 border skirt 消除不同 LOD 区块相邻时的视觉裂缝
- Draw Call 路径：compute culling + multi-draw indirect 提交

**不包含：**
- 纹理 / 光照着色（Phase 5）
- 玩家碰撞与移动（Phase 4）
- 区块持久化（Phase 6）

</domain>

<decisions>
## Implementation Decisions

### 顶点格式

- **格式：** 压缩打包，每顶点 **2× u32**（8 字节/顶点）
  - `u32_0`：xyz 格坐标（各 6 bit，限 64³ 区块内，共 18 bit）+ 面朝向（6 种，6 bit）+ 预留 8 bit
  - `u32_1`：block_id（8 bit）+ quad 内相对 UV 偏移（16 bit）+ 预留 8 bit
- **索引列表：** 顶点 + 索引（4 顶点/quad + 6 索引/quad），节省约 33% VRAM
- **UV 布局：** 存 quad 内相对偏移（16 bit 够）。纹理 atlas 切片由 block_id 在 shader 中查表，Phase 5 填充纹理逻辑时复用此布局

### GPU 缓冲区生命周期（槽位池）

- **结构：** 单块大缓冲区分段，VB 和 IB 各一块，均分为 N 个等大槽位
- **槽位数 N：** = 最大同时活跃区块数（LOD0 + LOD1 + LOD2 之和，与 Phase 2 的流式上限对齐）
- **每槽大小：** 按 LOD0 最差情况预留（6 × 64² 面 → 贪心合并后保守估计 ~4096 quad）；各 LOD 层共用相同槽位大小（空间换管理复杂度）
- **remesh 路径：** 通过 StagingBuffer（已有）→ vkCmdCopyBuffer → 对应槽位，only dirty chunks 上传
- **槽位分配：** 区块进入 Active 时分配槽位，进入 Unloading 时释放槽位

### LOD 边界接缝（Border Skirt）

- **策略：** LOD1/LOD2 负责生成 skirt —— 在面向更高精度（LOD0）邻居的一侧向下延伸额外面
- **触发条件：** LOD1 生成 mesh 时查询相邻位置的 LOD 级别；若相邻为 LOD0 且已 Active，则生成 skirt
- **未加载情况：** 若相邻 LOD0 尚未加载（未 Active），不生成 skirt；LOD0 进入 Active 后触发该 LOD1 区块失效重新生成（邻居失效机制复用）
- **LOD0 不生成 skirt：** LOD0 自身 mesh 不因相邻 LOD1 而改变

### Draw Call 结构

- **方式：** Multi-draw indirect（`vkCmdDrawIndexedIndirect`）
- **Culling：** 独立 compute shader pass，对每个活跃区块做 AABB vs 当帧视锥体剔除，通过的写入 indirect draw buffer
- **Instance buffer 同步：** CPU 只在区块进出活跃集时增量更新（写入新增区块的 AABB，清除已卸载区块条目），其余帧不碰 buffer
- **时序：** 同帧内 compute pass → pipeline barrier → graphics draw pass；barrier 确保 indirect buffer 写完再读
- **Descriptor：** 一个 storage buffer 存所有活跃区块的 AABB + 槽位 index；compute 写 indirect buffer，graphics 读 indirect buffer + 大 VB/IB

### Claude's Discretion

- 具体 descriptor set layout 和 binding 编号
- VkPipeline / VkPipelineLayout 的创建与缓存策略
- compute shader GLSL/SPIR-V 的具体实现细节
- barrier 的精确 srcStageMask / dstStageMask 选择

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets

- `StagingBuffer`（`src/renderer/mod.rs`）：已有 `new` / `write` / `copy_to` / `copy_to_image`，Phase 3 的 mesh 上传路径直接复用 `copy_to` 把 staging 数据复制进 VB/IB 槽位
- `create_allocated_buffer`（`src/renderer/mod.rs`）：Phase 3 用它分配大 VB、大 IB、AABB storage buffer、indirect draw buffer
- `submit_one_shot_commands`（`src/renderer/mod.rs`）：可用于初始化槽位池等一次性 GPU 操作
- `Renderer.allocator`（ManuallyDrop\<Allocator\>）：gpu-allocator 实例，所有新缓冲区通过它分配
- `ChunkKey` / `ChunkState` / `ChunkEntry`（`src/streaming/types.rs`）：Phase 3 mesh 管理以 ChunkKey 为索引，监听 Active/Unloading 状态变化
- `ChunkStateStore`（`src/streaming/state_store.rs`）：查询邻居 LOD 级别（skirt 决策）和监听状态变化

### Established Patterns

- 所有 GPU 资源通过 gpu-allocator 分配，无手动 vkAllocateMemory
- 双缓冲帧（2 frames in flight），command buffer 每帧 reset 后重新录制
- 现有 render pass 为单 subpass，Phase 3 需在此 render pass 内添加 graphics draw；compute pass 在 render pass 之外（compute 不能在 render pass 内 dispatch）
- `renderer_state()` 返回 `Option<&'static Mutex<Renderer>>`，Phase 3 mesh 系统通过此访问 Renderer

### Integration Points

- `src/runtime/scheduler.rs` — `MeshSync` arm：Phase 3 在此处理 mesh job 完成结果（VB/IB 上传、槽位分配/释放）
- `src/runtime/scheduler.rs` — `RenderSubmit` arm：Phase 3 在此执行 compute culling dispatch + indirect draw
- `src/runtime/boundaries/meshing.rs` — MeshingBoundaryRegistry：Phase 3 在此注册 mesh 生成系统
- 邻居失效：LOD0 进入 Active 时，通过现有事件总线（`src/runtime/events/bus.rs`）发布邻居失效事件，触发相邻 LOD1 重新生成 skirt

</code_context>

<specifics>
## Specific Ideas

- 用户明确选择了 compute shader 做 frustum culling + multi-draw indirect，而非每区块单独 draw call —— 架构要从 Phase 3 开始就支持 indirect，不要做 per-draw 的 fallback
- LOD 接缝由低精度侧（LOD1/LOD2）负责生成 skirt，高精度侧（LOD0）mesh 保持不变 —— 与标准 voxel engine 做法一致（参考 Transvoxel 思路的简化版）
- Instance buffer 只增量更新，不每帧全量刷新 —— 这个决定意味着需要一个空闲槽位 bitmap 追踪哪些槽位当前有效

</specifics>

<deferred>
## Deferred Ideas

- LOD 混合过渡（blend/淡入淡出，避免 hard cut 闪烁）—— Phase 4 或之后
- Hi-Z occlusion culling（在 frustum culling 基础上加 depth pyramid 剔除遮挡体）—— 后续优化阶段
- 顶点格式从 2× u32 压缩进一步到 1× u32（牺牲扩展性）—— 性能调优阶段
- 多 transfer queue 异步 mesh 上传（当前用 graphics queue）—— Phase 6 之后
- 完整 LOD 层 LOD3/LOD4 超远景 —— 后续阶段

</deferred>

---

*Phase: 03-greedy-meshing-and-render-delta-sync*
*Context gathered: 2026-03-21*
