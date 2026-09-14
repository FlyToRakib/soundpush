// SoundPush Virtual Audio: ring buffer implementation. See cable.h for the design.

#include "cable.h"

// The frame counters are read without a lock; that is only atomic on 64-bit targets.
C_ASSERT(sizeof(PVOID) == 8);

namespace
{

// Decodes one little-endian integer sample into a left-justified 32-bit sample.
// Only unsigned shifts are used, so no implementation-defined signed shifting occurs.
inline INT32 LoadSample(_In_reads_bytes_(Bytes) const UCHAR* Source, _In_ ULONG Bytes)
{
    UINT32 value;
    switch (Bytes)
    {
    case 2:
        value = ((UINT32)Source[0] << 16) | ((UINT32)Source[1] << 24);
        break;
    case 3:
        value = ((UINT32)Source[0] << 8) | ((UINT32)Source[1] << 16) | ((UINT32)Source[2] << 24);
        break;
    default:  // 4
        value = (UINT32)Source[0] | ((UINT32)Source[1] << 8) | ((UINT32)Source[2] << 16) |
                ((UINT32)Source[3] << 24);
        break;
    }
    return (INT32)value;
}

// Encodes a left-justified 32-bit sample as a little-endian integer of Bytes bytes,
// keeping the most significant bits.
inline void StoreSample(_Out_writes_bytes_(Bytes) UCHAR* Destination, _In_ INT32 Sample, _In_ ULONG Bytes)
{
    const UINT32 value = (UINT32)Sample;
    switch (Bytes)
    {
    case 2:
        Destination[0] = (UCHAR)(value >> 16);
        Destination[1] = (UCHAR)(value >> 24);
        break;
    case 3:
        Destination[0] = (UCHAR)(value >> 8);
        Destination[1] = (UCHAR)(value >> 16);
        Destination[2] = (UCHAR)(value >> 24);
        break;
    default:  // 4
        Destination[0] = (UCHAR)value;
        Destination[1] = (UCHAR)(value >> 8);
        Destination[2] = (UCHAR)(value >> 16);
        Destination[3] = (UCHAR)(value >> 24);
        break;
    }
}

}  // namespace

#pragma code_seg("PAGE")

_Use_decl_annotations_
NTSTATUS CCable::Initialize()
{
    PAGED_CODE();

    if (m_Samples != nullptr)
    {
        return STATUS_SUCCESS;
    }
    // Non-paged: the ring is used from timer callbacks at DISPATCH_LEVEL.
    m_Samples = (INT32*)ExAllocatePool2(POOL_FLAG_NON_PAGED,
                                        (SIZE_T)kCapacityFrames * 2 * sizeof(INT32),
                                        SP_POOL_TAG);
    if (m_Samples == nullptr)
    {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    m_WriteFrame = 0;
    m_ReadFrame = 0;
    m_ReaderActive = 0;
    m_Primed = FALSE;
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
void CCable::Cleanup()
{
    PAGED_CODE();

    if (m_Samples != nullptr)
    {
        ExFreePoolWithTag(m_Samples, SP_POOL_TAG);
        m_Samples = nullptr;
    }
}

#pragma code_seg()

_Use_decl_annotations_
void CCable::StartReader()
{
    // Start from "now": whatever was queued before anyone recorded is stale.
    const LONG64 write = InterlockedExchangeAdd64(&m_WriteFrame, 0);
    InterlockedExchange64(&m_ReadFrame, write);
    m_Primed = FALSE;
    InterlockedExchange(&m_ReaderActive, 1);
}

_Use_decl_annotations_
void CCable::StopReader()
{
    InterlockedExchange(&m_ReaderActive, 0);
}

_Use_decl_annotations_
void CCable::Write(const UCHAR* Source, ULONG Frames, const SP_PCM_FORMAT& Format)
{
    if (m_Samples == nullptr || Source == nullptr || Frames == 0 || !SpIsValidFormat(Format))
    {
        return;
    }
    if (InterlockedCompareExchange(&m_ReaderActive, 0, 0) == 0)
    {
        return;  // nobody is recording
    }

    const LONG64 write = m_WriteFrame;  // owned by this side
    const LONG64 read = InterlockedExchangeAdd64(&m_ReadFrame, 0);

    // The reader only moves forward, so this may overestimate the backlog, never underestimate it.
    const ULONG64 queued = (ULONG64)(write - read);
    const ULONG64 space = queued >= kCapacityFrames ? 0 : kCapacityFrames - queued;
    ULONG frames = Frames;
    if (frames > space)
    {
        m_DroppedFrames += frames - space;
        frames = (ULONG)space;
    }

    const ULONG bytes = Format.BytesPerSample;
    for (ULONG i = 0; i < frames; ++i)
    {
        const UCHAR* frame = Source + (SIZE_T)i * Format.BlockAlign;
        const INT32 left = LoadSample(frame, bytes);
        const INT32 right = Format.Channels == 2 ? LoadSample(frame + bytes, bytes) : left;
        const SIZE_T slot = (SIZE_T)(((ULONG64)write + i) & kMask) * 2;
        m_Samples[slot] = left;
        m_Samples[slot + 1] = right;
    }

    // Publish the new frames only after they are fully written.
    InterlockedExchange64(&m_WriteFrame, write + frames);
}

_Use_decl_annotations_
void CCable::Read(UCHAR* Destination, ULONG Frames, const SP_PCM_FORMAT& Format)
{
    if (Destination == nullptr || Frames == 0 || !SpIsValidFormat(Format))
    {
        return;
    }
    const SIZE_T totalBytes = (SIZE_T)Frames * Format.BlockAlign;
    if (m_Samples == nullptr)
    {
        RtlZeroMemory(Destination, totalBytes);
        return;
    }

    LONG64 read = m_ReadFrame;  // owned by this side
    const LONG64 write = InterlockedExchangeAdd64(&m_WriteFrame, 0);
    ULONG64 queued = (ULONG64)(write - read);

    if (queued > kMaxQueuedFrames)
    {
        // Too far behind the feed: drop stale audio instead of adding latency.
        m_SkippedFrames += queued - kPrimeFrames;
        read = write - kPrimeFrames;
        queued = kPrimeFrames;
    }

    if (!m_Primed)
    {
        if (queued < kPrimeFrames)
        {
            // Silence until enough audio is queued to play smoothly.
            RtlZeroMemory(Destination, totalBytes);
            InterlockedExchange64(&m_ReadFrame, read);
            return;
        }
        m_Primed = TRUE;
    }

    const ULONG available = queued < Frames ? (ULONG)queued : Frames;
    const ULONG bytes = Format.BytesPerSample;
    for (ULONG i = 0; i < available; ++i)
    {
        const SIZE_T slot = (SIZE_T)(((ULONG64)read + i) & kMask) * 2;
        const INT32 left = m_Samples[slot];
        const INT32 right = m_Samples[slot + 1];
        UCHAR* frame = Destination + (SIZE_T)i * Format.BlockAlign;
        if (Format.Channels == 2)
        {
            StoreSample(frame, left, bytes);
            StoreSample(frame + bytes, right, bytes);
        }
        else
        {
            StoreSample(frame, (INT32)(((INT64)left + (INT64)right) / 2), bytes);
        }
    }

    if (available < Frames)
    {
        // The feed stopped or fell behind: silence for the rest, then prime again.
        RtlZeroMemory(Destination + (SIZE_T)available * Format.BlockAlign,
                      (SIZE_T)(Frames - available) * Format.BlockAlign);
        ++m_Underruns;
        m_Primed = FALSE;
    }

    InterlockedExchange64(&m_ReadFrame, read + available);
}
