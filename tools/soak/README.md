# soundpush-soak

Soak and latency harness for the SoundPush engine (plan §29.1). Two engines run in one process and
stream over real QUIC (or TLS over TCP, the USB path) on loopback. The sender captures a generated
20 ms tone burst once per second; the receiver detects each burst when it is rendered.

Each interval it reports:

- end-to-end pipeline latency (p50, p95, max) measured per burst: capture, encode, network, jitter
  buffer, decode and render (audio device buffers are not included),
- bursts sent and missed (a missed burst is a dropout while it played),
- the engine's own route statistics: underruns, loss, jitter, buffer, clock drift,
- resident memory (Linux; watch the process in Task Manager or Activity Monitor elsewhere).

It exits with status 1 when a budget is exceeded and 2 when the run could not start.

```
cargo run --release -p soundpush-soak -- [options]

  --minutes N                 run time (default 10)
  --interval SECONDS          report interval (default 10)
  --latency low|balanced|stable
  --quality auto|opus|lossless
  --transport quic|tcp        tcp = the USB (adb reverse) transport
  --csv FILE                  also write every interval as CSV
  --max-latency-ms MS         fail if the overall p95 latency is higher
  --max-missed-per-hour N     fail if more bursts per hour are missed
```

Examples:

```
# Short CI-style check
cargo run --release -p soundpush-soak -- --minutes 2 --max-latency-ms 150 --max-missed-per-hour 60

# 24-hour soak with CSV for plotting memory and drift
cargo run --release -p soundpush-soak -- --minutes 1440 --interval 60 --csv soak.csv
```

This exercises the engine's pipeline on one machine. The release soak on real devices (phone and
computer, Wi-Fi, battery) remains a manual test (implementation status, Phase 5).
