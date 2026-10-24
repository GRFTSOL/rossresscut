# 开发计划

## 当前阶段

**Phase 1：媒体分析（已完成）**

Phase 2 尚未开始。

除非用户明确要求，不提前实现后续阶段功能。

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

托管 CI：`.github/workflows/ci.yml`。Git 仓库已初始化，尚未提交。

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

状态：`未开始`

## 目标

实现流畅的视频预览。

## 任务

- [ ] 确定预览后端
- [ ] 设计 PreviewBackend
- [ ] 播放
- [ ] 暂停
- [ ] Seek
- [ ] 时间显示
- [ ] 错误处理
- [ ] 音频播放

## 注意

预览后端必须先进行架构评估。

不要因为“需要播放视频”就直接添加未经评估的播放器 crate。

---

# Phase 3：Timeline

状态：`未开始`

## 目标

实现核心时间轴。

## 任务

- [ ] Timestamp
- [ ] Segment
- [ ] Timeline
- [ ] 当前播放位置
- [ ] In 点
- [ ] Out 点
- [ ] Segment 创建
- [ ] Segment 删除
- [ ] Segment 修改
- [ ] Segment 排序
- [ ] Segment 启用/禁用
- [ ] 键盘操作

---

# Phase 4：无损导出

状态：`未开始`

## 目标

使用 FFmpeg 实现无损 Segment 导出。

## 任务

- [ ] FFmpeg command builder
- [ ] ExportJob
- [ ] FFmpeg process runner
- [ ] progress parser
- [ ] cancellation
- [ ] 单 Segment 导出
- [ ] 多 Segment 导出
- [ ] 输出文件验证
- [ ] 回归测试

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

状态：`未开始`

## 任务

- [ ] Segment compatibility
- [ ] concat
- [ ] stream selection
- [ ] timestamp 处理
- [ ] metadata 处理
- [ ] 不兼容情况处理
- [ ] 回归测试

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

- [ ] Chapters
- [ ] Subtitle
- [ ] Stream selection
- [ ] Scene detection
- [ ] Batch processing
- [ ] Metadata
- [ ] Project persistence
- [ ] 快捷键配置
- [ ] 基于音频音量自动切分视频
- [ ] 导出视频时包含章节信息
- [ ] 智能裁切(裁切点为非关键帧时, 从该帧到关键帧进行基于此段平均码率的重新编码)

每个功能在实现前都需要独立设计。

---

# Phase 9：发布

- [ ] Linux
- [ ] Windows
- [ ] macOS
- [ ] FFmpeg 分发方案(Windows Only, macOS和Linux基本都有合适的ffmpeg安装方式)
- [ ] License
- [ ] Release build
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
