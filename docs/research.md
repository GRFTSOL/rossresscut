# 参考实现研究

本文件记录外部参考项目的使用边界与研究顺序。LosslessCut 是行为参考，不是代码模板。

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

## FilmCraft / EffectCraft

FilmCraft 与 EffectCraft 可以作为 Rust + egui 工程实践的参考，尤其是：

- engine-first、UI toolkit 位于最上层；
- media / render / gpu 与 UI 分离；
- 大型媒体项目的模块边界与测试方式。

使用边界与 LosslessCut 相同：

- 只参考架构取舍与行为，不复制代码。
- 不把其依赖、平台约束或产品范围直接带进本项目。
- 任何影响 Domain / Application / Media 架构的结论，走 `architecture-decision`。
