# 编码规范

本文件是错误处理、Rust 风格、依赖、测试、性能与架构修改的约束来源。入口与硬约束见 `AGENTS.md`。

## 错误处理

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

### 1. unwrap 的例外

`.unwrap()` / `.expect()` 仅在不变式保证 `Some` / `Ok` 时允许使用，并且必须在上一行
独占一行写明原因：

```rust
// INVARIANT: 调用方已校验 segments 非空。
let first = segments.first().unwrap();
```

不允许用 `expect()` 代替有意义的错误信息。

### 2. 错误必须携带上下文

错误信息至少包含：

- 失败的操作
- 相关文件路径
- 相关参数（stream index、时间戳等）

不允许吞掉 FFmpeg 的 stderr 后只返回"ffmpeg failed"。

### 3. 禁止吞错

不允许用以下方式静默丢失错误：

- `let _ = result;`
- 空 `match` 分支
- 只记录日志但返回 `Ok` 的 `Err` 分支

除非在同一位置说明为什么可以忽略。

---

## Rust 编码规范

### 1. 风格

- 4 空格缩进。
- 模块、函数、变量：`snake_case`。
- 类型、trait、enum：`CamelCase`。
- 常量、静态量：`SCREAMING_SNAKE_CASE`。
- 格式以 `cargo fmt` 为准，不手工对齐。

### 2. 表达

- 优先迭代器组合：`map` / `filter` / `collect`，而不是手写 `for` 加 `push`。
- 当循环更清楚时（多状态、提前退出、副作用顺序敏感）使用循环，不为了风格牺牲可读性。
- 避免不必要的克隆、分配和中间集合。
- 抽象必须由真实复用驱动，不为"看起来更通用"引入 trait 或泛型。

### 3. 内存安全：项目代码禁止 unsafe

项目内 crate 必须保持 `unsafe_code = "forbid"`，禁止 `unsafe`、FFI、裸指针、
`transmute`、`union`、`MaybeUninit`、`ManuallyDrop` 与 `static mut`。

预览使用外部 crate `mpv-engine`（封装 libmpv）与 `wgpu`，它们的 `unsafe` 由依赖内部承担，
不得因此在本项目中放开 `unsafe`。边界要求：

- `mpv-engine` 的 exported frame 只以 `wgpu::Texture` 进入 `media::preview`；
  裸指针、`wgpu-hal` 句柄与 FFI 类型不得出现在项目代码中。
- GPU 纹理句柄只在 `media::preview` 与 UI 渲染适配层之间按帧传递，
  不得进入 Domain、编辑状态或持久化。
- 升级 `mpv-engine` / `wgpu` 时必须重新审查其 safe API 边界是否仍成立。
- 必须有不依赖具体硬件的测试，以及硬件路径与软件路径的一致性测试（见「测试」一节）。

#### 理由

方案 F（ADR 0010）用 `mpv-engine` 取代了原 `media/platform` 自建 libav* 绑定。
原本需要项目内 `unsafe` 的外部纹理导入已由依赖完成，项目代码可以保持零 `unsafe`。
如果未来重新引入自建 FFI 或平台 surface 导入，必须恢复“唯一允许 unsafe 的 crate”规则，
并先记录 ADR。

#### safe Rust 内的内存安全要求

不写 `unsafe` 不等于自动安全，以下仍然必须遵守：

- 禁止全局可变状态；跨线程共享使用所有权传递、`Arc` 或消息通道，不用约定保证的 `Send` / `Sync`。
- 用外部数据算出的索引不得无条件索引切片；使用 `get` / `get_mut` 并处理越界。
- 来自 FFprobe、文件头或用户输入的缓冲区长度必须校验，不信任外部声明的大小。
- 不做零拷贝布局假设，例如把字节缓冲区按结构体重新解释。
- 不用 `mem::forget` 规避析构，不用泄漏换取“简单”。

### 4. 命名转换

- `as_`：廉价借用转换。
- `to_`：可能分配或计算的转换。
- `into_`：消耗所有权的转换。
- getter 不加 `get_` 前缀：用 `duration()`，而不是 `get_duration()`。
- 测试函数不加 `test_` 前缀，`#[test]` 已经表明它是测试。

### 5. 公开 API

- 公开类型、函数、字段必须有 `///` 文档注释。
- 文档注释说明行为、约束和失败条件，不解释语言基础。

---

## 注释与文档

### 1. 注释

- 注释独占一行，禁止行尾注释。
- 注释以句号结尾。
- 注释说明"为什么"，不复述代码"做了什么"。
- 引用类型、函数、关键字、路径时使用反引号。
- 过时注释等同于 bug，必须与代码同步修改。

### 2. 文档分工

- `AGENTS.md`：入口与不可违背的硬约束。
- `GOALS.md`：产品目标与非目标。
- `PLANS.md`：阶段划分与当前任务。
- `docs/history/completed-phases.md`：已完成阶段的任务、验证记录与已知限制。
- `CONTRIBUTING.md`：贡献流程。
- `RELEASING.md`：发布流程。
- `docs/architecture.md`：分层与数据流。
- `docs/media-model.md`：媒体数据模型。
- `docs/ui-guidelines.md`：UI 与桌面集成规则。
- `docs/media-rules.md`：媒体处理、时间、Segment、导出规则。
- `docs/coding-standards.md`：错误处理、Rust 风格、依赖、测试、性能。
- `docs/development.md`：命令、环境、权限边界、Git 与完成标准。
- `docs/research.md`：外部参考项目研究边界。
- `docs/open-questions.md`：未解决问题。
- `docs/decisions/`：架构决策记录。
- `docs/investigations/`：调查记录。

具体规则以对应文档为准；与 `AGENTS.md` 的硬约束冲突时，以 `AGENTS.md` 为准。

文档有两种语言版本：上表列出的是英文主文件，中文原文保存在同目录的 `<name>_ZH.md`
（`README.md` 对应 `README_ZH.md`）。修改文档时必须同时更新两种语言，并保持标题、代码块、
表格与列表结构一一对应；代码、路径、命令、标识符与专有名词不翻译。

### 3. 写作要求

- 简洁，不解释基础概念，假设读者技术熟练。
- 不使用营销式语言。
- 专有名词保留原形：`stream copy`、`time base`、`keyframe`、`PTS`、`DTS`。

---

## 依赖管理

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

## 测试

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

## 测试媒体

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

## 性能

本项目追求：

- 快速启动
- 低空闲 CPU
- 低内存占用
- 快速媒体分析
- 流畅时间轴
- 流畅预览

不要为了“看起来高级”持续运行后台任务。

波形、场景检测等昂贵操作应该：

- 按需执行
- 后台执行
- 可取消
- 可缓存

---

## 性能问题处理

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

## 架构修改

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
