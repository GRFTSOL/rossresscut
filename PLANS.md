# 开发计划

## 当前阶段

**Phase 5：多片段合并（单段 + 多段已实现）**

- 单 Segment 导出已完成并提交（`7a81434`）。
- Phase 5 调查：`docs/investigations/multi-segment-concat.md`。
- 多 Segment 实现：临时片段 + concat demuxer；临时目录在成功、失败、取消后自动清理。
- 顺序严格使用 `Timeline.segments()`；非关键帧起点回退到此前关键帧。
- 拼接处存在 ≤1 帧视频间隙，已在文档中记录。
- 导出章节按每个临时片段的实际容器时长累计，测试见
  `tests/multi_export.rs::chapters_follow_actual_part_lengths`。
- 切片不再使用 `-shortest`：源带附加图片/字幕流时它会提前结束，产物可能近乎为空。
- 未完成：stream selection、显式 metadata 处理、容器预检查、4K/VFR 验证。
- 未解决问题记录在 `docs/open-questions.md`。

## 已完成的 UI 批次

- 时间轴与控制行分层：时间轴单独一行，控制按钮位于其下并响应式居中。
- 符号按钮 + hover tooltip；片段按 Timeline 顺序带序号。
- 设置页分组为“常规 / 导出 / 快捷键”，TOML 持久化到 `config.toml`，支持字体缩放。
- 快捷键可在设置页录制、可为空、即时生效；播放控件动作也可绑定。
- “查看”菜单：全屏切换、复位 / 增大 / 减小字体大小。
- 字体滑块松开鼠标后生效，步进 0.05，并提供复位按钮。
- 时间轴与右侧片段列表使用同一配色；未选中片段为半强度颜色。
- 侧栏播放信息仅保留解码器（`mpv (vaapi)` 等实际 hwdec）、帧、视频、音频。
- 未打开媒体时点击预览区任意位置打开文件；提示文字不可选择。
- 快捷键：Space / I / O / Enter / Escape / Ctrl+W。
- 菜单栏、设置按钮、导出按钮加大；导出按钮固定在右下角。
- 已知未解决：mpv 精确 seek 后的短暂旧帧（OQ-21）。

除非用户明确要求，不提前实现后续阶段功能。

---

# 参考实现研究

LosslessCut 是产品行为、媒体语义和 FFmpeg 工作流的参考实现，不是本项目的架构或代码模板。

- 研究顺序：本项目代码 / ADR → FFmpeg 官方文档 → LosslessCut 实现与 issue / commit → 实验验证 → 设计本项目实现。
- 重点：FFmpeg 导出参数、`-ss` / `-to` / `-t`、stream copy、PTS/DTS/time base、keyframe、smart cut、concat、音画同步、容器兼容性、metadata、字幕、chapter。
- LosslessCut 使用 GPL-2.0：只阅读、理解和验证行为；不复制源代码，不做逐行 TypeScript → Rust 翻译。
- 阶段约束：Phase 4 只做普通 stream copy；smart cut 属于后续阶段，必须先调查再决定是否实现。当前 PLANS 编号中 Phase 5 是多段 concat、smart cut 在 Phase 8 候选；若要把 smart cut 提前到 Phase 5/6，需要显式调整阶段编号。

---

# Phase 0：项目基础

状态：`完成`

## 目标

建立最小可运行 Rust + egui 项目。

## 任务

- [x] 创建 Cargo 项目
- [x] 建立模块结构
- [x] 加入 egui / eframe
- [x] 创建最小窗口
- [x] 建立错误处理
- [x] 建立日志
- [x] 建立测试结构
- [x] 建立基本 CI 检查（`scripts/check.sh`）
- [x] 创建 `AGENTS.md`
- [x] 创建 `GOALS.md`
- [x] 创建 `PLANS.md`
- [x] 创建架构文档

托管 CI：`.github/workflows/ci.yml`。Git 仓库已初始化；初始 commit 与 Phase 2 commit 已创建，Phase 2 修订尚未提交。

## 完成条件

```text
cargo check
cargo test
cargo clippy
cargo fmt --check
```

全部通过。

验证记录（2026-10-24）：

- `./scripts/check.sh` 通过，实际执行 `cargo fmt --all --check`、`cargo check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`、`cargo build`。
- 最小窗口已在当前 Wayland 会话中实际启动，8 秒后由 `timeout` 终止。

---

# Phase 1：媒体分析

状态：`完成`

## 目标

能够打开媒体并获得可靠的媒体信息。

## 任务

- [x] FFprobe 进程封装
- [x] FFprobe JSON 解析
- [x] MediaFile
- [x] MediaFormat
- [x] VideoStream
- [x] AudioStream
- [x] SubtitleStream
- [x] Chapter
- [x] Duration
- [x] Timestamp
- [x] 错误处理
- [x] 测试媒体

## 完成条件

能够正确读取典型：

- MP4
- MKV
- WMV
- AVI
- MPEG-TS

并显示媒体基本信息。

验证记录（2026-10-24）：

- `./scripts/check.sh` 通过，实际执行 `cargo fmt --all --check`、`cargo check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`、`cargo build`。
- 单元测试 17 个、集成测试 7 个，全部通过。
- 集成测试使用真实 FFprobe 读取 `sample.mp4`、`sample.mkv`、`sample.wmv`、`sample.avi`、`sample.ts`，断言容器、codec、尺寸、流数量、章节、非零 start time 与错误路径。
- 窗口通过 `./target/debug/rossresscut tests/fixtures/sample.mkv` 实际启动；后台探测报告 4 个流、2 个章节。
- `.github/workflows/ci.yml` 已添加，尚未在 GitHub Actions 环境实际执行。

---

# Phase 2：媒体预览

状态：`完成`

## 目标

实现流畅的视频预览。

## 任务

- [x] 确定预览后端
- [x] 设计 PreviewBackend
- [x] 播放
- [x] 暂停
- [x] Seek
- [x] 时间显示
- [x] 错误处理
- [x] 音频播放

## 注意

预览后端必须先进行架构评估。

不要因为“需要播放视频”就直接添加未经评估的播放器 crate。

当前只有一套具体后端（外部 ffmpeg 视频帧 + cpal 音频输出），因此没有引入 `PreviewBackend` trait；
未来出现第二套后端时再按真实复用需求抽取接口。

## Phase 2 修订（2026-10-24）

根据真实文件测试补齐：

- [x] 拖动时间轴时刷新预览（快速 keyframe scrub，松开后精确 seek）
- [x] 音频主时钟（FFmpeg PCM + cpal），修复独立 ffplay 进程导致的 A/V 偏移
- [x] 硬件解码默认启用：Linux VAAPI、Windows D3D11VA、macOS VideoToolbox；设备不可用时回退软件解码
- [x] Linux XDG desktop portal 文件打开
- [x] 顶部改为菜单栏（File / View / Help）
- [x] 路径显示改为标题 label，只显示不含扩展名的文件名
- [x] 预览区点击切换播放 / 暂停
- [x] 预览区与时间轴滚轮按关键帧步进，mod + 滚轮步进 10 秒
- [x] 后台关键帧索引（FFprobe packet flags，不解码视频）
- [x] 自定义时间轴与精确播放头
- [x] 侧栏播放信息（mpv Tab 风格）显示在媒体信息上方
- [x] 流信息默认折叠，并由 egui memory 记录状态
- [x] 设置页：主题下拉框 + 跟随系统复选框，语言选择
- [x] UI 本地化：`locales/*.json` 编译期嵌入，运行时选择语言
- [x] 使用 eframe storage 持久化主题、语言与 egui memory

## 已知限制

- A/V 同步由 mpv 的 `time-pos` 驱动，它跟随音频输出；仍受设备缓冲延迟影响（通常数十毫秒）。
- 运行环境需要 `ffmpeg`、`ffprobe`、libmpv 与音频输出设备；不再需要 `ffplay`。
- 音频输出由 libmpv `ao` 完成；`cpal` 仅在外部 FFmpeg 回退路径使用。
- 硬件解码由 libmpv 自动选择（`hwdec=auto-safe`）；设备不可用时回退软件。
- 播放信息区分 `mpv (libmpv)` 与外部 `ffmpeg (...)`；mpv 内部实际 decoder 不单独报告。
- 关键帧索引在打开媒体后于后台生成；索引完成前滚轮回退为 1 秒步进。
- 时间轴尚未按 Segment 着色；播放头与拖动已实现，Segment 颜色随 Phase 3 落地。
- 4K HEVC 精确 seek 仍可能耗时数百毫秒；测试机 CPU 受限，不能代表一般硬件。快速 seek 调研见下节。
- 快速 scrub 可能显示目标时间之前的关键帧；只能用于视觉反馈，不能用于 Segment 或导出。
- 预览尺寸按预览窗格与源宽高比等比适配，不放大；预览不保证帧精确，不作为编辑或导出依据。
- 暂无音量、音频设备选择、音轨选择。
- UI 字符串已本地化；应用层 warning 与媒体层错误仍为英文。
- 文件门户仅在 Linux 实现；其他平台暂时禁用 Open。
- 文件对话框未做自动化选择测试；已确认 XDG portal 服务与 `FileChooser` 接口存在。
- 浅色主题通过截图与 egui Context 测试验证；中文界面通过截图验证。

验证记录（2026-10-24）：

- `./scripts/check.sh` 通过，实际执行 `cargo fmt --all --check`、`cargo check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`、`cargo build`。
- 单元测试 46 个、集成测试 14 个，全部通过；2 个音频测试默认 ignored。
- 集成测试覆盖：首帧解码、从 0.5s seek 解码、解码失败路径、快速 scrub、应用层播放 / 暂停 / seek、关键帧索引与关键帧导航。
- `cargo test --test preview_audio -- --ignored --nocapture --test-threads=1` 在带音频设备的会话中通过（2 个测试，1.94s）。
- GUI 实际加载真实 4K HEVC 素材（3840x2160, 59.94fps）并截图：菜单栏、标题 label、只读媒体信息、默认折叠的流信息、时间轴播放头与 `ffmpeg (vaapi)` 解码器状态均正常显示。
- 实际运行中后台索引了该文件的 265 个关键帧。
- 中文界面通过修改持久化语言设置后实际启动截图验证；设置窗口中的主题下拉框与语言选择可见。
- 硬件解码命令构造由单元测试覆盖（VAAPI 参数与 render node）；真实文件的 `-loglevel verbose` 输出确认选择 `hevc` 解码器并初始化 radeonsi VAAPI。
- 滚轮方向由 `wheel_direction` 单元测试覆盖；实际滚轮事件无法在无头环境注入，方向按用户反馈修正。
- 预览区空状态 / 失败状态点击打开文件已实现；未做自动化点击验证。
- `.github/workflows/ci.yml` 尚未在 GitHub Actions 环境实际执行。

## 预览性能与音画同步待办（Phase 2 后续，未开始）

### 快速 seek 调研（LosslessCut 3.70.1）

事实：

- LosslessCut 的预览不是外部 ffmpeg CLI 逐次 seek；`package.json` 没有 mpv 依赖。
- `src/renderer/src/MediaSourcePlayer.tsx` 使用 Chromium HTML5 `<video>` + Media Source Extensions：
  - master video 负责播放、`currentTime` 与音频；
  - 主进程的 `compatPlayer.createMediaSourceStream` 持续把 FFmpeg 输出写成 fragmented MP4，再 append 到 `SourceBuffer`；
  - seek 时 abort 当前流并用新的 `seekTo` 重启 ffmpeg，debounce 500ms；
  - 预览可限制到 800/420px、30/15fps，降低解码与上传成本；
  - 通过 `sourceBuffer.timestampOffset = seekTo - frameDuration` 与 30ms 间隔的 `currentTime` 同步。
- 来源：
  - https://github.com/mifi/lossless-cut/blob/master/src/renderer/src/MediaSourcePlayer.tsx
  - https://github.com/mifi/lossless-cut/blob/master/package.json

候选优化（未决定，按成本排序）：

1. 保持外部 FFmpeg，继续降低预览成本：
   - VAAPI 解码 + `scale_vaapi`，只下载缩小后的帧；
   - 降低预览分辨率 / fps，或生成低分辨率代理文件；
   - 拖动时只做 keyframe 近似，松开后再精确 seek（已实现）。
2. 换 libmpv：
   - 可能同时改善 seek、硬件解码与 A/V 同步；
   - 需要把 mpv 渲染进 egui，涉及 GL/Vulkan 互操作、窗口嵌入与跨平台。
3. 其他持久化解码器：GStreamer / libav 绑定，暂不推荐。

### 音画同步

- 当前以 cpal 实际消费的样本帧为时钟；cpal 回调写入设备缓冲不等于已经被听到，时钟可能领先一个输出缓冲周期。
- 待验证 / 待优化：
  - 减小 cpal 输出缓冲；
  - 从播放位置中扣除设备缓冲延迟；
  - 等待首个视频帧就绪后再启动音频；
  - 用真实文件做可量化的 A/V 偏移测试。
- 如果优化后仍不可接受，再评估 libmpv。

### libmpv 与 unsafe（待评估）

- libmpv 的渲染 API 需要把 mpv 的帧渲染到 egui 可用的纹理，通常要 OpenGL/Vulkan 互操作；如果由本项目代码调用 GL/FFI，会引入 `unsafe`，违反 `AGENTS.md`。
- 纯 JSON IPC（外部 mpv 进程）不引入 `unsafe`，但难以把视频嵌入 egui；Wayland 下尤其受限。
- 结论：换 libmpv 前必须先做最小实验并记录 ADR；当前不换。

## 进入 Phase 3 前注意事项

- Segment 边界必须使用精确时间与 FFprobe 数据，不得使用快速 scrub 的关键帧近似位置。
- 时间轴播放位置必须读取应用层的音频时钟；不要用 UI 墙钟自行推算播放时间。
- 关键帧索引已存在（`KeyframeIndex`），可作为 Segment 边界与导航的候选；但 keyframe 不等于用户期望的精确切点。
- 时间轴已按 Segment 着色并显示精确播放头；Segment 边界拖拽尚未实现。
- 硬件解码默认启用。不要为了预览计时而在后续阶段默认关闭它；预览解码路径与导出路径必须保持分离。
- 4K HEVC 精确 seek 可能耗时数百毫秒；timeline 交互需要接受该延迟，或先引入代理文件 / GPU 零拷贝预览方案。
- 保持 `Timestamp` 为整数微秒；UI 只做坐标转换，不引入浮点作为媒体时间表示。
- 预览不保证帧精确，不能作为编辑或导出精度的依据。

---

# Phase 3：Timeline

状态：`基础版完成`

## 目标

实现核心时间轴。

## 任务

- [x] Timestamp
- [x] Segment
- [x] Timeline
- [x] 当前播放位置
- [x] 时间轴按 Segment 着色（不同 Segment 不同颜色）
- [x] 精确播放头与 Segment 边界联动
- [x] In 点
- [x] Out 点
- [x] Segment 创建
- [x] Segment 删除
- [x] Segment 修改（从播放头设置起点 / 终点）
- [x] Segment 排序
- [x] Segment 启用/禁用
- [x] 键盘操作

## 实现记录

- `src/domain/segment.rs`：`Segment`、`SegmentId` 与 `Timeline`；整数微秒，`start < end` 不变式；`Vec` 顺序即输出顺序，新 Segment 追加，按起点排序是显式操作。
- 应用层：in/out 标记、按 `SegmentId` 选中 / 编辑 / 删除 / 上移 / 下移 / 排序 / 启停；命令错误返回 `SegmentCommandError`。
- UI：右侧 Segment 列表（输出顺序，可上移 / 下移）、底部时间轴彩色 Segment、入点 / 出点标记、精确播放头。
- 快捷键：`Space` 播放 / 暂停，`I` 入点，`O` 出点，`Enter` 添加 Segment，`Delete` 删除选中 Segment，`Esc` 清除标记。
- 时间轴点击会 seek，并按 `SegmentId` 选中点击位置所在的 Segment；Segment 行点击会 seek 到其起点。

## 已知限制

- Segment 允许重叠；时间轴按列表顺序绘制，后绘制的 Segment 可能覆盖先绘制的。重叠不自动拒绝、裁剪或合并。
- 列表顺序是输出顺序；修改起止点不会自动按起点重排，`Sort` 是显式操作。
- 尚未在时间轴上拖拽 Segment 边界或拖拽重排；当前用“上移 / 下移”按钮改变输出顺序。
- Timeline 绑定单一源媒体；打开新文件会清空 Segment。项目文件持久化尚未实现。
- Segment 未参与导出；Phase 5 将按 Timeline 顺序解释 enabled Segment。

验证记录（2026-10-24）：

- `./scripts/check.sh` 通过，实际执行 `cargo fmt --all --check`、`cargo check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`、`cargo build`。
- 单元测试 58 个、集成测试 22 个，全部通过；2 个音频测试默认 ignored。
- 集成测试 `tests/timeline.rs` 覆盖追加顺序、显式排序、按 ID 选中 / 编辑 / 删除、上移 / 下移、启用 / 禁用、非法标记、EDL 顺序护栏（B → C → A 不会被自动排序）与关闭媒体时清空。
- GUI 实际加载真实 4K HEVC 文件并用临时种子 Segment 截图：不同颜色、入点 / 出点标记、Segment 列表、Move up / Move down 按钮与按 `SegmentId` 选中均正常显示；截图后已移除种子代码。
- 未自动化测试时间轴拖拽与真实键盘输入；快捷键逻辑由代码路径保证，未注入真实按键。

---

# Phase 4：播放链路调查与渲染架构

状态：`Phase 4A 已完成；Phase 4B 已修订；Phase 4C 步骤 1–8 完成（旧路径保留）；Phase 4D 单 Segment 完成`

Phase 4 不再直接进入导出。当前实际播放性能与音画同步不可接受，先完成播放链路调查和渲染架构决策，再恢复无损导出。

## Phase 4A：播放器性能调查

目标：用测量而不是猜测，判断瓶颈属于 decoder、frame conversion、GPU upload、egui、wgpu、presentation 还是 clock/scheduler。

任务：

- [x] 确认当前 FFmpeg / VAAPI 解码路径与 ffmpeg 参数。
- [x] 确认 frame PTS / DTS / time base 与预览时间戳的关系。
- [x] 确认 audio clock 的实现；设备缓冲提前量未测（见调查文档“尚未验证”）。
- [x] 测量 A/V drift（`audio_clock - presented_video_pts`）。
- [x] 测量 decode → texture upload latency。
- [x] 测量 egui frame / render cadence。
- [x] 测量实际 presentation cadence。
- [x] 与 mpv 对照。
- [x] 执行 A/B/C/D 实验：
  - A：FFmpeg software → egui/wgpu
  - B：VAAPI → egui/wgpu
  - C：VAAPI → mpv
  - D：software → mpv
- [x] 输出调查文档：`docs/investigations/playback-pipeline.md`。

实测结论（详见调查文档）：

- 4K HEVC 下 VAAPI 路径只有 10.4 fps，`delta` 在 40 秒内增长 11.6 秒。
- 同一文件软件解码 29.8 fps，稳定后 `delta` ≈ 0~10 ms。
- FFmpeg 微基准显示瓶颈是 VAAPI 帧下载到系统内存：帧全程留在 GPU 时 86 fps，一旦 `hwdownload` 即使已缩到 960x540 也只有 20 fps。
- 纹理上传 0.01 ms、egui `ui_fps` 40+，均不是瓶颈。
- mpv 在相同文件与 VAAPI 下 33 fps，因为它保持 GPU 帧；外部 FFmpeg 进程 + 管道模型结构上必须同步下载。
- 1080p H.264 下 VAAPI 有 2.38x，可用；问题集中在 4K HEVC。

测量工具：

- `examples/phase4a_probe.rs`：绕开 egui 驱动 `Application`，输出帧率、呈现间隔与 A/V delta。
- `ROSS_RESCUT_DIAG`：GUI 渲染统计（每 2 秒）。
- `ROSS_RESCUT_DIAG_AUTOPLAY`：诊断时自动 seek 到 60s 并播放。
- `ROSS_RESCUT_PREVIEW_HW`：覆盖本次运行的解码后端。

未完成：Windows D3D11VA / macOS VideoToolbox 的下载开销；VAAPI 下载慢的具体原因（未 profiling）；分钟级长时漂移。

约束：

- 不继续增加 egui 视频功能。
- 测量工具可以临时、粗糙，但必须可重复运行。
- 诊断日志不得影响默认行为。

## Phase 4B：确定视频渲染架构

状态：`已修订（ADR 0010）；实现未开始`

原决策 ADR 0009 的方案 D（进程内 `libavformat` + `libavcodec` + `media/platform`）
已被 ADR 0010 取代。当前决策是方案 F：`mpv-engine` 的 `export` + `wgpu`，
libmpv 负责解码、音频与 seek；导出、裁切、合并与探测继续使用外部 FFmpeg / FFprobe。

项目代码保持 `unsafe_code = "forbid"`；GPU 句柄只在 `media::preview` 与
UI 渲染适配层之间按帧传递。详见 `docs/decisions/0010-preview-via-mpv-engine.md`。

被否决的方向（相对方案 F）：

- 外部 FFmpeg + `pipe:1`：无法交付 GPU-resident 帧。
- libmpv SW render：有 GPU → CPU 回读。
- libmpv OpenGL + eframe glow：需要切换渲染栈，GL 状态共享复杂。
- 自建 libav* + `media/platform`（原方案 D）：工程量与平台风险更高。
- 自写 demux + 平台媒体 API：容器解析工程量过大。
- Slint：UI 框架不是本问题的层面。

实现任务见 Phase 4C。

## Phase 4C：预览解码实现（方案 F）

状态：`步骤 1–8 完成；旧路径保留为回退`

任务：

- [x] 步骤 1：加入 `mpv-engine` 依赖、CI 系统包、headless 链接/平台测试。
- [x] 步骤 2：新增 `media::preview::mpv`，实现引擎生命周期、export attach、
  帧获取、命令、事件与错误；UI 尚未接入，外部 FFmpeg 回退仍在。
- [x] 步骤 3：UI 接入 wgpu feature、mpv 纹理注册、Application mpv 路径与
  外部 FFmpeg 回退。运行时已验证 4K mpv 纹理按预览窗格等比适配、`uploads=0`；
  feature 关闭时回退外部 FFmpeg、`uploads>0`。颜色/生命周期自动化回归见步骤 8。
- [x] 步骤 4：音频切换为 mpv `ao`；`Application::uses_audio_clock` 报告 mpv
  音频路径，`PreviewAudio` / `cpal` 只在外部 FFmpeg 回退路径启动。
- [x] 步骤 5：`hr-seek=no` 拖动 / `hr-seek=yes` 松手；松手使用指针最终位置，
  精确 seek 在 `PlaybackRestart` 缺失时以落点 ≤100 ms 兜底。
  `tests/mpv_preview.rs` 覆盖落点与暂停恢复。
- [x] 步骤 6：按预览窗格物理像素等比适配源尺寸（横竖屏均覆盖、不放大）；
  尺寸连续变化时等连续两帧一致后再下发，避免窗口拖动时每帧重配置 export。
- [x] 步骤 7：播放信息区分 `mpv (libmpv)` 与外部 `ffmpeg (...)` 后端；
  无 wgpu render state 时记录回退日志，mpv 失败通过 warning 显示。
- [x] 步骤 8：测试与文档修订。新增 `tests/mpv_gpu_regression.rs`（BGRA 通道、
  不偏色、drop 后纹理可用、连续实例不串帧）与 `tests/mpv_preview.rs`
  （seek / 暂停恢复 / 音频时钟），均默认 `ignored`。AGENTS、architecture、
  ADR 0010 已同步。旧预览路径**保留**：它同时承担无 feature / 无 wgpu /
  mpv 失败三条回退，删除条件见 ADR 0010「旧预览路径的退出条件」。

完成标准：

- 4K HEVC 在 VAAPI 下保持 GPU-resident，无每帧同步下载。
- 硬件不可用时自动回退软件，且 UI 可见。
- 预览时间只用于视觉反馈，不得用于 Segment 边界或导出；实际编辑时间来自源流。
- `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test` 全部通过。

未解决问题见 `docs/open-questions.md`。

## Phase 4D：无损导出（原 Phase 4C / 原 Phase 4）

状态：`单 Segment 已实现；多 Segment 归入 Phase 5`

## 目标

使用 FFmpeg 实现无损 Segment 导出。

## 任务

- [x] FFmpeg command builder（`media/export/command.rs`）
- [x] ExportJob（`application/export.rs`）
- [x] FFmpeg process runner（`ExportProcess`）
- [x] progress parser（`-progress pipe:1`）
- [x] cancellation
- [x] 单 Segment 导出
- [ ] 多 Segment 导出（归入 Phase 5）
- [x] 输出文件验证（codec / 分辨率 / 采样率 / 声道数 / 时长）
- [x] 回归测试（`tests/export.rs`、`tests/application_export.rs`）

实现语义：

- `-ss` 在 `-i` 前，快速 seek，实际起点回退到此前关键帧。
- `-t` 在 `-i` 后，使用 Segment 时长。
- `-map 0 -c copy`，不重新编码。
- 切开头时加 `-avoid_negative_ts make_zero`。
- 进度使用 `-progress pipe:1`，不依赖 stderr 文本。
- 输出路径不能等于源路径；UI 使用 save dialog，默认文件名带 `_export`。

验证记录（2026-10-26）：

- `tests/export.rs`：生成的 1080p30 H.264/AAC 素材，`[2s, 6s)` 导出后 ffprobe 对比 codec、分辨率、采样率、声道数一致；时长约 4 秒。
- `tests/application_export.rs`：走完整 `Application` 流程（open → in/out → add_segment → start_export → poll），状态为 `Completed`。
- `cargo test`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo fmt --check` 全部通过。
- 未做自动化 UI 交互测试；菜单与进度条通过编译与代码路径验证。
- 未用 4K60 素材重复测试；按产品要求以 1080p30 为日常回归素材。

## 研究任务（不复制代码）

- [ ] 研究 LosslessCut 单 Segment lossless export：FFmpeg 参数、`-ss` / `-to` / `-t`、stream copy、时间戳与音视频流处理。
- [ ] 验证普通无损切割的 keyframe 回退语义，并为实际回退行为增加回归测试。
- [ ] 区分产品行为、媒体处理算法、FFmpeg 参数与 Electron / Chromium 特定实现。

## 核心要求

不得静默从：

```text
stream copy
```

切换成：

```text
re-encode
```

---

# Phase 5：多片段合并

状态：`实现完成（单段 + 多段）`

调查文档：`docs/investigations/multi-segment-concat.md`。

已确认的语义：

- 采用“临时片段 + concat demuxer”；concat filter 会重编码，不用。
- concat demuxer 的 `inpoint/outpoint` 在非关键帧起点会破坏时间戳，不用。
- 每个 Segment 的起点会回退到此前关键帧；输出时长不等于 `sum(end - start)`。
- 拼接处存在 ≤1 帧的视频间隙（约 16–33ms），音频连续；stream copy 无法消除。
- 输出容器应与源一致；同一 source 的片段流布局一致。
- Timeline 顺序是唯一输出顺序，不得按 `start` 重排。

## 任务

- [x] Segment compatibility（同一 source 的 enabled segments）
- [x] 临时片段切片（`-ss` before `-i`、`-t`、`-map 0`、`-c copy`、`-shortest`）
- [x] concat demuxer 合并
- [x] 临时目录生命周期（成功 / 失败 / 取消都清理）
- [x] 多段进度（按各段请求时长加权）
- [ ] stream selection
- [ ] timestamp 处理（切片已用 `-avoid_negative_ts make_zero`；更细控制未做）
- [ ] metadata / chapter 显式处理
- [x] 不兼容情况处理（FFmpeg 错误向上传递并显示）
- [x] 顺序护栏测试（导出必须读取 Timeline 既有顺序，不得按 start 重排）
- [x] 回归测试（含非关键帧起点、重叠 Segment、取消）

验证记录（2026-10-26）：

- `tests/multi_export.rs`：多色源（红/绿/蓝各 5 秒）按蓝 → 红 → 绿导出，提取输出帧验证顺序；临时目录在成功与取消后都不存在。
- `tests/application_export.rs`：完整 `Application` 流程（open → 3 个 Segment → start_export → poll），输出顺序为 Timeline 顺序。
- `tests/export.rs`：单段 stream copy 不变，codec / 分辨率 / 采样率 / 声道数一致。
- `cargo test`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo fmt --check` 全部通过。
- 未做 4K / 60fps / VFR / 字幕 / 章节验证；见 `docs/open-questions.md`。

## 研究任务（不复制代码）

- [x] 研究 LosslessCut 的 concat / timestamp 处理与容器兼容性边界。
- [ ] 研究 audio-only 裁剪、leading non-keyframe audio packets 等历史边界修复。
- [ ] 对采用的媒体语义增加本项目自己的测试。
- [ ] 验证 60fps / 4K / VFR 素材的拼接间隙是否仍为一帧。
- [ ] 验证字幕、章节、attached_pic 与不同输出容器。

## 顺序语义

- 多 Segment 导出按 `Timeline.segments` 的顺序解释 enabled Segment。
- 导出代码不得对 Segment 重新按 `start` 排序；顺序由 Timeline 决定。
- 重叠 Segment 默认在输出中重复出现；不自动合并或裁剪。
- SegmentId 与 `next_id` 需要在项目持久化时保存。

## UI 交互修订（2026-10-26）

- 导出按钮移到主界面右下角；File 菜单不再放置单段导出入口。
- 时间轴单独一行，播放控制移到其下方，共 15 个符号按钮 + 两个时间输入框。
- 控制按钮使用 ASCII 符号（egui 默认字体不包含大部分媒体控制 emoji），全部带 tooltip。
- 切点交互：首次“添加切点”设置开始点；在其后再次添加自动生成 Segment；
  “删除切点”清除 pending 切点，没有 pending 时删除选中 Segment；
  “分割片段”以播放头为界。
- 单击时间轴选择片段；拖动播放头只移动位置，不改变选择。
- 导出章节：设置页开关，默认开启。章节按各 Segment 的请求时长累计生成。
- 关于窗口显示版本、git commit 与当前 Phase。
- 右侧媒体信息块已删除；只保留 Playback 与 Segments。
- 验证：`tests/application_export.rs` 覆盖切点自动创建、按播放头分割与删除；
  多段导出的 `ffprobe` 章节数断言通过；GUI 截图确认符号与布局。

---

# Phase 6：缩略图

状态：`未开始`

## 任务

- [ ] 缩略图生成
- [ ] 异步处理
- [ ] 缓存
- [ ] egui texture 管理
- [ ] 时间轴显示

---

# Phase 7：波形

状态：`未开始`

## 任务

- [ ] 音频数据获取
- [ ] waveform 数据模型
- [ ] 异步生成
- [ ] 缓存
- [ ] 时间轴绘制

---

# Phase 8：高级功能

候选：

- [x] Chapters（已提前实现导出章节；字幕与章节编辑仍未实现）
- [ ] Subtitle
- [ ] Stream selection
- [ ] Scene detection
- [ ] Batch processing
- [ ] Metadata
- [ ] Project persistence
- [ ] 快捷键配置
- [ ] 基于音频音量自动切分视频
- [x] 导出视频时包含章节信息（设置页开关，默认开启）
- [ ] 智能裁切(裁切点为非关键帧时, 从该帧到关键帧进行基于此段平均码率的重新编码)
- [ ] Smart cut 技术调查（只调查，不实现；输出独立文档 / ADR）

smart cut 在实现前必须先完成技术调查（LosslessCut 的边界判定、关键帧处理、重编码范围、FFmpeg 行为），形成独立 ADR；调查完成前不实现。

每个功能在实现前都需要独立设计。

---

# Phase 9：发布

## Linux

- [x] Release build 已验证（`cargo build --release`）
- [ ] 发布包说明运行时依赖：`libmpv.so.2`、`ffmpeg` / `ffprobe`、XDG desktop portal、EGL / GBM
- [ ] 打包格式（AppImage / flatpak / 发行版包）

## Windows

- [ ] Windows 文件打开 / 保存对话框；当前 `rfd` 仅作为 Linux 依赖，
  `FILE_DIALOG_SUPPORTED` 仅 Linux，导致「打开」与「导出」不可用
- [ ] Windows 版 libmpv：`libmpv-2.dll` 与 import library（GNU `.dll.a` 或 MSVC `mpv.lib`），DLL 随包分发
- [ ] Windows 版 FFmpeg / FFprobe 分发
- [ ] 构建方式：`x86_64-pc-windows-gnu` 交叉编译（`mingw-w64-gcc`）或 Windows 原生 MSVC；尚未实测
- [ ] D3D11VA 硬件解码与 wgpu 预览链路验证
- [ ] 导出路径选择、章节写入与多段 concat 验证

## macOS

- [ ] libmpv dylib 分发与签名
- [ ] VideoToolbox 硬件解码验证
- [ ] 公证与安装包

## 通用

- [ ] License
- [ ] 自动发布
- [ ] 安装包

---

# 开发原则

只处理：

> 当前阶段中最小的下一步。

不要一次实现整个 Phase。

每完成一个阶段：

1. 更新本文件。
2. 更新架构文档。
3. 更新测试。
4. 更新相关 ADR。
5. 确认 exit criteria。
