# Phase 7: Lighting and Shadows - Context

**Gathered:** 2026-03-29
**Status:** Ready for planning

<domain>
## Phase Boundary

建立完整的实时光照系统：方向光 PBR 光照（Lambertian diffuse + GGX specular）、4 级联 CSM 阴影、SSAO（多算法可配置）、体素 AO（经典 4-角法）、天空/大气渲染与昼夜循环。将视觉质量从纯色方块提升到有深度和氛围感的场景。

**不包含：**
- 玩家移动与碰撞（Phase 8）
- 方块放置/破坏（Phase 9）
- Chunk 持久化（Phase 10）

</domain>

<decisions>
## Implementation Decisions

### PBR 材质系统

- **4 张纹理贴图：** 每种 block_id 有 albedo、metallic-roughness、normal map、emissive map 共 4 张贴图
- **纹理分辨率：** 16/32 混合分辨率 — 普通方块用 16x16，特殊方块可用 32x32
- **混合分辨率实现：** 需要多个 texture array 或 atlas 方案处理不同分辨率纹理
- **BlockMaterial 扩展：** 利用现有 flags 字段（16 bits）标记是否有 MR/normal/emissive 贴图以及分辨率级别
- **每面独立纹理：** 延续 Phase 5 决定 — top/side/bottom 三组纹理，每组 4 张 PBR 贴图
- **发光方块行为：** emissive 方块自发光 + 产生动态点光源，照亮周围方块
- **动态点光源限制：** 需要点光源管理系统（限制同时可见点光源数量，按距离/亮度排序）

### 阴影系统（CSM）

- **级联数量：** 4 级联 CSM
- **深度图分辨率：** 默认 2048x2048，可在 egui HUD 中实时调整
- **CSM 参数可配置：** 级联数、分辨率、分割比例等均可在 egui 面板中调整
- **柔化算法：** PCF（3x3 或 5x5 kernel）
- **级联过渡：** 级联边界附近线性混合（从两个级联采样并插值），消除硬切换闪烁
- **阴影渲染：** depth-only pass，对每个级联从光源视角渲染场景

### 环境光遮蔽

- **SSAO 多算法可配置：** 实现 GTAO、HBAO+、经典 SSAO 三种算法，运行时在 egui 面板中切换
- **体素 AO：** 经典 4-角遮挡法，在 greedy meshing 时预计算，存入 packed vertex 数据
- **Packed vertex 扩展：** 从 uvec2 扩展以容纳 4 个角的 AO 值（每角 2 bits = 8 bits 总计）
- **叠加方式：** final_ao = voxel_ao x ssao，两者独立计算后相乘
- **SSAO 输入：** 复用现有深度 buffer + 从 G-buffer 或重建获取法线

### 天空与昼夜循环

- **大气模型可选：** 实现 Preetham 和 Hosek-Wilkie 两种模型，运行时在 egui 中切换
- **昼夜循环：** 连续太阳轨迹，太阳在天空中匀速旋转，日出/日落/正午/午夜平滑过渡
- **夜晚光照：** 太阳下山后切换为较暗的月光方向光，色温偏蓝
- **距离雾可选：** 实现线性雾、指数雾、高度雾三种类型，运行时在 egui 中切换
- **雾色与天空联动：** 雾色随天空颜色变化（日出偏暖、正午偏白、日落偏橙）
- **昼夜速率：** 可配置的时间倍率（如 1 游戏日 = N 现实分钟）

### Claude's Discretion

- PCF kernel 大小（3x3 vs 5x5）的具体选择
- CSM 级联分割比例（logarithmic vs practical split scheme）
- SSAO 采样数和半径的具体默认值
- 点光源的最大同时可见数量和衰减函数
- 混合分辨率纹理的具体 atlas / multi-array 实现方式
- 太阳轨迹的具体角度参数
- 天空 fullscreen quad vs skybox cube 的渲染方式
- SSAO bilateral blur 的具体参数

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets

- `HiZPyramid`（`src/renderer/hiz.rs`）：深度金字塔已存在，CSM depth pass 可复用 depth 渲染基础设施，SSAO 可复用深度 buffer
- `BindlessTable`（`src/renderer/bindless.rs`）：set 0 绑定管理，Phase 7 注册 shadow map、AO 纹理、天空 cubemap 等新资源
- `MaterialSystem`（`src/renderer/material.rs`）：BlockMaterial + texture_array 已存在，扩展为 4 张 PBR 贴图
- `TextureArray`（`src/renderer/texture_array.rs`）：16x16 纹理数组已存在，需扩展支持 MR/normal/emissive 和 32x32 分辨率
- `common.glsl`（`shaders/common.glsl`）：共享 shader include 系统已建立，新增 PBR 函数、阴影采样、AO 计算
- `MeshletDrawPushConstants`：已有 screen_height、sse_threshold、current_time 字段，可扩展 sun_direction 等光照参数
- `StagingRing`（`src/renderer/staging_ring.rs`）：32MB staging ring，shadow map / AO 纹理上传可复用
- `perf_counters`（`src/renderer/perf_counters.rs`）：性能统计扩展点，增加光照/阴影 pass 耗时

### Established Patterns

- 所有 GPU 资源通过 `gpu-allocator` 分配
- 双缓冲帧（2 frames in flight），fence 同步
- GLSL 源码在 `shaders/`，`build.rs` 编译为 SPIR-V，`#include "common.glsl"` 共享定义
- Push constants 用于 camera uniforms（当前 88 bytes），可扩展
- Compute shader 做剔除/后处理，graphics pipeline 做渲染
- egui 面板暴露运行时开关（剔除模式、性能数据等）
- 现有 chunk_mesh.frag 和 meshlet_draw.frag 做材质采样和 dither，Phase 7 在此基础上添加 PBR 光照

### Integration Points

- `shaders/chunk_mesh.frag` — 添加 PBR 光照计算（diffuse + specular + shadow + AO）
- `shaders/meshlet_draw.frag` — 同上，meshlet 路径的 fragment shader
- `shaders/meshlet.mesh` — mesh shader 路径也需要传递光照相关数据
- `shaders/common.glsl` — 新增 PBR 函数库（BRDF、shadow sampling、AO）
- `src/renderer/material.rs` — BlockMaterial 扩展 PBR 字段
- `src/renderer/texture_array.rs` — 多纹理数组支持 MR/normal/emissive
- `src/renderer/bindless.rs` — 注册 shadow map、AO texture、sky texture
- `src/renderer/mod.rs` — 新增 shadow pass、AO pass、sky pass 到渲染流程
- `src/renderer/submit.rs` — submit_frame 中插入 shadow/AO/sky render pass
- `src/meshing/greedy.rs` — 计算 4-角体素 AO，扩展 packed vertex 格式
- `src/meshing/packing.rs` — packed vertex 格式扩展以包含 AO 数据
- `build.rs` — 新增 shadow/AO/sky shader 编译

</code_context>

<specifics>
## Specific Ideas

- SSAO 算法选择参考现代引擎做法：GTAO 作为默认（Nanite 级引擎标准），提供 HBAO+ 和经典 SSAO 作为备选
- 发光方块产生真实点光源是重要的视觉目标 — 熔岩照亮洞穴、灯具照亮房间
- CSM 所有参数在 egui 中可调是明确需求 — 方便开发期间调试阴影质量
- 大气模型和雾效都是"可选"的 — 运行时切换，不是编译期选择
- 4-角体素 AO 是 Minecraft 风格的经典做法，视觉效果好且开销极低（meshing 时计算）
- 混合分辨率（16/32）给特殊方块更多细节空间，特别是 normal map 和 emissive map 在 32x32 下效果明显好于 16x16

</specifics>

<deferred>
## Deferred Ideas

- 全局光照（GI）/ 光线追踪反射 — 超出 v1 范围
- 体积云渲染 — 可作为后续视觉增强
- 水面反射/折射 — Phase 9+ 的水方块特效
- 粒子系统（火焰、烟雾）— 独立系统
- 屏幕空间反射（SSR）— 后续优化
- Bloom 后处理 — 配合 emissive 使用，但 Phase 7 先做基础光照

</deferred>

---

*Phase: 07-lighting-and-shadows*
*Context gathered: 2026-03-29*
