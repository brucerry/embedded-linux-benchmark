#define _POSIX_C_SOURCE 200809L
#include <alloca.h>
#include <alsa/asoundlib.h>
#include <errno.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { rate = 48000, block = 256, marker_size = 511, marker_start = 64 };
static const int64_t ns_per_second = INT64_C(1000000000);

struct settings {
    const char *playback, *capture;
    unsigned duration_ms, timeout_ms, channels;
};

static void usage(void) {
    puts("Usage:\n"
         "  eld-engine-audio --playback hw:C,D --capture hw:C,D --loopback\n"
         "    [--channels 1|2] [--duration-ms 100..2000] [--timeout-ms 1..30000]\n"
         "  eld-engine-audio --self-test");
}

static int64_t monotonic_ns(void) {
    struct timespec timestamp;
    if (clock_gettime(CLOCK_MONOTONIC, &timestamp) != 0)
        return -1;
    return (int64_t)timestamp.tv_sec * ns_per_second + timestamp.tv_nsec;
}

static int alive(int64_t deadline) {
    int64_t now = monotonic_ns();
    return now >= 0 && now < deadline;
}

static int parse_number(const char *input, unsigned min, unsigned max, unsigned *out) {
    if (*input < '0' || *input > '9')
        return 0;
    char *end;
    errno = 0;
    unsigned long value = strtoul(input, &end, 10);
    if (errno || *end || value < min || value > max)
        return 0;
    *out = (unsigned)value;
    return 1;
}

static int valid_pcm(const char *name) {
    if (strncmp(name, "hw:", 3) != 0)
        return 0;
    const char *cursor = name + 3;
    for (unsigned part = 0; part < 3; ++part) {
        const char *start = cursor;
        while (*cursor >= '0' && *cursor <= '9')
            ++cursor;
        if (cursor == start || cursor - start > 3)
            return 0;
        if (*cursor == '\0')
            return part >= 1;
        if (*cursor != ',')
            return 0;
        ++cursor;
    }
    return 0;
}

static int parse(int argc, char **argv, struct settings *settings) {
    *settings = (struct settings){.duration_ms = 1000, .timeout_ms = 5000, .channels = 2};
    unsigned used = 0;
    const char *flags[] = {"--playback", "--capture", "--loopback",
                           "--duration-ms", "--timeout-ms", "--channels"};
    for (int i = 1; i < argc; ++i) {
        unsigned option = 0;
        while (option < 6 && strcmp(argv[i], flags[option]) != 0)
            ++option;
        if (option == 6 || ((used >> option) & 1U)) {
            fprintf(stderr, "Error: unknown or duplicate option %s\n", argv[i]);
            return 0;
        }
        used |= 1U << option;
        if (option == 2)
            continue;
        if (++i == argc) {
            fprintf(stderr, "Error: %s needs a value\n", flags[option]);
            return 0;
        }
        const char *value = argv[i];
        int valid = 0;
        switch (option) {
        case 0: settings->playback = value; valid = valid_pcm(value); break;
        case 1: settings->capture = value; valid = valid_pcm(value); break;
        case 3: valid = parse_number(value, 100, 2000, &settings->duration_ms); break;
        case 4: valid = parse_number(value, 1, 30000, &settings->timeout_ms); break;
        case 5: valid = parse_number(value, 1, 2, &settings->channels); break;
        }
        if (!valid) {
            fprintf(stderr, "Error: invalid value for %s\n", flags[option]);
            return 0;
        }
    }
    if ((used & 7U) != 7U) {
        fputs("Error: explicit playback, capture, and --loopback are required\n", stderr);
        return 0;
    }
    return 1;
}

struct pcm {
    snd_pcm_t *handle;
    snd_pcm_uframes_t period, buffer;
    int monotonic_timestamps;
    unsigned xruns;
};

static int alsa_error(struct pcm *pcm, const char *action, int error) {
    if (error == -EPIPE)
        ++pcm->xruns;
    fprintf(stderr, "Error: %s: %s\n", action, snd_strerror(error));
    return 0;
}

static int configure(struct pcm *pcm, const char *name, snd_pcm_stream_t direction,
                     unsigned channels) {
    const int capture = direction == SND_PCM_STREAM_CAPTURE;
    int status = snd_pcm_open(&pcm->handle, name, direction, SND_PCM_NONBLOCK);
    if (status < 0)
        return alsa_error(pcm, capture ? "open capture PCM" : "open playback PCM", status);

    snd_pcm_hw_params_t *hw;
    snd_pcm_sw_params_t *sw;
    snd_pcm_hw_params_alloca(&hw);
    snd_pcm_sw_params_alloca(&sw);
    pcm->period = block;
    pcm->buffer = 4 * block;
    if ((status = snd_pcm_hw_params_any(pcm->handle, hw)) < 0 ||
        (status = snd_pcm_hw_params_set_access(pcm->handle, hw, SND_PCM_ACCESS_RW_INTERLEAVED)) < 0 ||
        (status = snd_pcm_hw_params_set_format(pcm->handle, hw, SND_PCM_FORMAT_S16)) < 0 ||
        (status = snd_pcm_hw_params_set_channels(pcm->handle, hw, channels)) < 0 ||
        (status = snd_pcm_hw_params_set_rate(pcm->handle, hw, rate, 0)) < 0 ||
        (status = snd_pcm_hw_params_set_period_size_near(pcm->handle, hw, &pcm->period, NULL)) < 0 ||
        (status = snd_pcm_hw_params_set_buffer_size_near(pcm->handle, hw, &pcm->buffer)) < 0)
        return alsa_error(pcm, "choose fixed ALSA format and bounded buffers", status);
    if (pcm->period < 64 || pcm->period > 1024 || pcm->buffer < 2 * pcm->period ||
        pcm->buffer > 4096) {
        fputs("Error: PCM did not accept a bounded period and buffer\n", stderr);
        return 0;
    }
    if ((status = snd_pcm_hw_params(pcm->handle, hw)) < 0 ||
        (status = snd_pcm_get_params(pcm->handle, &pcm->buffer, &pcm->period)) < 0 ||
        (status = snd_pcm_sw_params_current(pcm->handle, sw)) < 0)
        return alsa_error(pcm, "apply per-stream ALSA parameters", status);
    if (pcm->period < 64 || pcm->period > 1024 || pcm->buffer < 2 * pcm->period ||
        pcm->buffer > 4096) {
        fputs("Error: PCM applied an out-of-range period or buffer\n", stderr);
        return 0;
    }
    if (capture) {
        pcm->monotonic_timestamps =
            snd_pcm_sw_params_set_tstamp_type(pcm->handle, sw, SND_PCM_TSTAMP_TYPE_MONOTONIC) == 0 &&
            snd_pcm_sw_params_set_tstamp_mode(pcm->handle, sw, SND_PCM_TSTAMP_ENABLE) == 0;
    } else {
        snd_pcm_uframes_t boundary;
        if ((status = snd_pcm_sw_params_get_boundary(sw, &boundary)) < 0 ||
            (status = snd_pcm_sw_params_set_start_threshold(pcm->handle, sw, boundary)) < 0)
            return alsa_error(pcm, "configure explicit PCM start", status);
    }
    if ((status = snd_pcm_sw_params(pcm->handle, sw)) < 0 ||
        (status = snd_pcm_prepare(pcm->handle)) < 0)
        return alsa_error(pcm, "prepare PCM", status);
    return 1;
}

static void marker(int16_t output[marker_size]) {
    uint32_t state = UINT32_C(0x12345678);
    for (unsigned i = 0; i < marker_size; ++i) {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        output[i] = state & 1U ? 2048 : -2048;
    }
}

struct match {
    unsigned offset;
    double correlation;
};

static int find_marker(const int16_t *recorded, unsigned frames, const int16_t *signal,
                       int64_t deadline, struct match *best) {
    int64_t source_sum = 0, source_energy = 0;
    *best = (struct match){0};
    for (unsigned j = 0; j < marker_size; ++j) {
        source_sum += signal[j];
        source_energy += (int64_t)signal[j] * signal[j];
    }
    const double source_variance =
        source_energy - (double)source_sum * source_sum / marker_size;
    for (unsigned i = 0; i + marker_size <= frames; ++i) {
        if ((i & 127U) == 0 && !alive(deadline))
            return 0;
        int64_t total = 0, energy = 0, product = 0;
        for (unsigned j = 0; j < marker_size; ++j) {
            int64_t value = recorded[i + j];
            total += value;
            energy += value * value;
            product += value * signal[j];
        }
        double variance = energy - (double)total * total / marker_size;
        if (variance < 32.0 * 32.0 * marker_size)
            continue;
        double coefficient = (product - (double)total * source_sum / marker_size) /
                             sqrt(source_variance * variance);
        if (fabs(coefficient) > fabs(best->correlation))
            *best = (struct match){i, coefficient};
    }
    return alive(deadline);
}

static unsigned minimum(unsigned a, unsigned b) {
    return a < b ? a : b;
}

static int record(struct pcm *play, struct pcm *cap, const struct settings *settings,
                  const int16_t *signal, int16_t *samples, int64_t *sample_times,
                  unsigned frames, int64_t deadline, int64_t *submitted) {
    int status = snd_pcm_start(cap->handle);
    if (status < 0)
        return alsa_error(cap, "start capture", status);

    unsigned sent = 0, received = 0;
    const unsigned playback_frames = frames + (unsigned)play->buffer;
    int started = 0;
    int16_t output[block * 2], input[block * 2];
    snd_pcm_status_t *capture_status;
    snd_pcm_status_alloca(&capture_status);
    while (received < frames) {
        if (!alive(deadline)) {
            fputs("Error: PCM transfer reached the time limit\n", stderr);
            return 0;
        }
        int changed = 0;
        if (sent < playback_frames) {
            snd_pcm_sframes_t ready = snd_pcm_avail_update(play->handle);
            if (ready < 0 && ready != -EAGAIN)
                return alsa_error(play, "check playback availability", (int)ready);
            if (ready > 0) {
                unsigned count = minimum(minimum((unsigned)ready, block), playback_frames - sent);
                for (unsigned j = 0; j < count; ++j) {
                    unsigned frame = sent + j;
                    int16_t data = frame >= marker_start && frame < marker_start + marker_size
                                       ? signal[frame - marker_start] : 0;
                    for (unsigned channel = 0; channel < settings->channels; ++channel)
                        output[j * settings->channels + channel] = data;
                }
                int64_t before = monotonic_ns();
                snd_pcm_sframes_t written = snd_pcm_writei(play->handle, output, count);
                if (written < 0 && written != -EAGAIN && written != -EINTR)
                    return alsa_error(play, "write playback frames", (int)written);
                if (written > 0) {
                    if (sent <= marker_start && sent + (unsigned)written > marker_start)
                        *submitted = before;
                    sent += (unsigned)written;
                    changed = 1;
                }
                if (!started && sent >= play->period) {
                    status = snd_pcm_start(play->handle);
                    if (status < 0)
                        return alsa_error(play, "start playback", status);
                    started = 1;
                }
            }
        }

        status = snd_pcm_status(cap->handle, capture_status);
        if (status < 0)
            return alsa_error(cap, "check capture status", status);
        if (snd_pcm_status_get_state(capture_status) == SND_PCM_STATE_XRUN)
            return alsa_error(cap, "capture overrun", -EPIPE);
        snd_pcm_uframes_t ready = snd_pcm_status_get_avail(capture_status);
        if (ready > 0 && ready <= cap->buffer) {
            snd_htimestamp_t stamp;
            snd_pcm_status_get_htstamp(capture_status, &stamp);
            int64_t first = -1;
            if (cap->monotonic_timestamps && stamp.tv_sec > 0 &&
                stamp.tv_nsec >= 0 && stamp.tv_nsec < ns_per_second)
                first = (int64_t)stamp.tv_sec * ns_per_second + stamp.tv_nsec -
                        (int64_t)ready * ns_per_second / rate;
            unsigned count = minimum(minimum((unsigned)ready, block), frames - received);
            snd_pcm_sframes_t got = snd_pcm_readi(cap->handle, input, count);
            if (got < 0 && got != -EAGAIN && got != -EINTR)
                return alsa_error(cap, "read capture frames", (int)got);
            if (got > 0) {
                for (unsigned j = 0; j < (unsigned)got; ++j) {
                    samples[received + j] = input[j * settings->channels];
                    sample_times[received + j] =
                        first >= 0 ? first + (int64_t)j * ns_per_second / rate : -1;
                }
                received += (unsigned)got;
                changed = 1;
            }
        } else if (ready > cap->buffer) {
            return alsa_error(cap, "capture overrun", -EPIPE);
        }
        if (!changed) {
            struct timespec pause = {.tv_nsec = 500000};
            nanosleep(&pause, NULL);
        }
    }
    if (!started || *submitted < 0) {
        fputs("Error: capture finished before the playback marker was submitted\n", stderr);
        return 0;
    }
    return 1;
}

static int exercise(const struct settings *settings) {
    int result = 1;
    struct pcm play = {0}, cap = {0};
    unsigned frames = settings->duration_ms * rate / 1000;
    int16_t *samples = calloc(frames, sizeof(*samples));
    int64_t *times = calloc(frames, sizeof(*times));
    int64_t submitted = -1, start = monotonic_ns();
    int64_t deadline = start + (int64_t)settings->timeout_ms * 1000000;
    int16_t signal[marker_size];
    marker(signal);
    if (!samples || !times || start < 0) {
        fputs("Error: cannot allocate bounded PCM buffers or read timer\n", stderr);
        goto out;
    }
    if (!configure(&play, settings->playback, SND_PCM_STREAM_PLAYBACK, settings->channels) ||
        !configure(&cap, settings->capture, SND_PCM_STREAM_CAPTURE, settings->channels))
        goto out;

    if (!record(&play, &cap, settings, signal, samples, times, frames, deadline, &submitted))
        goto out;
    // Stop both streams before CPU correlation so analysis time cannot induce xruns.
    snd_pcm_drop(play.handle);
    snd_pcm_drop(cap.handle);
    struct match match;
    if (!find_marker(samples, frames, signal, deadline, &match)) {
        fputs("Error: PCM correlation reached the time limit\n", stderr);
        goto out;
    }
    if (fabs(match.correlation) < 0.70) {
        fputs("Error: loopback marker was not verified in capture channel 1\n", stderr);
        goto out;
    }
    printf("ALSA PCM loopback (%d Hz, %u channel(s), signed 16-bit)\n", rate, settings->channels);
    printf("Frames captured: %u\nMarker correlation on channel 1: %+.4f\n", frames, match.correlation);
    printf("Playback period / buffer: %lu / %lu frames\n", play.period, play.buffer);
    printf("Capture period / buffer: %lu / %lu frames\n", cap.period, cap.buffer);
    if (times[match.offset] >= submitted) {
        printf("Estimated software round trip: %.3f ms\n",
               (times[match.offset] - submitted) / 1000000.0);
    } else {
        puts("Estimated software round trip: unavailable from capture timestamps");
    }
    result = 0;
out:
    printf("Playback underruns: %u\nCapture overruns: %u\n", play.xruns, cap.xruns);
    if (cap.handle)
        snd_pcm_close(cap.handle);
    if (play.handle)
        snd_pcm_close(play.handle);
    free(times);
    free(samples);
    return result;
}

static int self_test(void) {
    int16_t signal[marker_size], capture[2048] = {0};
    marker(signal);
    const unsigned offset = 637;
    for (unsigned i = 0; i < marker_size; ++i)
        capture[offset + i] = (int16_t)(120 - signal[i] / 2);
    struct match match;
    int64_t deadline = monotonic_ns() + ns_per_second;
    if (!find_marker(capture, 2048, signal, deadline, &match) ||
        match.offset != offset || match.correlation > -0.999)
        return 1;
    memset(capture, 0, sizeof(capture));
    if (!find_marker(capture, 2048, signal, deadline, &match) ||
        match.correlation != 0 || find_marker(capture, 2048, signal, 0, &match))
        return 1;
    puts("Synthetic marker offset, phase inversion, silence and timeout passed.");
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 1 || (argc == 2 && strcmp(argv[1], "--help") == 0)) {
        usage();
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "--self-test") == 0)
        return self_test();
    struct settings settings;
    return parse(argc, argv, &settings) ? exercise(&settings) : 1;
}
