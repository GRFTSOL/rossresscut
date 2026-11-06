Rossresscut @VERSION@ - Windows x86_64 便携包
==========================================

构建：@DATE@（Windows 原生 MSVC 构建）。
许可证：GPL-3.0-or-later，见 LICENSE 与 ATTRIBUTION.md。

文件说明
--------

- rossresscut.exe   主程序
- libmpv-2.dll      预览与音频输出（mpv 官方 Windows 构建）
- ffmpeg.exe        探测 / 波形 / 场景检测 / 导出
- ffprobe.exe       媒体信息与关键帧探测
- LICENSE / ATTRIBUTION.md

运行
----

保持所有文件在同一目录。双击 rossresscut.exe，或在终端运行：

    rossresscut.exe <媒体文件>

ffmpeg.exe / ffprobe.exe 放在 exe 同目录时优先使用，否则从 PATH 查找。

配置文件：%APPDATA%\rossresscut\config.toml
项目文件扩展名：.rrc

日志与崩溃
----------

程序是 GUI 子系统，双击运行时没有控制台，日志不会显示。需要日志时从终端启动：

    cmd:         set RUST_LOG=info  然后  rossresscut.exe <媒体文件>
    PowerShell:  $env:RUST_LOG="info"; .\rossresscut.exe <媒体文件>

- 启动阶段失败会弹一个原生错误对话框，不会静默退出。
- 未捕获的 panic 会追加到 %LOCALAPPDATA%\rossresscut\crash.log，包含版本号、
  时间与堆栈；反馈问题时请附上该文件。

故障排查
--------

1. 双击后没有任何反应、也没有任何报错？

   wgpu 的 D3D12 后端会从 PATH 里加载 dxcompiler.dll。如果 PATH 里有
   ...\Windows Kits\10\bin\10.0.19041.0\x64（Windows SDK 的 bin 目录），
   其中的旧版 dxcompiler.dll 会让初始化失败，日志表现为：

       DxcCreateInstance failed: 不支持此接口 (0x80004002)
       Failed to create surface for any enabled backend: {}

   处理：把该 SDK bin 目录从用户 PATH 移除（只保留 MSVC 的
   Tools\MSVC\<ver>\bin\Hostx64\x64），然后重新登录或重启资源管理器。
   从终端用 RUST_LOG=info 启动可以直接看到这个错误。

2. 提示缺少 libmpv-2.dll？

   本包已包含；如果只拷贝了 exe，需要把 DLL 一起放过去。

3. 首次运行有 SmartScreen 提示（exe 未签名）：更多信息 -> 仍要运行。

已知限制
--------

- 多段导出的起点仍会回退到关键帧；智能裁切会重编码片段头尾。
- 图形字幕（PGS/DVD/DVB）不裁剪；只使用第一个文本字幕轨。
- 批处理暂不支持智能裁切。
- 预览硬件解码由 libmpv 的 hwdec=auto-safe 选择：Windows 上实测解析为
  nvdec（d3d11va 需要 gpu-context=d3d11，libmpv 的渲染路径下不可用）。

第三方组件
----------

- libmpv-2.dll：mpv，GPL-2.0-or-later。
- ffmpeg.exe / ffprobe.exe：FFmpeg 9.0 essentials build（GPLv3，gyan.dev 构建）。
- 本项目代码：GPL-3.0-or-later。
