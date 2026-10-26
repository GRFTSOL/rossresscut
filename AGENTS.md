# AGENTS.md

## 项目概述

本项目是一个使用 **Rust + egui** 开发的原生桌面媒体工具，目标是提供一个轻量、快速、低资源占用的 **LosslessCut 类替代品**。

项目核心职责：

- 媒体文件分析
- 视频预览
- 时间轴操作
- 片段选择
- 无损剪切
- 多片段导出
- 媒体流选择
- 媒体合并
- 基础媒体信息管理

媒体处理由 **FFmpeg / FFprobe** 负责。

图形界面由 **egui / eframe** 负责。

## 本文件的地位

本文件是项目内 **AI Agent 行为的唯一事实来源**。

- 人类贡献者同样应阅读本文件。
- 其他文档（`GOALS.md`、`PLANS.md`、`docs/`）与本文件冲突时，以本文件为准。
- 本文件描述的是"项目为什么这样组织、什么不能碰"，不是教程。
- 修改行为规则必须同时修改本文件，不允许只在对话里约定。

---

# 一、最高优先级原则

## 1. 保持原生 Rust 架构

本项目使用：

- Rust
- egui
- eframe
- FFmpeg
- FFprobe

禁止引入以下技术作为项目核心架构：

- Electron
- JavaScript
- TypeScript
- React
- Vue
- WebView
- Tauri
- 浏览器作为核心 UI
- Node.js 运行时
- 其他 Web UI 框架

不要因为某个功能实现起来方便，就把 Web 技术重新引入项目。

---

## 2. 保持架构简单

优先使用：

- Rust 标准库
- 已有依赖
- 简单的数据结构
- 明确的模块边界
- 直接的数据流

避免：

- 过度抽象
- 无实际需求的 trait
- 无实际需求的泛型
- 无实际需求的事件总线
- 无实际需求的 service locator
- 无实际需求的依赖注入框架
- 为未来假想需求提前设计复杂架构

### 核心原则

**代码应该服从问题，而不是为了展示架构技巧制造问题。**

### 新增模块

新增模块同样需要说明归属：

1. 它属于哪一层（UI / Application / Domain / Media）。
2. 它依赖谁、被谁依赖。
3. 为什么现有模块不能承载这段逻辑。

模块数量不是目标，边界清晰才是。

---

# 二、Agent 工作方式

## 0. 语言

为节省 token，思维过程和回答均使用简体中文；代码、标识符、命令、日志与专有名词不强行翻译。

---

## 1. 修改代码之前先理解项目

处理任何非简单修改时，必须先：

1. 查看相关目录。
2. 查看相关模块。
3. 查看已有的数据模型。
4. 查看已有的命令或应用层逻辑。
5. 查看相关测试。
6. 查看相关文档。
7. 判断当前架构是否已经存在对应能力。

不要在没有阅读现有代码的情况下直接创建新的：

- struct
- trait
- module
- service
- manager
- event
- command
- dependency

---

## 2. 不要猜测项目现状

如果不知道某个 API、依赖、FFmpeg 行为或项目内部实现：

- 搜索代码。
- 查看官方文档。
- 查看实际命令输出。
- 运行测试。
- 必要时创建最小实验。

不要根据记忆猜测。

尤其不要假设某个 Rust crate 存在某个 API。

---

## 3. 不要扩大任务范围

用户要求实现一个功能时：

- 只实现相关功能。
- 不要顺便重构整个模块。
- 不要顺便更换依赖。
- 不要顺便重新设计 UI。
- 不要顺便建立新的架构层。
- 不要实现用户没有要求的未来功能。

如果确实发现现有架构阻碍实现，应先分析原因，再进行最小范围修改。

---

## 4. 参考实现：LosslessCut

LosslessCut 是本项目的**行为参考实现**和**工程资料库**，不是本项目的架构或代码模板。

```text
LosslessCut
    │
    ├── 产品行为       ← 可以参考
    ├── 媒体语义       ← 强烈参考
    ├── FFmpeg 参数    ← 可以研究
    ├── 边界条件       ← 强烈参考
    ├── 用户交互       ← 可以参考
    │
    ├── TypeScript 架构 ← 不照搬
    ├── Electron        ← 不采用
    ├── Chromium video  ← 不采用
    └── Web UI          ← 不采用
```

可以 clone LosslessCut 主分支源码用于研究。重点研究：

- 单 Segment lossless export 的 FFmpeg 参数构造，尤其是 `-ss`、`-to` / `-t`、stream copy、时间戳与音视频流处理。
- keyframe 边界、非关键帧切点、smart cut 的边界判定与重编码范围。
- PTS、DTS、time base、start time、negative timestamp、VFR、concat 后的时间戳。
- 音频 / 视频不同起始位置、audio-only 裁剪、leading non-keyframe audio packets。
- 容器兼容性、metadata、字幕、chapter。
- 历史 issue / commit 中的具体边界 bug 与修复策略。

LosslessCut 官方说明：普通无损切割受 keyframe 限制，实际切点可能回退到此前的 keyframe。
这是 Phase 4 必须理解并向用户如实说明的行为语义。

### 研究方式

正确的路径：

```text
LosslessCut source
        ↓
理解它为什么这样做
        ↓
FFmpeg 行为验证
        ↓
转换成 Rust / domain / application / media 设计
```

错误的路径：

```text
LosslessCut TypeScript
        ↓
翻译成 Rust
```

后者容易得到“TypeScript 穿 Rust 外套”，且可能把 Electron / Chromium 特定行为带进本项目。

### 研究顺序

1. 本项目已有代码 / ADR。
2. FFmpeg 官方文档。
3. LosslessCut 实现与 issue / commit。
4. 实验验证。
5. 设计本项目实现。

### 许可证约束

LosslessCut 使用 GPL-2.0。允许：

- 阅读源码，理解产品行为与媒体语义。
- 查 FFmpeg 行为并做最小实验。
- 自己设计 Rust 实现与测试。

禁止：

- 复制 LosslessCut 源代码。
- 把 TypeScript 函数逐行翻译成 Rust。
- “只改语法”的移植。

### Investigate LosslessCut behavior for <问题>

1. 检查当前 LosslessCut 源码。
2. 找到相关实现、commit、issue 或文档。
3. 说明它解决的具体问题。
4. 区分：产品行为、媒体处理算法、FFmpeg 参数、Electron / Chromium 特定实现。
5. 检查哪些部分适用于本项目。
6. 不复制 LosslessCut 源代码。
7. 不改变本项目 Rust + egui + FFmpeg 架构约束。
8. 如果结论会影响 Domain / Application / Media 架构，进入 `architecture-decision` 流程。
9. 对关键结论增加本项目自己的测试。

---

# 三、工程命令与工具链

Cargo 工程建立之后，以下命令是本地与 CI 的共同准入门槛：

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo fmt --all --check
```

## 规则

- 提交前上述命令必须全部通过。
- `--no-default-features` 的 clippy 用于验证无 `mpv-preview` 时的外部 FFmpeg 回退路径仍可编译；它不运行需要 libmpv 的测试。
- 编译警告零容忍：`unused import`、`dead_code`、`deprecated API` 都必须为零。
- 格式问题不靠人工审查，交给 `cargo fmt`；`cargo fmt --all --check` 失败即视为未完成。
- 工具链版本由 `rust-toolchain.toml` 固定，避免不同机器行为差异；升级工具链属于独立变更。
- 命令块中的参数与 `clippy` 的 `-D warnings` 不得为了绕过报错而删减。

## 开发环境

本节只记录**会改变构建与验证行为**的机器约定，不是产品支持范围。

- **操作系统**：Arch Linux（滚动发行版）。系统包用 `pacman` 安装，例如 `sudo pacman -S ffmpeg`；不通过脚本直接下载系统级二进制。
- **Rust 工具链**：由 `rustup` 管理，当前为 `stable-x86_64-unknown-linux-gnu`，`rustfmt` 与 `clippy` 已就位。缺组件时用 `rustup component add rustfmt clippy`。
- **工具链固定**：仓库建立后应由 `rust-toolchain.toml` 固定版本；升级工具链是独立变更，不与功能改动混在一起。
- **FFmpeg / FFprobe**：使用系统包（`/usr/sbin/ffmpeg`、`/usr/sbin/ffprobe`；`ffprobe` 由 `ffmpeg` 包提供，不是独立包），项目不自带二进制。滚动发行版会改变实际版本，因此测试不得假设固定版本行为。
- **桌面会话**：Wayland（`XDG_SESSION_TYPE=wayland`，合成器为 niri），没有 X11 会话。egui / winit 的行为验证必须在 Wayland 下进行，不依赖 X11 专属路径。
- **交互 shell**：fish。脚本不依赖交互 shell 配置：显式写 shebang（`#!/usr/bin/env bash` 或 `#!/usr/bin/env fish`），并保证在非交互环境下可运行。
- **辅助脚本**：只用于开发与验证，不进入产品运行时；项目不引入 Python / Node 运行时依赖。

产品支持平台仍以 `GOALS.md` 与 `PLANS.md` 中的 Linux / Windows / macOS 为准。本节描述的是当前开发机的验证环境，不能作为"只支持 Arch Linux"的依据。

## 系统包与权限边界

### 硬约束：agent 无法提权

当前 agent 运行环境设置了 `NoNewPrivs: 1`，且 **CapEff / CapPrm / CapBnd 全为 0**。
`sudo`、`pkexec`、`su` 都无法获得 root：

```text
sudo: The "no new privileges" flag is set, which prevents sudo from running as root.
```

这是运行环境的既定限制，不是配置遗漏，也不允许尝试绕过。

**结论：系统级安装一律由用户手动执行。**

### agent 的职责

1. 只读检测：

   ```bash
   command -v ffmpeg ffprobe
   pacman -Q ffmpeg
   ```

2. 缺失时报告，并给出可直接复制的命令：

   ```bash
   sudo pacman -S --needed ffmpeg
   ```

3. 用户确认完成后复检版本与路径，再继续后续工作。

### 禁止

- 尝试 `sudo`、`pkexec`、`su -c`、`sudo -S` 读密码、`sudo -A` 配合自建 askpass。
- 修改 `/etc/sudoers`、`/etc/sudoers.d/*`、`/etc/sudo.conf`，或建议为用户机器配置 `NOPASSWD` 以便 agent 提权。
- 在用户未明确要求时执行 `pacman -Syu`、`pacman -R*`、`systemctl` 修改系统服务。
- 把任何密码写入脚本、环境变量、日志或提交到仓库。

### 需要 root 的构建依赖

如果某个 crate 需要系统开发库，同样按上述流程：报告包名，由用户安装，agent 只做检测。

项目自身的 Rust 依赖与项目内脚本不得要求 root。

### 包与文件归属查询

本机安装了 **`pacfiles` 0.3.0-1**（来自 `archlinuxcn` 仓库，依赖 `plocate`），
它是 `pacman -F` 的快速替代，查询范围覆盖仓库，不限于已安装包。

```bash
# 文件 → 包，支持 glob
pacfiles '*/ffprobe'
pacfiles 'usr/bin/ffprobe'

# 只输出包名，便于脚本消费
pacfiles -q usr/bin/ffprobe

# 包 → 文件
pacfiles -l ffmpeg

# POSIX 扩展正则
pacfiles -x '.*/libav(codec|format)\.so'
```

**用途**：在报告"缺依赖"之前，先确认某个命令、头文件或共享库由哪个包提供，
以及现有包是否已经包含它。例如 `usr/bin/ffprobe` 属于 `extra/ffmpeg`，不需要单独安装。

**边界**：

- `pacfiles` 只读查询，可以作为默认排查工具；它不安装、不修改系统。
- 数据库陈旧时，刷新需要写 `/var/lib/pacman`，由用户在宿主终端执行 `sudo pacman -Fy`。
- 换机器或未安装 `pacfiles` 时回退：

  ```bash
  pacman -Ql <pkg>             # 包 → 文件，仅限已安装包
  pacman -Fq usr/bin/ffprobe   # 文件 → 包，需要 pacman 文件数据库
  ```

- 本机 `pacman -F` 会报第三方仓库（`arch4edu`）数据库不一致；文件查询优先用 `pacfiles`，
  同时把该告警报告给用户，不要静默忽略。
- 查询只用于决定"是否需要装包"，**安装仍然走本节开头的手动流程**。

---

# 四、架构

项目采用分层架构：

```text
┌─────────────────────────────┐
│             UI              │
│          egui / eframe      │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│        应用层 Application    │
│     Command / Job / State   │
└──────────────┬──────────────┘
               │
               ▼
┌─────────────────────────────┐
│          领域层 Domain       │
│ Media / Timeline / Segment  │
└──────────────┬──────────────┘
               │
        ┌──────┴──────┐
        ▼             ▼
┌──────────────┐ ┌──────────────┐
│ 媒体层 Media │ │ 项目层 Project│
│ FFmpeg       │ │ Persistence  │
│ FFprobe      │ │              │
└──────┬───────┘ └──────────────┘
       │
       ▼
┌─────────────────────────────┐
│      FFmpeg / FFprobe       │
│ 导出 / 探测：外部进程         │
│ 预览解码：libmpv（进程内）   │
└─────────────────────────────┘
```

---

# 五、依赖方向

允许：

```text
UI
 ↓
Application
 ↓
Domain
 ↓
Media
 ↓
FFmpeg（外部进程：导出 / 探测）
libmpv（进程内：预览解码与音频）
```

禁止：

```text
Domain → UI
Domain → egui
Media → UI
Media → egui
FFmpeg → UI
media::platform 的 FFI / GPU 句柄类型 → Domain / Application / UI
```

### 核心原则

**核心媒体逻辑不能依赖 egui。**

领域层和媒体层必须能够在不启动图形界面的情况下进行测试。

---

# 六、UI 规则

## 1. egui UI 不能阻塞

禁止在 egui 每帧更新逻辑中直接执行长时间任务。

包括：

- FFmpeg
- FFprobe
- 视频解码
- 大量文件读取
- 缩略图生成
- 波形生成
- 场景检测
- 大规模数据处理
- 阻塞式进程等待
- 阻塞式 channel 接收

这些工作必须放到 UI 帧之外。

---

## 2. UI 不负责业务逻辑

UI 负责：

- 显示状态
- 获取用户输入
- 发送命令
- 显示进度
- 显示错误

UI 不负责：

- 构造 FFmpeg 命令
- 解析 FFprobe JSON
- 计算媒体时间
- 判断片段是否合法
- 执行媒体导出
- 操作底层文件

---

## 3. UI 状态与领域状态分离

区分：

### 领域状态

例如：

- 当前媒体
- 时间轴
- 片段
- 流选择
- 导出配置

### 应用状态

例如：

- 当前任务
- 导出进度
- 错误状态
- 取消状态

### UI 状态

例如：

- 当前拖动的控件
- 当前悬停位置
- 面板展开状态
- 临时输入内容

**UI 状态不能成为领域状态的唯一来源。**

---

## 4. 状态变更必须经过应用层

UI 不允许直接修改领域状态。

- 所有改变领域状态的操作都必须表达为应用层命令，并具备 `enabled` 判断与失败原因。
- UI 只负责构造参数、调用命令、展示结果。
- 命令之外的代码路径不得 mutate 领域状态。

---

## 5. 桌面集成

- Linux 文件打开必须使用 XDG desktop portal（`rfd` 的 `xdg-portal` 后端），不自建文件选择器。
- 顶部显示当前媒体标题标签，只显示文件名主体，不含扩展名；不提供可编辑路径输入框。
- 主题默认跟随系统，并允许在设置页手动选择 Dark / Light。
- 设置页提供主题与界面语言选择；设置的持久化使用 eframe storage。
- 侧栏播放信息显示在媒体文件信息上方，参考 mpv Tab 信息。
- 流信息默认折叠，折叠状态由 egui memory 记录。

---

# 七、媒体处理

## 1. FFmpeg 是媒体处理后端

不要重新实现：

- 编解码器
- 容器解析
- 音视频编码
- mux
- demux
- 时间戳处理的完整替代实现

这些工作交给 FFmpeg。

两种调用方式并存：

- 导出、裁切、合并与探测：外部 FFmpeg / FFprobe 进程。
- 预览解码与音频输出：进程内 `mpv-engine`（libmpv）。

两者都必须使用 FFmpeg / libmpv 的能力，不允许自写编解码器或容器解析。预览的进程内边界与 zero-copy 规则见 `docs/decisions/0010-preview-via-mpv-engine.md`。

Rust 负责：

- 命令构造
- 参数验证
- 进程启动
- 进程管理
- 进度解析
- 取消
- 错误处理
- 媒体模型
- 应用层逻辑

---

## 2. FFmpeg 调用方式

导出、裁切、合并与探测必须直接执行外部程序：

```text
Command
 ↓
FFmpeg executable
```

避免：

```text
Shell
 ↓
FFmpeg
```

不要通过 shell 拼接用户输入。

文件路径、参数等必须作为独立进程参数传递。

预览解码与音频使用进程内 `mpv-engine`（libmpv），同样不得经由 shell，也不得在 egui 帧内同步调用。

---

## 3. 预览与音频

- 预览主路径使用 `mpv-engine`（libmpv）解码、渲染与音频输出；外部 FFmpeg 预览保留为回退路径。
- 播放位置使用 libmpv 的 `time-pos`，它跟随音频输出；无音轨时同一时钟仍然有效。
- 音频由 libmpv 的 `ao` 输出；`cpal` 只在外部 FFmpeg 回退路径使用，不得与 mpv 同时输出音频。
- 拖动时间轴使用 `hr-seek=no`（落在前一个关键帧）提供视觉反馈；松开时使用指针最终位置并切换 `hr-seek=yes` 做精确 seek。
- `PlaybackRestart` 事件缺失时，精确 seek 以 `position()` 落点兜底，容差为 100 ms。
- 快速 scrub 的位置是近似值，不得用于 Segment 边界、编辑结果或导出。
- 硬件解码由 libmpv 自动选择（`hwdec=auto-safe`）；设备不可用时自动回退软件。
- 预览导出尺寸按预览窗格大小与源宽高比等比适配，按物理像素下发，且不得超过源分辨率（不放大）。
- 解码失败或 mpv 启动失败必须可见；播放信息区分 `mpv (libmpv)` 与外部 `ffmpeg (...)` 后端。
- 预览区点击切换播放 / 暂停；滚轮按关键帧步进（上滚上一个，下滚下一个），按住 mod 键时步进 10 秒。
- 预览不保证帧精确；导出与编辑不得依赖预览时间。

---

# 八、FFprobe

FFprobe 输出必须尽快转换成明确的 Rust 数据结构。

推荐：

```text
FFprobe JSON
    ↓
解析器
    ↓
MediaFile
    ├── Format
    ├── VideoStream
    ├── AudioStream
    ├── SubtitleStream
    └── Chapter
```

不要让 `serde_json::Value` 在整个项目中到处传播。

FFprobe 的 JSON 只应该存在于媒体解析边界。

---

# 九、时间系统

媒体时间是本项目的核心数据。

## 禁止

不要在核心逻辑中使用：

```rust
f32
f64
```

作为唯一的媒体时间表示。

---

## 原因

视频编辑涉及：

- 帧边界
- 时间戳
- PTS
- DTS
- time base
- VFR
- 音视频同步
- 精确 seek
- 容器时间戳

浮点秒数容易产生精度和边界问题。

---

## 要求

项目必须定义明确的：

- `Timestamp`
- `Duration`
- `TimeBase`

并明确它们之间的转换规则。

具体设计记录在：

```text
docs/media-model.md
```

---

# 十、Segment

片段是用户编辑行为的核心数据。

概念模型：

```text
Segment
├── id (SegmentId)
├── start
├── end
└── enabled
```

片段必须能够脱离 UI 单独存在。

不能依赖：

```text
某个 egui widget
某个拖动状态
某个临时 UI 数值
```

来表达最终编辑结果。

当前实现将 `source` 绑定在 `Timeline` 上，而不是在每个 `Segment` 中重复保存。
一次只编辑一个源文件；打开新文件会清空 Timeline。

`Timeline.segments` 是**有输出顺序的 EDL**：列表顺序就是导出顺序，按起点排序是显式操作，不是不变量。
Segment 允许重叠，不自动拒绝、裁剪或合并。
选择、编辑、删除与重排必须使用稳定 `SegmentId`，不得依赖 Vec index。
详见 `docs/decisions/0008-segment-timeline-semantics.md`。

---

# 十一、无损编辑

项目的核心功能是**无损媒体编辑**。

默认情况下优先使用：

```text
stream copy
```

而不是重新编码。

如果某项操作需要重新编码：

- 必须明确知道原因。
- 必须明确告诉用户。
- 不允许静默切换到重新编码。
- 不能把重新编码操作标记成无损操作。

## 降级原因必须具体

降级原因必须是可展示给用户的具体文本，例如"切点不在关键帧上，smart cut 需要重新编码"，
不能只用一个布尔标志表示"会发生重编码"。

判断是否降级的那一层负责给出原因文本；UI 只负责展示，不自行拼接或改写原因。

---

# 十二、导出

导出任务至少应该包含：

```text
ExportJob
├── source
├── segments
├── stream_selection
├── output
├── mode
├── progress
├── cancellation
└── result
```

导出必须支持：

- 进度
- 错误
- 取消
- 输出文件检查

导出未成功结束之前，不得将任务标记为成功。

---

# 十三、错误处理

生产代码中不要无理由使用：

```rust
unwrap()
expect()
panic!()
todo!()
unimplemented!()
```

优先使用：

```rust
Result<T, E>
Option<T>
```

外部数据必须视为不可信：

- 媒体文件
- FFprobe 输出
- FFmpeg 输出
- 文件路径
- stream index
- timestamp
- codec
- container

损坏媒体应该产生可理解的错误，而不是导致程序 panic。

## 1. unwrap 的例外

`.unwrap()` / `.expect()` 仅在不变式保证 `Some` / `Ok` 时允许使用，并且必须在上一行
独占一行写明原因：

```rust
// INVARIANT: 调用方已校验 segments 非空。
let first = segments.first().unwrap();
```

不允许用 `expect()` 代替有意义的错误信息。

## 2. 错误必须携带上下文

错误信息至少包含：

- 失败的操作
- 相关文件路径
- 相关参数（stream index、时间戳等）

不允许吞掉 FFmpeg 的 stderr 后只返回"ffmpeg failed"。

## 3. 禁止吞错

不允许用以下方式静默丢失错误：

- `let _ = result;`
- 空 `match` 分支
- 只记录日志但返回 `Ok` 的 `Err` 分支

除非在同一位置说明为什么可以忽略。

---

# 十四、Rust 编码规范

## 1. 风格

- 4 空格缩进。
- 模块、函数、变量：`snake_case`。
- 类型、trait、enum：`CamelCase`。
- 常量、静态量：`SCREAMING_SNAKE_CASE`。
- 格式以 `cargo fmt` 为准，不手工对齐。

## 2. 表达

- 优先迭代器组合：`map` / `filter` / `collect`，而不是手写 `for` 加 `push`。
- 当循环更清楚时（多状态、提前退出、副作用顺序敏感）使用循环，不为了风格牺牲可读性。
- 避免不必要的克隆、分配和中间集合。
- 抽象必须由真实复用驱动，不为"看起来更通用"引入 trait 或泛型。

## 3. 内存安全：项目代码禁止 unsafe

项目内 crate 必须保持 `unsafe_code = "forbid"`，禁止 `unsafe`、FFI、裸指针、
`transmute`、`union`、`MaybeUninit`、`ManuallyDrop` 与 `static mut`。

预览使用外部 crate `mpv-engine`（封装 libmpv）与 `wgpu`，它们的 `unsafe` 由依赖内部承担，
不得因此在本项目中放开 `unsafe`。边界要求：

- `mpv-engine` 的 exported frame 只以 `wgpu::Texture` 进入 `media::preview`；
  裸指针、`wgpu-hal` 句柄与 FFI 类型不得出现在项目代码中。
- GPU 纹理句柄只在 `media::preview` 与 UI 渲染适配层之间按帧传递，
  不得进入 Domain、编辑状态或持久化。
- 升级 `mpv-engine` / `wgpu` 时必须重新审查其 safe API 边界是否仍成立。
- 必须有不依赖具体硬件的测试，以及硬件路径与软件路径的一致性测试（见 §十七）。

### 理由

方案 F（ADR 0010）用 `mpv-engine` 取代了原 `media/platform` 自建 libav* 绑定。
原本需要项目内 `unsafe` 的外部纹理导入已由依赖完成，项目代码可以保持零 `unsafe`。
如果未来重新引入自建 FFI 或平台 surface 导入，必须恢复“唯一允许 unsafe 的 crate”规则，
并先记录 ADR。

### safe Rust 内的内存安全要求

不写 `unsafe` 不等于自动安全，以下仍然必须遵守：

- 禁止全局可变状态；跨线程共享使用所有权传递、`Arc` 或消息通道，不用约定保证的 `Send` / `Sync`。
- 用外部数据算出的索引不得无条件索引切片；使用 `get` / `get_mut` 并处理越界。
- 来自 FFprobe、文件头或用户输入的缓冲区长度必须校验，不信任外部声明的大小。
- 不做零拷贝布局假设，例如把字节缓冲区按结构体重新解释。
- 不用 `mem::forget` 规避析构，不用泄漏换取“简单”。

## 4. 命名转换

- `as_`：廉价借用转换。
- `to_`：可能分配或计算的转换。
- `into_`：消耗所有权的转换。
- getter 不加 `get_` 前缀：用 `duration()`，而不是 `get_duration()`。
- 测试函数不加 `test_` 前缀，`#[test]` 已经表明它是测试。

## 5. 公开 API

- 公开类型、函数、字段必须有 `///` 文档注释。
- 文档注释说明行为、约束和失败条件，不解释语言基础。

---

# 十五、注释与文档

## 1. 注释

- 注释独占一行，禁止行尾注释。
- 注释以句号结尾。
- 注释说明"为什么"，不复述代码"做了什么"。
- 引用类型、函数、关键字、路径时使用反引号。
- 过时注释等同于 bug，必须与代码同步修改。

## 2. 文档分工

- `AGENTS.md`：Agent 行为的唯一事实来源。
- `GOALS.md`：产品目标与非目标。
- `PLANS.md`：阶段划分与当前任务。
- `docs/architecture.md`：分层与数据流。
- `docs/media-model.md`：媒体数据模型。
- `docs/decisions/`：架构决策记录。

文档冲突时以 `AGENTS.md` 为准。

## 3. 写作要求

- 简洁，不解释基础概念，假设读者技术熟练。
- 不使用营销式语言。
- 专有名词保留原形：`stream copy`、`time base`、`keyframe`、`PTS`、`DTS`。

---

# 十六、依赖管理

添加 crate 之前必须考虑：

1. 标准库是否已经能够解决问题。
2. 项目现有依赖是否已经提供能力。
3. crate 是否仍在维护。
4. crate 的许可证。
5. 传递依赖数量。
6. 编译时间。
7. 二进制体积。
8. 是否引入新的 runtime。
9. 是否引入新的架构约束。

禁止因为：

> “这个功能好像有个 crate。”

就直接添加依赖。

新增依赖时，必须在变更说明中写清选择理由与被否决的替代方案。

---

# 十七、测试

修改媒体核心逻辑时必须优先添加测试。

测试至少包括：

### 单元测试

测试：

- 时间计算
- Segment
- 时间轴
- FFmpeg 参数生成
- FFprobe 解析
- stream selection
- progress parsing

单元测试不涉及文件系统、外部进程和图形界面。

### 集成测试

测试：

- FFprobe
- FFmpeg
- 导出
- 多片段
- stream copy
- 取消任务

集成测试不应依赖特定 FFmpeg 版本的日志措辞。需要区分版本行为时，在测试中显式检测版本并说明。

### 回归测试

遇到一个具体媒体文件导致 bug 时：

```text
Bug
 ↓
最小复现
 ↓
测试
 ↓
修复
 ↓
回归测试
```

不要只修复当前文件而不留下测试。

### UI 测试

不做像素级快照测试。

只测试纯逻辑：

- 时间线坐标转换
- 选择集运算
- 命令参数构造

### 无损必须被断言

导出相关的集成测试不能只断言"命令成功退出"。

至少用 FFprobe 对比输入与输出：

- 相同 codec
- 相同 time base
- 相同分辨率 / 采样率 / 声道数
- 时长在允许误差内

以此证明输出没有被重新编码。

---

# 十八、测试媒体

测试媒体应该：

- 尽可能小
- 可重复生成
- 具有明确用途
- 允许合法分发
- 尽可能稳定

测试素材应该覆盖典型情况：

- H.264 + AAC / MP4
- H.265 + AAC / MP4
- MKV
- MPEG-TS
- VFR
- 多音轨
- 字幕
- chapters
- 非零 start time

具体素材说明放在：

```text
tests/fixtures/
```

不要提交来源不明、体积过大或版权不清的媒体文件。

---

# 十九、性能

本项目追求：

- 快速启动
- 低空闲 CPU
- 低内存占用
- 快速媒体分析
- 流畅时间轴
- 流畅预览

不要为了“看起来高级”持续运行后台任务。

缩略图、波形、场景检测等昂贵操作应该：

- 按需执行
- 后台执行
- 可取消
- 可缓存

---

# 二十、性能问题处理

不要凭感觉优化。

正确流程：

```text
发现问题
 ↓
复现
 ↓
测量
 ↓
找到瓶颈
 ↓
最小修改
 ↓
再次测量
```

禁止：

> “这个地方可能比较慢，所以我先加缓存。”

---

# 二十一、架构修改

如果一个功能需要大范围修改架构：

必须先说明：

1. 当前架构为什么无法支持。
2. 哪个边界存在问题。
3. 有哪些解决方案。
4. 为什么选择当前方案。
5. 修改后会增加什么复杂度。

重大架构决定记录到：

```text
docs/decisions/
```

---

# 二十二、Git

不要自动：

- commit
- push
- reset
- rebase
- 修改历史

除非用户明确要求。

保持修改范围清晰。

不要把以下内容混在一个修改中：

- 功能
- 大规模重构
- 格式化
- 依赖升级
- 无关 bug 修复

---

# 二十三、红线清单

以下行为直接视为未完成：

- ❌ 引入 `Electron` / `Tauri` / `WebView` / JS UI 框架 / `Node.js` 运行时。
- ❌ 在 egui 帧回调中同步执行 FFmpeg、FFprobe、解码或大量文件 I/O。
- ❌ 用 `f32` / `f64` 秒作为媒体时间的唯一表示。
- ❌ 在项目代码中使用 `unsafe`、FFI、裸指针、`transmute`、`union`、`MaybeUninit`、`ManuallyDrop` 或 `static mut`。
- ❌ 让 `mpv-engine` / `wgpu` 的裸指针、FFI 类型或底层句柄越过 `media::preview` 与 UI 渲染适配层。
- ❌ 同时让 mpv 与 cpal 输出音频。
- ❌ 静默把 `stream copy` 降级为重新编码，或把重编码结果标记为无损。
- ❌ 通过 shell 拼接命令执行 FFmpeg。
- ❌ 用 `unwrap()` / `expect()` 处理用户输入或外部进程输出。
- ❌ 让 `serde_json::Value` 越过 FFprobe 解析边界继续传播。
- ❌ 在命令回调之外直接修改领域状态。
- ❌ 提交带 warning 或 `cargo fmt --check` 不通过的代码。
- ❌ 未经实际运行就声称"已经测试通过"。

---

# 二十四、完成标准

一个功能只有满足以下条件才算完成：

- [ ] 编译通过
- [ ] 相关测试通过
- [ ] 新功能有合理测试
- [ ] 错误路径经过考虑
- [ ] UI 不会被长任务阻塞
- [ ] 没有无必要的新依赖
- [ ] 新增依赖的理由已记录
- [ ] 公开 API 有 `///` 文档注释
- [ ] `cargo fmt --all --check` 通过
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` 通过
- [ ] 项目代码无 `unsafe`；外部 GPU 句柄只在 `media::preview` 与 UI 渲染适配层之间传递
- [ ] 相关文档已经更新
- [ ] 没有产生无关修改
- [ ] 功能、重构、格式化、依赖升级没有混在同一次修改中
- [ ] 实际验证了相关功能

如果没有实际测试：

**禁止声称“已经测试通过”。**

如果只是理论上 FFmpeg 支持：

**禁止声称“项目已经支持该格式”。**
