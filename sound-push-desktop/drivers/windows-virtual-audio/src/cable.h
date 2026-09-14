// SoundPush Virtual Audio: the "cable", a lock-free ring buffer that joins the render
// endpoint (writer) to the capture endpoint (reader).
//
// Samples are stored as stereo, left-justified 32-bit integers, whatever format each
// side opened with, so the two endpoints may use different bit depths or channel counts.
//
// Concurrency: exactly one writer (the render stream) and one reader (the capture
// stream); each endpoint allows a single stream at a time and serializes its own calls
// with its stream lock. Writer and reader may run at the same time on different CPUs.
// The write and read positions are 64-bit frame counters that only grow; each side owns
// one counter and reads the other atomically, so no lock is shared between the two.
//
// Latency: the reader keeps about kPrimeFrames queued to absorb timer jitter. After an
// underrun it outputs silence until that much is queued again, and when more than
// kMaxQueuedFrames pile up (for example after a stall) it skips the stale audio.

#pragma once

#include "common.h"

class CCable
{
public:
    // Ring capacity in frames. Must be a power of two (~341 ms at 48 kHz).
    static constexpr ULONG kCapacityFrames = 16384;
    // Frames the reader keeps queued before it starts reading (30 ms).
    static constexpr ULONG kPrimeFrames = 1440;
    // Beyond this backlog (100 ms) the reader drops stale audio down to kPrimeFrames.
    static constexpr ULONG kMaxQueuedFrames = 4800;

    // Allocates the ring. Call once before use.
    _IRQL_requires_(PASSIVE_LEVEL)
    NTSTATUS Initialize();

    // Frees the ring. No stream may be using the cable any more.
    _IRQL_requires_(PASSIVE_LEVEL)
    void Cleanup();

    // Writer: appends Frames frames of Format-encoded audio. Audio is discarded while no
    // reader is active (nobody is recording) or when the ring is full.
    _IRQL_requires_max_(DISPATCH_LEVEL)
    void Write(_In_reads_bytes_((SIZE_T)Frames * Format.BlockAlign) const UCHAR* Source,
               _In_ ULONG Frames,
               _In_ const SP_PCM_FORMAT& Format);

    // Reader: fills exactly Frames frames. Missing audio is written as silence.
    _IRQL_requires_max_(DISPATCH_LEVEL)
    void Read(_Out_writes_bytes_((SIZE_T)Frames * Format.BlockAlign) UCHAR* Destination,
              _In_ ULONG Frames,
              _In_ const SP_PCM_FORMAT& Format);

    // Reader lifetime: the capture stream calls StartReader before it starts running and
    // StopReader once it no longer calls Read. StartReader discards anything queued.
    _IRQL_requires_max_(DISPATCH_LEVEL)
    void StartReader();
    _IRQL_requires_max_(DISPATCH_LEVEL)
    void StopReader();

private:
    static constexpr ULONG kMask = kCapacityFrames - 1;

    // kCapacityFrames * 2 samples: left, right, left, right...
    INT32* m_Samples = nullptr;

    // Total frames ever written (owned by the writer) and read (owned by the reader).
    // m_ReadFrame <= m_WriteFrame <= m_ReadFrame + kCapacityFrames always holds.
    LONG64 m_WriteFrame = 0;
    LONG64 m_ReadFrame = 0;

    // 1 while a capture stream is running.
    LONG m_ReaderActive = 0;

    // Reader-owned: whether kPrimeFrames were queued since the last underrun.
    BOOLEAN m_Primed = FALSE;

    // Diagnostics (each owned by one side; read only by a debugger for now).
    ULONG64 m_DroppedFrames = 0;   // writer: frames lost because the ring was full
    ULONG64 m_Underruns = 0;       // reader: reads that ran out of audio
    ULONG64 m_SkippedFrames = 0;   // reader: stale frames dropped to bound latency
};
