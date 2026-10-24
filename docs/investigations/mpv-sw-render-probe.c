/*
 * Phase 4B probe: measure libmpv software-render throughput.
 *
 * Not part of the product build. It exists so the ADR numbers can be
 * reproduced. Build and run from the repository root:
 *
 *   gcc -O2 docs/investigations/mpv-sw-render-probe.c \
 *       -o /tmp/mpv-sw-render-probe -lmpv
 *   /tmp/mpv-sw-render-probe <file> <hwdec> <seconds> [start] [target_frames]
 *
 * It loads the file, seeks, and renders every new frame into a 960x540 RGB0
 * buffer. The reported fps is the frame rate the embedding application could
 * actually consume, including any GPU->CPU readback inside libmpv.
 */
#include <mpv/client.h>
#include <mpv/render.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static double now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec / 1e9;
}

int main(int argc, char **argv) {
    if (argc < 4) {
        fprintf(stderr, "usage: %s <file> <hwdec> <seconds> [start] [target_frames]\n", argv[0]);
        return 2;
    }
    const char *file = argv[1];
    const char *hwdec = argv[2];
    double seconds = atof(argv[3]);
    double start = argc > 4 ? atof(argv[4]) : 60.0;
    int target_frames = argc > 5 ? atoi(argv[5]) : 0;

    mpv_handle *mpv = mpv_create();
    if (!mpv) {
        fprintf(stderr, "mpv_create failed\n");
        return 1;
    }
    mpv_set_option_string(mpv, "vo", "libmpv");
    mpv_set_option_string(mpv, "hwdec", hwdec);
    mpv_set_option_string(mpv, "ao", "null");
    mpv_set_option_string(mpv, "aid", "no");
    mpv_set_option_string(mpv, "untimed", "yes");
    mpv_set_option_string(mpv, "config", "no");
    mpv_set_option_string(mpv, "terminal", "no");
    if (mpv_initialize(mpv) < 0) {
        fprintf(stderr, "mpv_initialize failed\n");
        return 1;
    }

    mpv_render_context *ctx = NULL;
    mpv_render_param params[] = {
        {MPV_RENDER_PARAM_API_TYPE, (void *)MPV_RENDER_API_TYPE_SW},
        {MPV_RENDER_PARAM_INVALID, NULL},
    };
    if (mpv_render_context_create(&ctx, mpv, params) < 0) {
        fprintf(stderr, "mpv_render_context_create failed\n");
        return 1;
    }

    int size[2] = {960, 540};
    size_t stride = 960 * 4;
    char *format = "rgb0";
    void *buffer = NULL;
    if (posix_memalign(&buffer, 64, stride * 540) != 0) {
        fprintf(stderr, "allocation failed\n");
        return 1;
    }
    mpv_render_param rparams[] = {
        {MPV_RENDER_PARAM_SW_SIZE, size},
        {MPV_RENDER_PARAM_SW_FORMAT, format},
        {MPV_RENDER_PARAM_SW_STRIDE, &stride},
        {MPV_RENDER_PARAM_SW_POINTER, buffer},
        {MPV_RENDER_PARAM_INVALID, NULL},
    };

    const char *load[] = {"loadfile", file, NULL};
    if (mpv_command(mpv, load) < 0) {
        fprintf(stderr, "loadfile failed\n");
        return 1;
    }

    int loaded = 0;
    double wait_begin = now();
    while (!loaded && now() - wait_begin < 30.0) {
        mpv_event *event = mpv_wait_event(mpv, 0.1);
        if (event->event_id == MPV_EVENT_FILE_LOADED) {
            loaded = 1;
        } else if (event->event_id == MPV_EVENT_END_FILE) {
            fprintf(stderr, "file ended before playback\n");
            return 1;
        }
    }
    if (!loaded) {
        fprintf(stderr, "timed out loading file\n");
        return 1;
    }

    char start_text[32];
    snprintf(start_text, sizeof start_text, "%.3f", start);
    const char *seek[] = {"seek", start_text, "absolute", NULL};
    if (mpv_command(mpv, seek) < 0) {
        fprintf(stderr, "seek failed\n");
        return 1;
    }

    double began = now();
    long frames = 0;
    double render_total = 0.0;
    double render_max = 0.0;
    while (now() - began < seconds) {
        mpv_event *event = mpv_wait_event(mpv, 0);
        if (event->event_id == MPV_EVENT_END_FILE) {
            break;
        }
        if (mpv_render_context_update(ctx) & MPV_RENDER_UPDATE_FRAME) {
            double render_begin = now();
            mpv_render_context_render(ctx, rparams);
            double render_took = now() - render_begin;
            render_total += render_took;
            if (render_took > render_max) {
                render_max = render_took;
            }
            frames++;
            if (target_frames > 0 && frames >= target_frames) {
                break;
            }
        } else {
            usleep(1000);
        }
    }
    double elapsed = now() - began;
    printf(
        "frames=%ld elapsed=%.3f fps=%.2f render_avg_ms=%.2f render_max_ms=%.2f hwdec=%s\n",
        frames,
        elapsed,
        elapsed > 0.0 ? (double)frames / elapsed : 0.0,
        frames > 0 ? render_total * 1000.0 / (double)frames : 0.0,
        render_max * 1000.0,
        hwdec
    );

    mpv_render_context_free(ctx);
    mpv_terminate_destroy(mpv);
    free(buffer);
    return 0;
}
