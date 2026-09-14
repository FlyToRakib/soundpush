// SoundPush Virtual Audio: shared declarations for the Windows kernel-mode audio driver.
//
// The driver is a PortCls WaveRT adapter with two endpoints on one device:
//
//   "SoundPush Microphone Feed"  render endpoint. The SoundPush engine plays the phone's
//                                microphone into it (like "CABLE Input" of VB-CABLE).
//   "SoundPush Microphone"       capture endpoint. Meet, Zoom, Discord and recorders
//                                record from it (like "CABLE Output").
//
// Each endpoint is a pair of PortCls filters: a WaveRT filter (streaming) and a topology
// filter (the "jack" that Windows turns into an endpoint). Audio written to the feed is
// copied into a lock-free ring buffer (cable.h) by the render stream and read back by
// the capture stream. Both streams run on the same clock (the performance counter), so
// the ring stays at a constant fill level. When nothing is fed, apps record silence.
//
// The code is C-style C++: PortCls exposes COM-style C++ interfaces. There are no
// exceptions, no RTTI, no STL and no global constructors.

#pragma once

#include <portcls.h>
#include <ksmedia.h>

// Pool tag "SpVa" (reads correctly in pool tools, which print tags little-endian).
#define SP_POOL_TAG 'aVpS'

// Every driver object is allocated from zeroed, non-paged, non-executable pool.
// Use: new (SpPool::NonPaged) CSomething(...). Returns nullptr on failure.
// (A distinct tag type: POOL_FLAGS is the same type as size_t and would collide with sized delete.)
enum class SpPool
{
    NonPaged,
};

_IRQL_requires_max_(DISPATCH_LEVEL)
void* __cdecl operator new(size_t Size, SpPool Pool);
_IRQL_requires_max_(DISPATCH_LEVEL)
void __cdecl operator delete(void* Pointer, SpPool Pool);
// The replaceable operator delete(void*) and delete(void*, size_t) are defined in adapter.cpp
// (the kernel has no C++ runtime); the compiler already declares them.

// ------------------------------------------------------------------ audio format

// The only sample rate. SoundPush's engine and the phone both work at 48 kHz, so the
// driver never resamples.
inline constexpr ULONG SP_SAMPLE_RATE = 48000;
inline constexpr ULONG SP_MAX_CHANNELS = 2;

// A validated integer PCM stream format (see SpParseFormat in wavert.cpp).
struct SP_PCM_FORMAT
{
    ULONG Channels;        // 1 or 2
    ULONG BytesPerSample;  // 2 (16-bit), 3 (24-bit) or 4 (32-bit, 24 or 32 valid bits)
    ULONG BlockAlign;      // Channels * BytesPerSample
};

// True when the format has one of the shapes above.
inline bool SpIsValidFormat(const SP_PCM_FORMAT& Format)
{
    return Format.Channels >= 1 && Format.Channels <= SP_MAX_CHANNELS &&
           Format.BytesPerSample >= 2 && Format.BytesPerSample <= 4 &&
           Format.BlockAlign == Format.Channels * Format.BytesPerSample;
}

// ------------------------------------------------------------------ topology

enum class SpDirection
{
    Render,   // "SoundPush Microphone Feed": SoundPush writes here
    Capture,  // "SoundPush Microphone": apps record from here
};

// Subdevice names. They are the KS reference strings in the INF (AddInterface lines);
// both must change together.
#define SP_NAME_WAVE_RENDER      L"WaveRender"
#define SP_NAME_TOPOLOGY_RENDER  L"TopologyRender"
#define SP_NAME_WAVE_CAPTURE     L"WaveCapture"
#define SP_NAME_TOPOLOGY_CAPTURE L"TopologyCapture"

// WaveRT filter pins.
inline constexpr ULONG SP_WAVE_PIN_STREAM = 0;  // clients (the Windows audio engine) connect here
inline constexpr ULONG SP_WAVE_PIN_BRIDGE = 1;  // leads to the topology filter

// Topology filter pins.
inline constexpr ULONG SP_TOPO_PIN_WAVE = 0;    // leads to the WaveRT filter
inline constexpr ULONG SP_TOPO_PIN_JACK = 1;    // the endpoint itself; carries its name and jack info

// ------------------------------------------------------------------ adapter

class CCable;

// Private interface between the adapter object and its miniports. Miniports receive the
// adapter's IUnknown in Init and query for this interface.
struct DECLSPEC_UUID("00e9edc7-771c-4445-9320-187f1ac18867") DECLSPEC_NOVTABLE
ISoundPushAdapter : public IUnknown
{
    // The ring buffer joining the render and capture endpoints. Lives as long as the adapter.
    virtual CCable* STDMETHODCALLTYPE Cable() = 0;
    // True while the device is in D0. Streams move silence while it is not.
    virtual BOOLEAN STDMETHODCALLTYPE IsPoweredOn() = 0;
};

// Miniport factories. On success *Unknown holds one reference owned by the caller.
_IRQL_requires_(PASSIVE_LEVEL)
NTSTATUS SpCreateMiniportTopology(_Outptr_ PUNKNOWN* Unknown, _In_ SpDirection Direction);

_IRQL_requires_(PASSIVE_LEVEL)
NTSTATUS SpCreateMiniportWaveRT(_Outptr_ PUNKNOWN* Unknown, _In_ SpDirection Direction);
