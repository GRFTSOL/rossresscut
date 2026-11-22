# ADR-0015：项目许可证选择 GPL-3.0-or-later

## 状态

Accepted

## 背景

项目准备发布 Linux 二进制包，但一直没有确定自身许可证。预览主路径动态链接
系统的 `libmpv`，因此不能单方面选择宽松许可证而不处理 GPL 合规问题。

## 已知约束

- Arch 的 `mpv` 包标注 `GPL-2.0-or-later AND LGPL-2.1-or-later`；发布二进制时
  按 `GPL-2.0-or-later` 处理最稳妥。
- `cpal 0.18.2` 是单一 `Apache-2.0`，用于外部 FFmpeg 回退路径的音频输出。
- 其余直接依赖为 `MIT` 或 `MIT OR Apache-2.0`；内嵌字体为 `OFL-1.1`。
- 项目代码没有复制 LosslessCut / FilmCraft / EffectCraft 的源码。
- 项目仅面向本地桌面使用，当前不发布到 AUR 或公开仓库。

## 候选方案

### 方案 A：GPL-3.0-or-later

优点：

- 与 `libmpv` 的 `GPL-2.0-or-later` 兼容（允许升级到 GPLv3）。
- 与 `cpal` 的 `Apache-2.0` 兼容（Apache-2.0 与 GPLv2 存在兼容性争议，与 GPLv3 兼容）。
- 与所有 MIT / 双许可依赖兼容。
- 包含专利授权条款。

缺点：

- 分发二进制时必须提供对应源码。
- 不能闭源分发。
- 与 `GPL-2.0-only` 的代码不兼容（当前没有这类依赖）。

### 方案 B：GPL-2.0-or-later

优点：

- 许可证文本更短。
- 同样可以链接 `libmpv`。

缺点：

- `cpal` 的 `Apache-2.0` 与 GPLv2 的兼容性有争议，实际分发需要按 GPLv3 条款使用，
  声明会变得含糊。

### 方案 C：MIT / Apache-2.0

优点：

- 最宽松，允许闭源再分发。

缺点：

- 动态链接 GPL 的 `libmpv` 并分发二进制时存在合规风险。
- 只有改用 LGPL 构建的 `libmpv` 或不分发二进制时才比较安全。

## 决策

选择：

> 方案 A：GPL-3.0-or-later。

## 决策理由

1. 一条许可证同时覆盖 `libmpv`、`cpal` 与全部 MIT 依赖，不需要额外例外。
2. `libmpv` 的 `or-later` 允许升级到 GPLv3，合规路径最简单。
3. 项目定位是开源桌面工具，GPL 的开源义务与目标一致。
4. 比 AGPL 温和，不引入与本地桌面使用无关的网络条款。

## 后果

### 正面

- 依赖许可证组合清晰，发布时不需要逐项解释兼容性。
- 项目保持完全开源。

### 负面

- 分发二进制必须同时提供对应源码或书面提供方式。
- 不能闭源分发。

### 后续影响

- `LICENSE` 使用 GPLv3 全文。
- `Cargo.toml`、`packaging/PKGBUILD`、`README.md`、`ATTRIBUTION.md`、
  `RELEASING.md` 与 `PLANS.md` 的许可证说明已同步。
- pacman 包同时安装 `LICENSE` 与 `ATTRIBUTION.md`。

## 未解决问题

- 无。
