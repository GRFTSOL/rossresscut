#!/usr/bin/env bash
# Regenerates the tiny synthetic media fixtures used by integration tests.
set -euo pipefail

cd "$(dirname "$0")/../.."

if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "ffmpeg is required to regenerate test fixtures" >&2
    exit 1
fi

ffmpeg -y -hide_banner -loglevel error \
    -f lavfi -i testsrc2=size=160x120:rate=10:duration=1 \
    -f lavfi -i sine=frequency=440:sample_rate=44100:duration=1 \
    -map 0:v:0 -map 1:a:0 \
    -c:v libx264 -preset ultrafast -crf 35 -pix_fmt yuv420p \
    -c:a aac -b:a 32k -ac 1 \
    -shortest -movflags +faststart \
    tests/fixtures/sample.mp4

ffmpeg -y -hide_banner -loglevel error \
    -f lavfi -i testsrc2=size=160x120:rate=10:duration=2 \
    -c:v libx264 -preset ultrafast -crf 35 -pix_fmt yuv420p \
    -an \
    tests/fixtures/sample-video-only.mp4

ffmpeg -y -hide_banner -loglevel error \
    -f lavfi -i testsrc2=size=160x120:rate=10:duration=2 \
    -f lavfi -i sine=frequency=440:sample_rate=44100:duration=2 \
    -f lavfi -i sine=frequency=880:sample_rate=44100:duration=2 \
    -i tests/fixtures/subtitles.srt \
    -i tests/fixtures/chapters.txt \
    -map 0:v:0 -map 1:a:0 -map 2:a:0 -map 3:s:0 \
    -map_metadata 4 -map_chapters 4 \
    -c:v libx264 -preset ultrafast -crf 35 -pix_fmt yuv420p \
    -c:a aac -b:a 32k -ac 1 \
    -c:s srt \
    -metadata:s:a:0 language=eng \
    -metadata:s:a:1 language=jpn \
    -metadata:s:s:0 language=eng \
    -shortest \
    tests/fixtures/sample.mkv

ffmpeg -y -hide_banner -loglevel error \
    -f lavfi -i testsrc2=size=160x120:rate=10:duration=1 \
    -f lavfi -i sine=frequency=440:sample_rate=44100:duration=1 \
    -map 0:v:0 -map 1:a:0 \
    -c:v wmv2 -b:v 200k -pix_fmt yuv420p \
    -c:a wmav2 -b:a 32k -ac 1 \
    -shortest \
    tests/fixtures/sample.wmv

ffmpeg -y -hide_banner -loglevel error \
    -f lavfi -i testsrc2=size=160x120:rate=10:duration=1 \
    -f lavfi -i sine=frequency=440:sample_rate=44100:duration=1 \
    -map 0:v:0 -map 1:a:0 \
    -c:v mpeg4 -q:v 10 -pix_fmt yuv420p \
    -c:a pcm_s16le -ar 22050 -ac 1 \
    -shortest \
    tests/fixtures/sample.avi

ffmpeg -y -hide_banner -loglevel error \
    -f lavfi -i testsrc2=size=160x120:rate=10:duration=1 \
    -f lavfi -i sine=frequency=440:sample_rate=44100:duration=1 \
    -map 0:v:0 -map 1:a:0 \
    -c:v libx264 -preset ultrafast -crf 35 -pix_fmt yuv420p \
    -c:a aac -b:a 32k -ac 1 \
    -shortest -f mpegts \
    tests/fixtures/sample.ts

echo "fixtures regenerated"
