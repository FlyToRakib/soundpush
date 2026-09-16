# latency-probe — end-to-end pipeline latency

Measures how long audio takes to travel through the SoundPush media pipeline — capture, DSP, Opus,
redundancy, the network, the jitter buffer, concealment, drift compensation and render — by pushing
a chirp through it and cross-correlating what comes out with what went in (plan §29.1 "Audio
quality"). It needs no audio hardware and no network, so it runs anywhere, including CI.

    latency-probe [options]

Options:

    --latency low|balanced|stable   Latency profile (default: balanced)
    --quality auto|opus|lossless    Codec (default: auto). --quality opus:64000 fixes the bitrate.
    --channels 1|2                  Wire channels (default: 2)
    --redundancy                    Send a redundant copy of each frame
    --link <profile>                Impairment profile from `netsim list` (default: perfect)
    --seconds <n>                   How long to measure (default: 12; one reading per second)
    --seed <n>                      Seed for the impairment RNG (default: 1)
    --max-ms <n>                    Exit 1 when the p95 reading exceeds this
    --json                          Print one JSON object instead of a table

Examples:

    latency-probe
    latency-probe --latency low --quality lossless
    latency-probe --link congested --seconds 30
    latency-probe --max-ms 120        # a budget check for CI

## What it measures, and what it does not

A 20 ms chirp is captured once a second, silence in between (so DTX runs, as it would with real
audio). The receiver's rendered audio is recorded, and one period at a time is cross-correlated
with the chirp to recover the delay. Both fake devices tick at 10 ms, like real ones, and both
timestamp their first buffer, so the reading is the delay between a sample entering capture and the
same sample leaving render.

The reading therefore includes the 10 ms device buffer at each end and the jitter buffer, and
excludes the hardware's own output latency (headphones, Bluetooth), which is a property of the
device rather than of the pipeline. A real end-to-end number is the reading plus that.

`tools/soak` measures the same path over hours with a burst detector; this tool is the short,
precise version for a single configuration.
