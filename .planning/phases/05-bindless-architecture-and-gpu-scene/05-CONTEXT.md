# Phase 5: Bindless Architecture and GPU Scene - Context

**Gathered:** 2026-03-26
**Status:** Ready for planning

<domain>
## Phase Boundary

利用 Vulkan 1.2 descriptor indexing（硬要求，无 fallback）消除 per-material descriptor set 切换，构建统一 GPU scene buffer，建立 block 材质/纹理系统。当前 6 个 per-chunk buffer 合并为 3 个，chunk 容量从固定 881 变为动态增长，IndirectCount 取代 CPU 端 draw count 管理。

**不包含：**
- Meshlet 生成与 GPU 剔除（Phase 6）
- PBR 光照 / 阴影（Phase 7）
- 玩家移动与碰撞（Phase 8）

</domain>

<decisions>
## Implementation Decisions

### 纹理/材质系统

- **每面独立纹理：** 每个 block_id 的 top/side/bottom 可指向不同 texture array 切片（如草块顶部绿色、侧面土层、底部泥土）
- **BlockMaterial 结构：** `{ top_texture: u16, side_texture: u16, bottom_texture: u16, flags: u16 }`（8 bytes）
- **6 种面朝向映射到 3 组：** +Y → top, -Y → bottom, ±X/±Z → side，shader 根据 face normal 选择 texture index
- **纹理分辨率：** 统一 16×16 RGBA8，所有 block 纹理在同一个 2D texture array 中
- **纹理来源：** 运行时从 `assets/textures/` 目录加载 PNG 文件，使用 `image` crate 解码，构建 texture array
- **初始方块种类：** 8 种基础方块（泥土、草块、石头、沙子、原木、木板、树叶、水），足够验证材质系统可工作
- **Material SSBO：** 所有 BlockMaterial 打包进一个 SSBO，shader 通过 block_id 索引

### GPU Scene Buffer 合并

- **从 6 buffer 合并到 3 buffer：**
  - 保留：vertex_buffer (GpuOnly)、index_buffer (GpuOnly)
  - 合并 metadata_buffer + indirect_template + draw_slot_mapping + dense_indirect → 统一 scene_buffer (SSBO)
- **GpuChunkInstance 结构：** 每区块一条记录，包含 aabb_min/max、chunk_origin、chunk_scale、lod_level、material_id 等
- **indirect commands 区域：** scene_buffer 内单独区域存放 VkDrawIndexedIndirectCommand 数组
- **索引方式：** vertex shader 通过 `gl_DrawID` 直接索引 scene_buffer 获取 per-chunk 数据，无额外 instance buffer

### 动态容量增长策略

- **增长策略：** 倍增（Vec 风格），当前容量不够时分配 2× 新 buffer
- **初始容量：** 1024 slots（取代硬编码 MAX_RENDER_CHUNKS=881）
- **触发条件：** active_chunks > capacity × 0.9
- **增长流程：** 分配新 buffer → vkCmdCopyBuffer 复制旧数据 → fence wait → 释放旧 buffer → 更新 descriptor set 绑定
- **IndirectCount：** 从 vkCmdDrawIndexedIndirect 切换到 vkCmdDrawIndexedIndirectCount，GPU 写入 draw count，CPU 只传 max_capacity 作为安全上限
- **VK_KHR_draw_indirect_count：** Vulkan 1.2 核心功能，与 descriptor indexing 同步要求

### Bindless 迁移路径

- **一步切换：** Plan 02 中建立全局 bindless set 0 并同时迁移 cull_pipeline 和 mesh_pipeline，废弃旧 per-pipeline descriptor set
- **BindlessTable 结构体：** 管理 set 0 所有 binding，提供 register_buffer / register_texture 接口
- **全局 descriptor set layout：**
  - binding 0: scene SSBO（chunk instances + indirect commands）
  - binding 1: material SSBO（BlockMaterial 数组）
  - binding 2: texture array sampler（2D array，16×16 per layer）
  - binding 3+: 扩展位（未来 Phase 6/7 使用）
- **Vulkan 1.2 硬要求：** 设备创建时检查 descriptorIndexing 等特性，不支持则 log::error! 打印明确消息（含缺失的具体 feature 名称）并返回 Err，不做 fallback
- **旧代码清理：** 删除 cull_pipeline 和 mesh_pipeline 各自的 descriptor_pool / descriptor_set_layout / descriptor_set 代码

### Claude's Discretion

- BindlessTable 内部 descriptor pool 大小和 update 频率策略
- scene_buffer 内 GpuChunkInstance 与 indirect commands 的具体内存布局排列
- texture array mipmap 生成方式（compute vs CPU 预生成）
- 增长时的 fence 同步精确时机和帧内 placement

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets

- `ChunkPool`（`src/renderer/chunk_pool.rs`）：当前管理 6 个 buffer + slot bitmap，Phase 5 重构为 3 buffer + 动态容量
- `StagingRing`（`src/renderer/staging_ring.rs`）：32MB ring buffer，增长时的 buffer copy 可复用
- `ChunkCullPipeline`（`src/renderer/cull_pipeline.rs`）：8-binding descriptor set + compute cull，Phase 5 迁移到 bindless set 0
- `ChunkMeshPipeline`（`src/renderer/mesh_pipeline.rs`）：当前 graphics pipeline，Phase 5 重写 descriptor 引用
- `DeviceContext`（`src/renderer/device.rs`）：设备选择逻辑，Phase 5 在此添加 Vulkan 1.2 feature 检查
- `HiZPyramid`（`src/renderer/hiz.rs`）：Hi-Z 深度金字塔，descriptor 绑定需迁移到 set 0
- `create_allocated_buffer` / `destroy_allocated_buffer`（`src/renderer/helpers.rs`）：buffer 创建/销毁，增长路径直接复用

### Established Patterns

- 所有 GPU 资源通过 `gpu-allocator` 分配，Phase 5 延续此模式
- 双缓冲帧（2 frames in flight），增长操作需在帧间 fence 同步后执行
- GLSL 源码在 `shaders/` 目录，`build.rs` 编译为 SPIR-V
- push constants 用于 camera uniforms（80 bytes），Phase 5 不改变此模式

### Integration Points

- `src/renderer/device.rs` — pick_physical_device：添加 Vulkan 1.2 feature 检查
- `src/renderer/chunk_pool.rs` — ChunkPool：重构为 3 buffer + 动态容量
- `src/renderer/cull_pipeline.rs` — ChunkCullPipeline：迁移到 bindless set 0
- `src/renderer/mesh_pipeline.rs` — ChunkMeshPipeline：迁移到 bindless set 0 + 材质采样
- `shaders/chunk_mesh.vert` — 添加 gl_DrawID 索引和纹理采样
- `shaders/chunk_mesh.frag` — 添加 texture array 采样
- `shaders/chunk_cull.comp` — 更新 buffer 绑定到 set 0

</code_context>

<specifics>
## Specific Ideas

- 纹理系统必须为 Phase 7 的 PBR 光照做好准备，BlockMaterial 的 flags 字段预留 emissive / transparent 等标志位
- 增长操作在帧间完成，不允许在 command buffer 录制中途触发增长
- IndirectCount 完全取代 CPU 端 draw count 管理，cull compute shader 的 atomicAdd 写入 count_buffer 机制保持不变
- 8 种初始方块的纹理 PNG 可以先用程序生成的简单像素图（棋盘格、纯色+噪声），不需要美术级资源

</specifics>

<deferred>
## Deferred Ideas

- PBR metallic/roughness 参数加入 BlockMaterial — Phase 7 光照时扩展
- 纹理热重载（文件变化时自动重新加载 texture array）— 后续优化
- 多 texture array 支持混合分辨率 — 当前不需要
- 完整的资源管理器（异步加载、引用计数）— v2 功能

</deferred>

---

*Phase: 05-bindless-architecture-and-gpu-scene*
*Context gathered: 2026-03-26*
