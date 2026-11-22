# 0001 基础技术栈与工程结构

- 状态：已接受
- 日期：2026-10-24
- 阶段：Phase 0

## 背景

Phase 0 需要建立可构建、可测试、能启动最小窗口的工程骨架，同时为 UI / Application / Domain / Media 分层保留明确边界，但不提前实现后续阶段的能力。

## 决策

### 1. 单个 Cargo 包，同时提供库与二进制

`src/lib.rs` 导出分层模块，`src/main.rs` 只负责日志初始化、启动窗口和错误退出。集成测试通过库访问代码，不需要启动图形界面。

否决工作区多 crate 方案：当前没有独立发布或独立编译的需求，多 crate 只会增加路径、版本与依赖管理成本。

### 2. GUI 使用 `eframe`，通过 `eframe::egui` 使用 egui

不单独声明 `egui` 依赖，避免 egui 与 eframe 版本不一致带来的类型冲突。

使用 eframe 的默认特性（Wayland / X11、wgpu、默认字体、accessibility 等）。渲染器与预览后端是 Phase 2 的评估内容，Phase 0 不提前锁定。

### 3. 日志使用 `log` + `env_logger`

库代码只调用 `log` 宏，只有二进制入口安装 logger。默认过滤为 `warn,rossresscut=info`，避免转发 GUI 依赖的运行期日志；`RUST_LOG` 可以覆盖。

否决自建 logger：需要重新实现过滤语法、格式化与线程安全，收益不足。

### 4. 手写 `AppError`，不引入 `thiserror` / `anyhow`

Phase 0 只有 I/O 失败与启动失败两类错误。`AppError` 保存操作上下文，并通过 `source()` 保留底层错误链。

否决 `thiserror`：当前只有一个错误类型，派生宏带来的收益不足。否决 `anyhow`：它会丢失可匹配的错误类型，不适合作为领域层与媒体层的长期错误类型；后续错误类型显著增多时再单独评估 `thiserror`。

### 5. 固定工具链

`rust-toolchain.toml` 固定 `1.99.1`，并声明 `clippy` 与 `rustfmt`。升级工具链属于独立变更，不与功能改动混合。

### 6. 禁止 `unsafe`

`Cargo.toml` 设置 `[lints.rust] unsafe_code = "forbid"`，对包内所有编译目标生效。

## 后果

- Domain / Media / Application 可以在不创建窗口的情况下编译与测试。
- Phase 0 不为后续阶段预置占位类型或 trait，分层目前只体现为模块边界与文档。
- `eframe` 带来较大的依赖树，这是原生 GUI 的既定成本。若启动时间或二进制体积成为实测问题，再基于测量调整特性或渲染器。
- 当前准入门槛由 `scripts/check.sh` 提供；托管 CI 已添加 `.github/workflows/ci.yml`，尚未在 GitHub Actions 实际执行。
