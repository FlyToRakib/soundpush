// SoundPush Virtual Audio: WaveRT miniports and streams.
//
// With WaveRT, the Windows audio engine reads and writes a cyclic buffer that the
// miniport allocates, and asks the miniport where the "hardware" currently is in that
// buffer. There is no hardware here, so each stream emulates it:
//
//   - The stream clock is the performance counter. While running, the position advances
//     by exactly 48000 frames per second.
//   - A high-resolution timer (and every position query) moves the frames between the
//     previous and the current position: a render stream copies them from the cyclic
//     buffer into the cable, a capture stream copies them from the cable into the buffer.
//   - For event-driven clients the timer fires at each notification boundary and signals
//     the registered events; otherwise it fires every 10 ms.
//
// Render and capture streams use the same clock, so what the feed receives is what
// the microphone delivers, a constant ~30 ms later (see cable.h).
//
// Supported formats: 48 kHz, mono or stereo, 16-bit, 24-bit or 32-bit integer PCM
// (32-bit with 24 or 32 valid bits). The audio engine converts from its float mix format.

#include "common.h"
#include "cable.h"

namespace
{

// ------------------------------------------------------------------ constants

// 100-nanosecond units per second (the unit of ExSetTimer due times).
constexpr LONGLONG kTicksPerSecond = 10000000;
// Timer period when no client waits for notifications (10 ms).
constexpr LONGLONG kPollTicks = 100000;
// Bounds on the notification timer (1 ms .. 10 ms), and a 0.5 ms lateness so the
// position has crossed the boundary when the timer fires.
constexpr LONGLONG kMinTimerTicks = 10000;
constexpr LONGLONG kTimerSlackTicks = 5000;
// Cyclic buffer size bounds: 10 ms .. 2 s of audio.
constexpr ULONG kMinBufferFrames = SP_SAMPLE_RATE / 100;
constexpr ULONG kMaxBufferFrames = SP_SAMPLE_RATE * 2;
// Notification events a stream can hold (the audio engine registers one).
constexpr ULONG kMaxNotificationEvents = 4;
// Notifications per buffer allowed by the WaveRT contract.
constexpr ULONG kMaxNotificationsPerBuffer = 2;

// ------------------------------------------------------------------ data ranges

#define SP_PCM_RANGE(bits)                                                   \
    {                                                                        \
        {sizeof(KSDATARANGE_AUDIO), 0, 0, 0,                                 \
         STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),                              \
         STATICGUIDOF(KSDATAFORMAT_SUBTYPE_PCM),                             \
         STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)},                 \
        SP_MAX_CHANNELS, (bits), (bits), SP_SAMPLE_RATE, SP_SAMPLE_RATE      \
    }

KSDATARANGE_AUDIO kPcmRanges[] = {
    SP_PCM_RANGE(16),
    SP_PCM_RANGE(24),
    SP_PCM_RANGE(32),
};

PKSDATARANGE kStreamRanges[] = {
    &kPcmRanges[0].DataRange,
    &kPcmRanges[1].DataRange,
    &kPcmRanges[2].DataRange,
};

KSDATARANGE kWaveBridgeRange = {
    {sizeof(KSDATARANGE), 0, 0, 0,
     STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),
     STATICGUIDOF(KSDATAFORMAT_SUBTYPE_ANALOG),
     STATICGUIDOF(KSDATAFORMAT_SPECIFIER_NONE)}};

PKSDATARANGE kWaveBridgeRanges[] = {&kWaveBridgeRange};

// ------------------------------------------------------------------ filter descriptors

// One stream per endpoint (global and per filter). The audio engine opens a single shared
// stream and mixes all apps into it; this also guarantees one writer and one reader for the cable.
const PCPIN_DESCRIPTOR kRenderPins[] = {
    // SP_WAVE_PIN_STREAM
    {1, 1, 0, nullptr,
     {0, nullptr, 0, nullptr, SIZEOF_ARRAY(kStreamRanges), kStreamRanges,
      KSPIN_DATAFLOW_IN, KSPIN_COMMUNICATION_SINK, &KSCATEGORY_AUDIO, nullptr, 0}},
    // SP_WAVE_PIN_BRIDGE
    {0, 0, 0, nullptr,
     {0, nullptr, 0, nullptr, SIZEOF_ARRAY(kWaveBridgeRanges), kWaveBridgeRanges,
      KSPIN_DATAFLOW_OUT, KSPIN_COMMUNICATION_NONE, &KSCATEGORY_AUDIO, nullptr, 0}},
};

const PCCONNECTION_DESCRIPTOR kRenderConnections[] = {
    {PCFILTER_NODE, SP_WAVE_PIN_STREAM, PCFILTER_NODE, SP_WAVE_PIN_BRIDGE},
};

const PCPIN_DESCRIPTOR kCapturePins[] = {
    // SP_WAVE_PIN_STREAM
    {1, 1, 0, nullptr,
     {0, nullptr, 0, nullptr, SIZEOF_ARRAY(kStreamRanges), kStreamRanges,
      KSPIN_DATAFLOW_OUT, KSPIN_COMMUNICATION_SINK, &KSCATEGORY_AUDIO, nullptr, 0}},
    // SP_WAVE_PIN_BRIDGE
    {0, 0, 0, nullptr,
     {0, nullptr, 0, nullptr, SIZEOF_ARRAY(kWaveBridgeRanges), kWaveBridgeRanges,
      KSPIN_DATAFLOW_IN, KSPIN_COMMUNICATION_NONE, &KSCATEGORY_AUDIO, nullptr, 0}},
};

const PCCONNECTION_DESCRIPTOR kCaptureConnections[] = {
    {PCFILTER_NODE, SP_WAVE_PIN_BRIDGE, PCFILTER_NODE, SP_WAVE_PIN_STREAM},
};

// Categories come from the INF interface registration (KSCATEGORY_AUDIO, RENDER or
// CAPTURE, and REALTIME).
const PCFILTER_DESCRIPTOR kRenderFilter = {
    0, nullptr,
    sizeof(PCPIN_DESCRIPTOR), SIZEOF_ARRAY(kRenderPins), kRenderPins,
    sizeof(PCNODE_DESCRIPTOR), 0, nullptr,
    SIZEOF_ARRAY(kRenderConnections), kRenderConnections,
    0, nullptr,
};

const PCFILTER_DESCRIPTOR kCaptureFilter = {
    0, nullptr,
    sizeof(PCPIN_DESCRIPTOR), SIZEOF_ARRAY(kCapturePins), kCapturePins,
    sizeof(PCNODE_DESCRIPTOR), 0, nullptr,
    SIZEOF_ARRAY(kCaptureConnections), kCaptureConnections,
    0, nullptr,
};

// ------------------------------------------------------------------ format validation

#pragma code_seg("PAGE")

// Validates a KS data format from a client and reduces it to SP_PCM_FORMAT.
// Returns STATUS_NO_MATCH for anything the driver does not support.
_IRQL_requires_(PASSIVE_LEVEL)
NTSTATUS SpParseFormat(_In_ PKSDATAFORMAT DataFormat, _Out_ SP_PCM_FORMAT* Format)
{
    PAGED_CODE();

    RtlZeroMemory(Format, sizeof(*Format));

    if (DataFormat == nullptr || DataFormat->FormatSize < sizeof(KSDATAFORMAT_WAVEFORMATEX))
    {
        return STATUS_NO_MATCH;
    }
    if (!IsEqualGUID(DataFormat->MajorFormat, KSDATAFORMAT_TYPE_AUDIO) ||
        !IsEqualGUID(DataFormat->SubFormat, KSDATAFORMAT_SUBTYPE_PCM) ||
        !IsEqualGUID(DataFormat->Specifier, KSDATAFORMAT_SPECIFIER_WAVEFORMATEX))
    {
        return STATUS_NO_MATCH;
    }

    // WAVEFORMATEX is byte-packed, so its GUIDs are compared with IsEqualGUID (memcmp),
    // never the aligned variant.
    const WAVEFORMATEX* wave = &static_cast<PKSDATAFORMAT_WAVEFORMATEX>(static_cast<PVOID>(DataFormat))->WaveFormatEx;
    ULONG validBits = wave->wBitsPerSample;

    if (wave->wFormatTag == WAVE_FORMAT_EXTENSIBLE)
    {
        if (DataFormat->FormatSize < sizeof(KSDATAFORMAT) + sizeof(WAVEFORMATEXTENSIBLE) ||
            wave->cbSize < sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX))
        {
            return STATUS_NO_MATCH;
        }
        const WAVEFORMATEXTENSIBLE* extensible = CONTAINING_RECORD(wave, WAVEFORMATEXTENSIBLE, Format);
        if (!IsEqualGUID(extensible->SubFormat, KSDATAFORMAT_SUBTYPE_PCM))
        {
            return STATUS_NO_MATCH;
        }
        validBits = extensible->Samples.wValidBitsPerSample;
    }
    else if (wave->wFormatTag != WAVE_FORMAT_PCM)
    {
        return STATUS_NO_MATCH;
    }

    if (wave->nSamplesPerSec != SP_SAMPLE_RATE || wave->nChannels < 1 || wave->nChannels > SP_MAX_CHANNELS)
    {
        return STATUS_NO_MATCH;
    }

    const ULONG bits = wave->wBitsPerSample;
    const bool supported = (bits == 16 && validBits == 16) ||
                           (bits == 24 && validBits == 24) ||
                           (bits == 32 && (validBits == 24 || validBits == 32));
    if (!supported)
    {
        return STATUS_NO_MATCH;
    }

    const ULONG blockAlign = wave->nChannels * (bits / 8);
    if (wave->nBlockAlign != blockAlign || wave->nAvgBytesPerSec != blockAlign * SP_SAMPLE_RATE)
    {
        return STATUS_NO_MATCH;
    }

    Format->Channels = wave->nChannels;
    Format->BytesPerSample = bits / 8;
    Format->BlockAlign = blockAlign;
    return STATUS_SUCCESS;
}

#pragma code_seg()

// Frames elapsed in Ticks performance-counter ticks, without overflow for any
// realistic uptime (Frequency * SP_SAMPLE_RATE stays far below 2^63).
inline ULONG64 FramesFromTicks(_In_ LONGLONG Ticks, _In_ LONGLONG Frequency)
{
    if (Ticks <= 0 || Frequency <= 0)
    {
        return 0;
    }
    const ULONG64 ticks = (ULONG64)Ticks;
    const ULONG64 frequency = (ULONG64)Frequency;
    return (ticks / frequency) * SP_SAMPLE_RATE + ((ticks % frequency) * SP_SAMPLE_RATE) / frequency;
}

class CMiniportWaveRT;

// ------------------------------------------------------------------ stream

class CMiniportWaveRTStream final : public IMiniportWaveRTStreamNotification
{
public:
    CMiniportWaveRTStream(_In_ CMiniportWaveRT* Miniport,
                          _In_ ISoundPushAdapter* Adapter,
                          _In_ PPORTWAVERTSTREAM PortStream,
                          _In_ const SP_PCM_FORMAT& Format,
                          _In_ SpDirection Direction);
    CMiniportWaveRTStream(const CMiniportWaveRTStream&) = delete;
    CMiniportWaveRTStream& operator=(const CMiniportWaveRTStream&) = delete;

    _IRQL_requires_(PASSIVE_LEVEL)
    NTSTATUS Initialize();

    // IUnknown
    STDMETHODIMP QueryInterface(_In_ REFIID Iid, _COM_Outptr_ PVOID* Object) override;
    STDMETHODIMP_(ULONG) AddRef() override
    {
        return (ULONG)InterlockedIncrement(&m_Refs);
    }
    STDMETHODIMP_(ULONG) Release() override
    {
        const LONG refs = InterlockedDecrement(&m_Refs);
        NT_ASSERT(refs >= 0);
        if (refs == 0)
        {
            delete this;
        }
        return (ULONG)refs;
    }

    IMP_IMiniportWaveRTStream;
    IMP_IMiniportWaveRTStreamNotification;

private:
    // Only Release may destroy the object; runs at PASSIVE_LEVEL.
    ~CMiniportWaveRTStream();

    static EXT_CALLBACK TimerCallback;

    _IRQL_requires_(PASSIVE_LEVEL)
    NTSTATUS AllocateBuffer(_In_ ULONG NotificationCount,
                            _In_ ULONG RequestedSize,
                            _Out_ PMDL* AudioBufferMdl,
                            _Out_ ULONG* ActualSize,
                            _Out_ ULONG* OffsetFromFirstPage,
                            _Out_ MEMORY_CACHING_TYPE* CacheType);

    _IRQL_requires_(PASSIVE_LEVEL)
    void FreeBuffer(_In_opt_ PMDL AudioBufferMdl);

    // Advances the emulated hardware to Now and moves the audio in between.
    _Requires_lock_held_(m_Lock)
    _IRQL_requires_(DISPATCH_LEVEL)
    void UpdateLocked(_In_ LONGLONG Now);

    // Schedules the next timer callback.
    _Requires_lock_held_(m_Lock)
    _IRQL_requires_(DISPATCH_LEVEL)
    void ArmTimerLocked();

    LONG m_Refs = 0;

    // Owners (each holds a reference).
    CMiniportWaveRT* m_Miniport;
    ISoundPushAdapter* m_Adapter;
    PPORTWAVERTSTREAM m_PortStream;
    CCable* m_Cable;

    const SP_PCM_FORMAT m_Format;
    const SpDirection m_Direction;
    LONGLONG m_QpcFrequency = 0;
    PEX_TIMER m_Timer = nullptr;
    // Whether this capture stream has told the cable it is reading (PASSIVE_LEVEL only).
    bool m_ReaderStarted = false;

    // Everything below is guarded by m_Lock: the timer callback runs concurrently with the
    // port's calls.
    KSPIN_LOCK m_Lock = 0;

    PMDL m_BufferMdl = nullptr;
    PUCHAR m_Buffer = nullptr;       // system-space mapping of m_BufferMdl
    ULONG m_BufferBytes = 0;
    ULONG m_BufferFrames = 0;
    ULONG m_NotificationCount = 0;   // 0 = polling client
    PKEVENT m_Events[kMaxNotificationEvents] = {};

    KSSTATE m_State = KSSTATE_STOP;
    bool m_Running = false;
    LONGLONG m_RunStartQpc = 0;      // performance counter when RUN was entered
    ULONG64 m_FramesAtRunStart = 0;  // m_Frames when RUN was entered
    ULONG64 m_Frames = 0;            // frames moved since STOP; position = m_Frames mod buffer
};

// ------------------------------------------------------------------ miniport

class CMiniportWaveRT final : public IMiniportWaveRT
{
public:
    explicit CMiniportWaveRT(SpDirection Direction) : m_Direction(Direction) {}
    CMiniportWaveRT(const CMiniportWaveRT&) = delete;
    CMiniportWaveRT& operator=(const CMiniportWaveRT&) = delete;

    STDMETHODIMP QueryInterface(_In_ REFIID Iid, _COM_Outptr_ PVOID* Object) override
    {
        if (Object == nullptr)
        {
            return STATUS_INVALID_PARAMETER;
        }
        if (IsEqualGUID(Iid, IID_IUnknown) || IsEqualGUID(Iid, IID_IMiniport) ||
            IsEqualGUID(Iid, IID_IMiniportWaveRT))
        {
            *Object = static_cast<IMiniportWaveRT*>(this);
            AddRef();
            return STATUS_SUCCESS;
        }
        *Object = nullptr;
        return STATUS_INVALID_PARAMETER;
    }

    STDMETHODIMP_(ULONG) AddRef() override
    {
        return (ULONG)InterlockedIncrement(&m_Refs);
    }

    STDMETHODIMP_(ULONG) Release() override
    {
        const LONG refs = InterlockedDecrement(&m_Refs);
        NT_ASSERT(refs >= 0);
        if (refs == 0)
        {
            delete this;
        }
        return (ULONG)refs;
    }

    IMP_IMiniportWaveRT;

    // Called by the stream's destructor.
    void StreamClosed()
    {
        InterlockedExchange(&m_StreamOpen, 0);
    }

private:
    ~CMiniportWaveRT()
    {
        if (m_Adapter != nullptr)
        {
            m_Adapter->Release();
        }
    }

    LONG m_Refs = 0;
    const SpDirection m_Direction;
    ISoundPushAdapter* m_Adapter = nullptr;
    LONG m_StreamOpen = 0;
};

// ------------------------------------------------------------------ miniport methods

#pragma code_seg("PAGE")

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::Init(PUNKNOWN UnknownAdapter, PRESOURCELIST ResourceList, PPORTWAVERT Port)
{
    PAGED_CODE();
    UNREFERENCED_PARAMETER(ResourceList);
    // The port is not kept: it owns this miniport, and a reference back would never be released.
    UNREFERENCED_PARAMETER(Port);

    if (UnknownAdapter == nullptr || m_Adapter != nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }
    PVOID adapter = nullptr;
    NTSTATUS status = UnknownAdapter->QueryInterface(__uuidof(ISoundPushAdapter), &adapter);
    if (!NT_SUCCESS(status) || adapter == nullptr)
    {
        return NT_SUCCESS(status) ? STATUS_INVALID_PARAMETER : status;
    }
    m_Adapter = static_cast<ISoundPushAdapter*>(adapter);
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::GetDescription(PPCFILTER_DESCRIPTOR* Description)
{
    PAGED_CODE();
    if (Description == nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }
    // PortCls never writes through the descriptor; the cast only satisfies the interface type.
    *Description = const_cast<PPCFILTER_DESCRIPTOR>(
        m_Direction == SpDirection::Render ? &kRenderFilter : &kCaptureFilter);
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::DataRangeIntersection(ULONG PinId,
                                                               PKSDATARANGE DataRange,
                                                               PKSDATARANGE MatchingDataRange,
                                                               ULONG OutputBufferLength,
                                                               PVOID ResultantFormat,
                                                               PULONG ResultantFormatLength)
{
    PAGED_CODE();
    UNREFERENCED_PARAMETER(PinId);
    UNREFERENCED_PARAMETER(DataRange);
    UNREFERENCED_PARAMETER(MatchingDataRange);
    UNREFERENCED_PARAMETER(OutputBufferLength);
    UNREFERENCED_PARAMETER(ResultantFormat);
    if (ResultantFormatLength != nullptr)
    {
        *ResultantFormatLength = 0;
    }
    // PortCls's default intersection handles fixed-rate PCM ranges; NewStream re-validates.
    return STATUS_NOT_IMPLEMENTED;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::GetDeviceDescription(PDEVICE_DESCRIPTION DeviceDescription)
{
    PAGED_CODE();
    if (DeviceDescription == nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }
    // No DMA hardware. PortCls asks anyway; describe a bus master that can address all memory.
    RtlZeroMemory(DeviceDescription, sizeof(DEVICE_DESCRIPTION));
    DeviceDescription->Version = DEVICE_DESCRIPTION_VERSION;
    DeviceDescription->Master = TRUE;
    DeviceDescription->ScatterGather = TRUE;
    DeviceDescription->Dma64BitAddresses = TRUE;
    DeviceDescription->InterfaceType = PNPBus;
    DeviceDescription->MaximumLength = 0xFFFFFFFF;
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRT::NewStream(PMINIPORTWAVERTSTREAM* Stream,
                                                   PPORTWAVERTSTREAM PortStream,
                                                   ULONG Pin,
                                                   BOOLEAN Capture,
                                                   PKSDATAFORMAT DataFormat)
{
    PAGED_CODE();

    if (Stream == nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }
    *Stream = nullptr;

    if (PortStream == nullptr || m_Adapter == nullptr || Pin != SP_WAVE_PIN_STREAM ||
        (Capture != FALSE) != (m_Direction == SpDirection::Capture))
    {
        return STATUS_INVALID_PARAMETER;
    }

    SP_PCM_FORMAT format;
    NTSTATUS status = SpParseFormat(DataFormat, &format);
    if (!NT_SUCCESS(status))
    {
        return status;
    }

    // The pin descriptor already limits instances to one; this also keeps the cable single-writer
    // and single-reader if that limit were ever raised.
    if (InterlockedCompareExchange(&m_StreamOpen, 1, 0) != 0)
    {
        return STATUS_DEVICE_BUSY;
    }

    auto stream = new (SpPool::NonPaged) CMiniportWaveRTStream(this, m_Adapter, PortStream, format, m_Direction);
    if (stream == nullptr)
    {
        StreamClosed();
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    stream->AddRef();

    status = stream->Initialize();
    if (!NT_SUCCESS(status))
    {
        stream->Release();  // the destructor reopens the endpoint
        return status;
    }

    *Stream = stream;  // the port takes over this reference
    return STATUS_SUCCESS;
}

#pragma code_seg()

// ------------------------------------------------------------------ stream methods

_Use_decl_annotations_
CMiniportWaveRTStream::CMiniportWaveRTStream(CMiniportWaveRT* Miniport,
                                             ISoundPushAdapter* Adapter,
                                             PPORTWAVERTSTREAM PortStream,
                                             const SP_PCM_FORMAT& Format,
                                             SpDirection Direction)
    : m_Miniport(Miniport),
      m_Adapter(Adapter),
      m_PortStream(PortStream),
      m_Cable(Adapter->Cable()),
      m_Format(Format),
      m_Direction(Direction)
{
    m_Miniport->AddRef();
    m_Adapter->AddRef();
    m_PortStream->AddRef();
    KeInitializeSpinLock(&m_Lock);
}

CMiniportWaveRTStream::~CMiniportWaveRTStream()
{
    if (m_Timer != nullptr)
    {
        // Cancels the timer and waits for a running callback to finish.
        ExDeleteTimer(m_Timer, TRUE, TRUE, nullptr);
        m_Timer = nullptr;
    }
    if (m_ReaderStarted)
    {
        m_Cable->StopReader();
        m_ReaderStarted = false;
    }
    FreeBuffer(m_BufferMdl);  // normally already freed by the port

    m_PortStream->Release();
    m_Adapter->Release();
    m_Miniport->StreamClosed();
    m_Miniport->Release();
}

#pragma code_seg("PAGE")

_Use_decl_annotations_
NTSTATUS CMiniportWaveRTStream::Initialize()
{
    PAGED_CODE();

    KeQueryPerformanceCounter((PLARGE_INTEGER)static_cast<PVOID>(&m_QpcFrequency));
    if (m_QpcFrequency <= 0 || m_Cable == nullptr)
    {
        return STATUS_UNSUCCESSFUL;
    }

    // High resolution: ~1 ms accuracy for event-driven clients. The timer only runs while
    // the stream is in RUN, so it does not raise the system timer rate otherwise.
    m_Timer = ExAllocateTimer(TimerCallback, this, EX_TIMER_HIGH_RESOLUTION);
    return m_Timer != nullptr ? STATUS_SUCCESS : STATUS_INSUFFICIENT_RESOURCES;
}

#pragma code_seg()

_Use_decl_annotations_
STDMETHODIMP CMiniportWaveRTStream::QueryInterface(REFIID Iid, PVOID* Object)
{
    if (Object == nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }
    if (IsEqualGUID(Iid, IID_IUnknown) || IsEqualGUID(Iid, IID_IMiniportWaveRTStream) ||
        IsEqualGUID(Iid, IID_IMiniportWaveRTStreamNotification))
    {
        *Object = static_cast<IMiniportWaveRTStreamNotification*>(this);
        AddRef();
        return STATUS_SUCCESS;
    }
    *Object = nullptr;
    return STATUS_INVALID_PARAMETER;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::SetFormat(PKSDATAFORMAT DataFormat)
{
    UNREFERENCED_PARAMETER(DataFormat);
    // The format is fixed for the life of a stream; the audio engine opens a new one instead.
    return STATUS_NOT_SUPPORTED;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::SetState(KSSTATE State)
{
    if (State != KSSTATE_STOP && State != KSSTATE_ACQUIRE && State != KSSTATE_PAUSE && State != KSSTATE_RUN)
    {
        return STATUS_INVALID_PARAMETER;
    }

    const bool capture = m_Direction == SpDirection::Capture;

    // A capture stream becomes the cable's reader before its timer can run.
    if (State == KSSTATE_RUN && capture && !m_ReaderStarted)
    {
        m_Cable->StartReader();
        m_ReaderStarted = true;
    }

    bool cancelTimer = false;
    KIRQL oldIrql;
    KeAcquireSpinLock(&m_Lock, &oldIrql);
    const LONGLONG now = KeQueryPerformanceCounter(nullptr).QuadPart;

    switch (State)
    {
    case KSSTATE_RUN:
        if (!m_Running)
        {
            m_RunStartQpc = now;
            m_FramesAtRunStart = m_Frames;
            m_Running = true;
            ArmTimerLocked();
        }
        break;

    case KSSTATE_PAUSE:
    case KSSTATE_ACQUIRE:
        if (m_Running)
        {
            UpdateLocked(now);  // account for audio up to this moment, then freeze the position
            m_Running = false;
            cancelTimer = true;
        }
        break;

    default:  // KSSTATE_STOP: WaveRT resets the position to zero
        m_Running = false;
        m_Frames = 0;
        m_FramesAtRunStart = 0;
        cancelTimer = true;
        break;
    }
    m_State = State;
    KeReleaseSpinLock(&m_Lock, oldIrql);

    // A callback that is already running sees m_Running == false and does not re-arm.
    if (cancelTimer)
    {
        ExCancelTimer(m_Timer, nullptr);
    }
    if (State != KSSTATE_RUN && capture && m_ReaderStarted)
    {
        m_Cable->StopReader();
        m_ReaderStarted = false;
    }
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetPosition(PKSAUDIO_POSITION Position)
{
    if (Position == nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }

    KIRQL oldIrql;
    KeAcquireSpinLock(&m_Lock, &oldIrql);
    UpdateLocked(KeQueryPerformanceCounter(nullptr).QuadPart);
    const ULONGLONG offset =
        m_BufferFrames != 0 ? (m_Frames % m_BufferFrames) * m_Format.BlockAlign : 0;
    KeReleaseSpinLock(&m_Lock, oldIrql);

    // The emulated hardware consumes (render) or produces (capture) exactly at this offset.
    Position->PlayOffset = offset;
    Position->WriteOffset = offset;
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::AllocateAudioBuffer(ULONG RequestedSize,
                                                                   PMDL* AudioBufferMdl,
                                                                   ULONG* ActualSize,
                                                                   ULONG* OffsetFromFirstPage,
                                                                   MEMORY_CACHING_TYPE* CacheType)
{
    return AllocateBuffer(0, RequestedSize, AudioBufferMdl, ActualSize, OffsetFromFirstPage, CacheType);
}

_Use_decl_annotations_
STDMETHODIMP_(VOID) CMiniportWaveRTStream::FreeAudioBuffer(PMDL AudioBufferMdl, ULONG BufferSize)
{
    UNREFERENCED_PARAMETER(BufferSize);
    FreeBuffer(AudioBufferMdl);
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::AllocateBufferWithNotification(ULONG NotificationCount,
                                                                              ULONG RequestedSize,
                                                                              PMDL* AudioBufferMdl,
                                                                              ULONG* ActualSize,
                                                                              ULONG* OffsetFromFirstPage,
                                                                              MEMORY_CACHING_TYPE* CacheType)
{
    return AllocateBuffer(NotificationCount, RequestedSize, AudioBufferMdl, ActualSize, OffsetFromFirstPage, CacheType);
}

_Use_decl_annotations_
STDMETHODIMP_(VOID) CMiniportWaveRTStream::FreeBufferWithNotification(PMDL AudioBufferMdl, ULONG BufferSize)
{
    UNREFERENCED_PARAMETER(BufferSize);
    FreeBuffer(AudioBufferMdl);
}

_Use_decl_annotations_
STDMETHODIMP_(VOID) CMiniportWaveRTStream::GetHWLatency(KSRTAUDIO_HWLATENCY* hwLatency)
{
    if (hwLatency == nullptr)
    {
        return;
    }
    // No FIFO and no converters. The cable's ~30 ms is added by the ring, not by the endpoint.
    hwLatency->FifoSize = 0;
    hwLatency->ChipsetDelay = 0;
    hwLatency->CodecDelay = 0;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetPositionRegister(KSRTAUDIO_HWREGISTER* Register)
{
    UNREFERENCED_PARAMETER(Register);
    // No memory-mapped position register: clients use KSPROPERTY_AUDIO_POSITION (GetPosition).
    return STATUS_NOT_SUPPORTED;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::GetClockRegister(KSRTAUDIO_HWREGISTER* Register)
{
    UNREFERENCED_PARAMETER(Register);
    return STATUS_NOT_SUPPORTED;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::RegisterNotificationEvent(PKEVENT NotificationEvent)
{
    if (NotificationEvent == nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }

    NTSTATUS status = STATUS_INSUFFICIENT_RESOURCES;
    KIRQL oldIrql;
    KeAcquireSpinLock(&m_Lock, &oldIrql);
    for (ULONG i = 0; i < kMaxNotificationEvents; ++i)
    {
        if (m_Events[i] == NotificationEvent)
        {
            status = STATUS_SUCCESS;  // already registered
            break;
        }
        if (m_Events[i] == nullptr && status != STATUS_SUCCESS)
        {
            m_Events[i] = NotificationEvent;
            status = STATUS_SUCCESS;
            break;
        }
    }
    KeReleaseSpinLock(&m_Lock, oldIrql);
    return status;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportWaveRTStream::UnregisterNotificationEvent(PKEVENT NotificationEvent)
{
    if (NotificationEvent == nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }

    NTSTATUS status = STATUS_NOT_FOUND;
    KIRQL oldIrql;
    KeAcquireSpinLock(&m_Lock, &oldIrql);
    for (ULONG i = 0; i < kMaxNotificationEvents; ++i)
    {
        if (m_Events[i] == NotificationEvent)
        {
            m_Events[i] = nullptr;
            status = STATUS_SUCCESS;
        }
    }
    KeReleaseSpinLock(&m_Lock, oldIrql);
    return status;
}

// ------------------------------------------------------------------ buffer

_Use_decl_annotations_
NTSTATUS CMiniportWaveRTStream::AllocateBuffer(ULONG NotificationCount,
                                               ULONG RequestedSize,
                                               PMDL* AudioBufferMdl,
                                               ULONG* ActualSize,
                                               ULONG* OffsetFromFirstPage,
                                               MEMORY_CACHING_TYPE* CacheType)
{
    // Not paged: it takes the stream spin lock.
    *AudioBufferMdl = nullptr;
    *ActualSize = 0;
    *OffsetFromFirstPage = 0;
    *CacheType = MmCached;

    if (NotificationCount > kMaxNotificationsPerBuffer)
    {
        return STATUS_INVALID_PARAMETER;
    }
    if (m_BufferMdl != nullptr)
    {
        return STATUS_INVALID_DEVICE_REQUEST;  // one buffer per stream
    }

    // Clamp to 10 ms .. 2 s and round down to whole frames (a multiple of the notification
    // count, so notification boundaries fall on frames).
    const ULONG block = m_Format.BlockAlign;
    ULONG frames = RequestedSize / block;
    if (frames < kMinBufferFrames)
    {
        frames = kMinBufferFrames;
    }
    if (frames > kMaxBufferFrames)
    {
        frames = kMaxBufferFrames;
    }
    if (NotificationCount > 1)
    {
        frames -= frames % NotificationCount;
    }
    const ULONG bytes = frames * block;  // at most 96000 * 8, no overflow

    // No DMA constraint: any physical address will do.
    PHYSICAL_ADDRESS highest;
    highest.QuadPart = MAXLONGLONG;
    PMDL mdl = m_PortStream->AllocatePagesForMdl(highest, bytes);
    if (mdl == nullptr)
    {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    // The allocation may be partial.
    if (MmGetMdlByteCount(mdl) < bytes)
    {
        m_PortStream->FreePagesFromMdl(mdl);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    const PUCHAR buffer = static_cast<PUCHAR>(m_PortStream->MapAllocatedPages(mdl, MmCached));
    if (buffer == nullptr)
    {
        m_PortStream->FreePagesFromMdl(mdl);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    // A capture client must never read stale memory.
    RtlZeroMemory(buffer, bytes);

    KIRQL oldIrql;
    KeAcquireSpinLock(&m_Lock, &oldIrql);
    m_BufferMdl = mdl;
    m_Buffer = buffer;
    m_BufferBytes = bytes;
    m_BufferFrames = frames;
    m_NotificationCount = NotificationCount;
    KeReleaseSpinLock(&m_Lock, oldIrql);

    *AudioBufferMdl = mdl;
    *ActualSize = bytes;
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
void CMiniportWaveRTStream::FreeBuffer(PMDL AudioBufferMdl)
{
    if (AudioBufferMdl == nullptr)
    {
        return;
    }

    // Detach under the lock first, so no timer callback uses the mapping while it goes away.
    KIRQL oldIrql;
    KeAcquireSpinLock(&m_Lock, &oldIrql);
    if (AudioBufferMdl != m_BufferMdl)
    {
        KeReleaseSpinLock(&m_Lock, oldIrql);
        return;  // not ours
    }
    const PUCHAR buffer = m_Buffer;
    m_BufferMdl = nullptr;
    m_Buffer = nullptr;
    m_BufferBytes = 0;
    m_BufferFrames = 0;
    m_NotificationCount = 0;
    KeReleaseSpinLock(&m_Lock, oldIrql);

    m_PortStream->UnmapAllocatedPages(buffer, AudioBufferMdl);
    m_PortStream->FreePagesFromMdl(AudioBufferMdl);
}

// ------------------------------------------------------------------ clock

_Use_decl_annotations_
void CMiniportWaveRTStream::UpdateLocked(LONGLONG Now)
{
    if (!m_Running)
    {
        return;
    }

    const ULONG64 target = m_FramesAtRunStart + FramesFromTicks(Now - m_RunStartQpc, m_QpcFrequency);
    if (target <= m_Frames)
    {
        return;
    }
    if (m_Buffer == nullptr || m_BufferFrames == 0)
    {
        m_Frames = target;  // no buffer yet: time still passes
        return;
    }

    const ULONG64 notifyPeriod = m_NotificationCount != 0 ? m_BufferFrames / m_NotificationCount : 0;
    const ULONG64 notifyBefore = notifyPeriod != 0 ? m_Frames / notifyPeriod : 0;

    ULONG64 due = target - m_Frames;
    if (due > m_BufferFrames)
    {
        // Stalled for more than a whole buffer: those frames can no longer be moved.
        m_Frames += due - m_BufferFrames;
        due = m_BufferFrames;
    }

    const BOOLEAN poweredOn = m_Adapter->IsPoweredOn();
    ULONG remaining = (ULONG)due;
    while (remaining != 0)
    {
        const ULONG offset = (ULONG)(m_Frames % m_BufferFrames);
        const ULONG contiguous = m_BufferFrames - offset;
        const ULONG chunk = remaining < contiguous ? remaining : contiguous;
        const PUCHAR data = m_Buffer + (SIZE_T)offset * m_Format.BlockAlign;

        if (m_Direction == SpDirection::Render)
        {
            if (poweredOn)
            {
                m_Cable->Write(data, chunk, m_Format);
            }
        }
        else if (poweredOn)
        {
            m_Cable->Read(data, chunk, m_Format);
        }
        else
        {
            RtlZeroMemory(data, (SIZE_T)chunk * m_Format.BlockAlign);
        }

        m_Frames += chunk;
        remaining -= chunk;
    }

    if (notifyPeriod != 0 && m_Frames / notifyPeriod != notifyBefore)
    {
        for (ULONG i = 0; i < kMaxNotificationEvents; ++i)
        {
            if (m_Events[i] != nullptr)
            {
                KeSetEvent(m_Events[i], 0, FALSE);
            }
        }
    }
}

_Use_decl_annotations_
void CMiniportWaveRTStream::ArmTimerLocked()
{
    LONGLONG dueTicks = kPollTicks;
    if (m_NotificationCount != 0 && m_BufferFrames != 0)
    {
        // Fire just after the next notification boundary.
        const ULONG64 period = m_BufferFrames / m_NotificationCount;
        const ULONG64 framesToBoundary = period - (m_Frames % period);
        dueTicks = (LONGLONG)(framesToBoundary * kTicksPerSecond / SP_SAMPLE_RATE) + kTimerSlackTicks;
        if (dueTicks < kMinTimerTicks)
        {
            dueTicks = kMinTimerTicks;
        }
        if (dueTicks > kPollTicks)
        {
            dueTicks = kPollTicks;
        }
    }
    // Negative due time = relative. One-shot; the callback re-arms while the stream runs.
    ExSetTimer(m_Timer, -dueTicks, 0, nullptr);
}

_Use_decl_annotations_
void CMiniportWaveRTStream::TimerCallback(PEX_TIMER Timer, PVOID Context)
{
    UNREFERENCED_PARAMETER(Timer);

    // Runs at DISPATCH_LEVEL. The stream outlives its timer: the destructor deletes the
    // timer and waits for this callback before any member goes away.
    auto stream = static_cast<CMiniportWaveRTStream*>(Context);
    if (stream == nullptr)
    {
        return;
    }

    KIRQL oldIrql;
    KeAcquireSpinLock(&stream->m_Lock, &oldIrql);
    stream->UpdateLocked(KeQueryPerformanceCounter(nullptr).QuadPart);
    if (stream->m_Running)
    {
        stream->ArmTimerLocked();
    }
    KeReleaseSpinLock(&stream->m_Lock, oldIrql);
}

}  // namespace

// ------------------------------------------------------------------ factory

#pragma code_seg("PAGE")

_Use_decl_annotations_
NTSTATUS SpCreateMiniportWaveRT(PUNKNOWN* Unknown, SpDirection Direction)
{
    PAGED_CODE();

    *Unknown = nullptr;
    auto miniport = new (SpPool::NonPaged) CMiniportWaveRT(Direction);
    if (miniport == nullptr)
    {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    miniport->AddRef();
    *Unknown = static_cast<IMiniportWaveRT*>(miniport);
    return STATUS_SUCCESS;
}

#pragma code_seg()
