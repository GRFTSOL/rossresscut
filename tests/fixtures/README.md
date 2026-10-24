# 测试素材

这里的所有媒体文件都是使用 FFmpeg `lavfi` 源生成的合成素材，不包含第三方内容。

重新生成：

```bash
./tests/fixtures/generate.sh
```

| 文件 | 用途 | 容器 / codec |
| --- | --- | --- |
| `sample.mp4` | 基础 MP4 解析、音频预览 | MP4 / H.264 + AAC |
| `sample-video-only.mp4` | 视频预览与播放控制 | MP4 / H.264 |
| `sample.mkv` | 多音轨、字幕、章节 | Matroska / H.264 + 2x AAC + SubRip |
| `sample.wmv` | ASF/WMV 解析 | ASF / WMV2 + WMA2 |
| `sample.avi` | AVI 解析 | AVI / MPEG-4 Part 2 + PCM |
| `sample.ts` | 非零 start time | MPEG-TS / H.264 + AAC |

`subtitles.srt` 与 `chapters.txt` 是生成 `sample.mkv` 的输入。`not-media.bin` 用于验证 FFprobe 失败路径。

测试只断言项目模型中的稳定字段（容器名、codec、流数量、时长范围、章节、start time），不依赖 FFprobe 的日志措辞或版本特定输出。
