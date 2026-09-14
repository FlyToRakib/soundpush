// SoundPush Virtual Audio: topology miniports.
//
// A topology filter describes the "physical" side of an endpoint. Windows builds one
// audio endpoint per jack pin that it reaches from a WaveRT streaming pin. Ours is a
// straight connection from the wave-facing pin to the jack pin, with no nodes: volume
// and mute are handled in software by the Windows audio engine.
//
// The jack pin carries:
//   - its category, which sets the endpoint's form factor (line level for the feed, so it
//     looks like a cable rather than speakers; microphone for the capture side), and
//   - its name GUID. The INF maps each GUID to the endpoint name under
//     HKLM\SYSTEM\CurrentControlSet\Control\MediaCategories, which is where PortCls reads
//     pin names from. Both GUIDs must match the INF.

#include "common.h"

namespace
{

// {7AA30571-02ED-476D-B5AE-18F304A960E0}: "SoundPush Microphone Feed" (INF: SoundPush.FeedNameGuid)
const GUID kFeedPinName = {0x7aa30571, 0x02ed, 0x476d, {0xb5, 0xae, 0x18, 0xf3, 0x04, 0xa9, 0x60, 0xe0}};
// {D4ECC74D-A006-46DF-8196-27D088592DE6}: "SoundPush Microphone" (INF: SoundPush.MicNameGuid)
const GUID kMicPinName = {0xd4ecc74d, 0xa006, 0x46df, {0x81, 0x96, 0x27, 0xd0, 0x88, 0x59, 0x2d, 0xe6}};

// ------------------------------------------------------------------ jack description

NTSTATUS NTAPI PropertyHandlerJack(_In_ PPCPROPERTY_REQUEST Request);

// Fills a KSPROPERTY_DESCRIPTION for a basic-support query on a get-only property.
_IRQL_requires_(PASSIVE_LEVEL)
NTSTATUS BasicSupportGetOnly(_In_ PPCPROPERTY_REQUEST Request)
{
    PAGED_CODE();

    const ULONG access = KSPROPERTY_TYPE_BASICSUPPORT | KSPROPERTY_TYPE_GET;
    if (Request->ValueSize >= sizeof(KSPROPERTY_DESCRIPTION))
    {
        auto description = static_cast<PKSPROPERTY_DESCRIPTION>(Request->Value);
        description->AccessFlags = access;
        description->DescriptionSize = sizeof(KSPROPERTY_DESCRIPTION);
        description->PropTypeSet.Set = KSPROPTYPESETID_General;
        description->PropTypeSet.Id = VT_ILLEGAL;
        description->PropTypeSet.Flags = 0;
        description->MembersListCount = 0;
        description->Reserved = 0;
        Request->ValueSize = sizeof(KSPROPERTY_DESCRIPTION);
        return STATUS_SUCCESS;
    }
    if (Request->ValueSize >= sizeof(ULONG))
    {
        *static_cast<PULONG>(Request->Value) = access;
        Request->ValueSize = sizeof(ULONG);
        return STATUS_SUCCESS;
    }
    Request->ValueSize = 0;
    return STATUS_BUFFER_TOO_SMALL;
}

// Answers KSPROPERTY_JACK_DESCRIPTION and KSPROPERTY_JACK_DESCRIPTION2 on the jack pin.
// Both describe an always-connected device built into the computer.
_Use_decl_annotations_
NTSTATUS NTAPI PropertyHandlerJack(PPCPROPERTY_REQUEST Request)
{
    PAGED_CODE();

    if (Request == nullptr || Request->PropertyItem == nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }
    // Pin properties arrive as KSP_PIN: the pin id follows the KSPROPERTY header.
    if (Request->InstanceSize < sizeof(ULONG) || Request->Instance == nullptr)
    {
        return STATUS_INVALID_PARAMETER;
    }
    if (*static_cast<PULONG>(Request->Instance) != SP_TOPO_PIN_JACK)
    {
        return STATUS_INVALID_PARAMETER;
    }
    if (!IsEqualGUID(*Request->PropertyItem->Set, KSPROPSETID_Jack))
    {
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    const ULONG id = Request->PropertyItem->Id;
    if (id != KSPROPERTY_JACK_DESCRIPTION && id != KSPROPERTY_JACK_DESCRIPTION2)
    {
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    if (Request->Verb & KSPROPERTY_TYPE_BASICSUPPORT)
    {
        return BasicSupportGetOnly(Request);
    }
    if (!(Request->Verb & KSPROPERTY_TYPE_GET))
    {
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    // One jack: a KSMULTIPLE_ITEM header followed by one description.
    const ULONG itemSize = id == KSPROPERTY_JACK_DESCRIPTION ? sizeof(KSJACK_DESCRIPTION)
                                                             : sizeof(KSJACK_DESCRIPTION2);
    const ULONG required = sizeof(KSMULTIPLE_ITEM) + itemSize;
    if (Request->ValueSize == 0)
    {
        Request->ValueSize = required;
        return STATUS_BUFFER_OVERFLOW;
    }
    if (Request->ValueSize < required || Request->Value == nullptr)
    {
        return STATUS_BUFFER_TOO_SMALL;
    }

    auto header = static_cast<PKSMULTIPLE_ITEM>(Request->Value);
    header->Size = required;
    header->Count = 1;

    if (id == KSPROPERTY_JACK_DESCRIPTION)
    {
        auto jack = static_cast<PKSJACK_DESCRIPTION>(static_cast<PVOID>(header + 1));
        RtlZeroMemory(jack, sizeof(*jack));
        jack->ChannelMapping = KSAUDIO_SPEAKER_STEREO;
        jack->Color = 0;  // no physical jack colour
        jack->ConnectionType = eConnTypeUnknown;
        jack->GeoLocation = eGeoLocNotApplicable;
        jack->GenLocation = eGenLocInternal;
        jack->PortConnection = ePortConnIntegratedDevice;
        jack->IsConnected = TRUE;  // always present; never "unplugged"
    }
    else
    {
        auto jack = static_cast<PKSJACK_DESCRIPTION2>(static_cast<PVOID>(header + 1));
        RtlZeroMemory(jack, sizeof(*jack));
        jack->DeviceStateInfo = 0;
        jack->JackCapabilities = 0;  // no presence detection: it is always connected
    }

    Request->ValueSize = required;
    return STATUS_SUCCESS;
}

const PCPROPERTY_ITEM kJackProperties[] = {
    {&KSPROPSETID_Jack, KSPROPERTY_JACK_DESCRIPTION,
     PCPROPERTY_ITEM_FLAG_GET | PCPROPERTY_ITEM_FLAG_BASICSUPPORT, PropertyHandlerJack},
    {&KSPROPSETID_Jack, KSPROPERTY_JACK_DESCRIPTION2,
     PCPROPERTY_ITEM_FLAG_GET | PCPROPERTY_ITEM_FLAG_BASICSUPPORT, PropertyHandlerJack},
};

DEFINE_PCAUTOMATION_TABLE_PROP(kJackAutomation, kJackProperties);

// ------------------------------------------------------------------ filter descriptors

// Analog bridge pins carry no data format of their own.
KSDATARANGE kBridgeRange = {
    {sizeof(KSDATARANGE), 0, 0, 0,
     STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),
     STATICGUIDOF(KSDATAFORMAT_SUBTYPE_ANALOG),
     STATICGUIDOF(KSDATAFORMAT_SPECIFIER_NONE)}};

PKSDATARANGE kBridgeRanges[] = {&kBridgeRange};

// Render ("SoundPush Microphone Feed"): wave filter -> pin 0 -> pin 1 (line-level jack).
const PCPIN_DESCRIPTOR kRenderPins[] = {
    // SP_TOPO_PIN_WAVE
    {0, 0, 0, nullptr,
     {0, nullptr, 0, nullptr, SIZEOF_ARRAY(kBridgeRanges), kBridgeRanges,
      KSPIN_DATAFLOW_IN, KSPIN_COMMUNICATION_NONE, &KSCATEGORY_AUDIO, nullptr, 0}},
    // SP_TOPO_PIN_JACK
    {0, 0, 0, &kJackAutomation,
     {0, nullptr, 0, nullptr, SIZEOF_ARRAY(kBridgeRanges), kBridgeRanges,
      KSPIN_DATAFLOW_OUT, KSPIN_COMMUNICATION_NONE, &KSNODETYPE_LINE_CONNECTOR, &kFeedPinName, 0}},
};

const PCCONNECTION_DESCRIPTOR kRenderConnections[] = {
    {PCFILTER_NODE, SP_TOPO_PIN_WAVE, PCFILTER_NODE, SP_TOPO_PIN_JACK},
};

// Capture ("SoundPush Microphone"): pin 1 (microphone jack) -> pin 0 -> wave filter.
const PCPIN_DESCRIPTOR kCapturePins[] = {
    // SP_TOPO_PIN_WAVE
    {0, 0, 0, nullptr,
     {0, nullptr, 0, nullptr, SIZEOF_ARRAY(kBridgeRanges), kBridgeRanges,
      KSPIN_DATAFLOW_OUT, KSPIN_COMMUNICATION_NONE, &KSCATEGORY_AUDIO, nullptr, 0}},
    // SP_TOPO_PIN_JACK
    {0, 0, 0, &kJackAutomation,
     {0, nullptr, 0, nullptr, SIZEOF_ARRAY(kBridgeRanges), kBridgeRanges,
      KSPIN_DATAFLOW_IN, KSPIN_COMMUNICATION_NONE, &KSNODETYPE_MICROPHONE, &kMicPinName, 0}},
};

const PCCONNECTION_DESCRIPTOR kCaptureConnections[] = {
    {PCFILTER_NODE, SP_TOPO_PIN_JACK, PCFILTER_NODE, SP_TOPO_PIN_WAVE},
};

// Categories come from the INF interface registration (KSCATEGORY_AUDIO + TOPOLOGY).
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

// ------------------------------------------------------------------ miniport

class CMiniportTopology final : public IMiniportTopology
{
public:
    explicit CMiniportTopology(SpDirection Direction) : m_Direction(Direction) {}
    CMiniportTopology(const CMiniportTopology&) = delete;
    CMiniportTopology& operator=(const CMiniportTopology&) = delete;

    STDMETHODIMP QueryInterface(_In_ REFIID Iid, _COM_Outptr_ PVOID* Object) override
    {
        if (Object == nullptr)
        {
            return STATUS_INVALID_PARAMETER;
        }
        if (IsEqualGUID(Iid, IID_IUnknown) || IsEqualGUID(Iid, IID_IMiniport) ||
            IsEqualGUID(Iid, IID_IMiniportTopology))
        {
            *Object = static_cast<IMiniportTopology*>(this);
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

    IMP_IMiniportTopology;

private:
    ~CMiniportTopology() = default;

    LONG m_Refs = 0;
    SpDirection m_Direction;
};

#pragma code_seg("PAGE")

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportTopology::Init(PUNKNOWN UnknownAdapter, PRESOURCELIST ResourceList, PPORTTOPOLOGY Port)
{
    PAGED_CODE();
    UNREFERENCED_PARAMETER(UnknownAdapter);
    UNREFERENCED_PARAMETER(ResourceList);
    // The port is not kept: it owns this miniport, and a reference back would never be released.
    UNREFERENCED_PARAMETER(Port);
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CMiniportTopology::GetDescription(PPCFILTER_DESCRIPTOR* Description)
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
STDMETHODIMP_(NTSTATUS) CMiniportTopology::DataRangeIntersection(ULONG PinId,
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
    // Bridge pins only: let PortCls use its default handler.
    return STATUS_NOT_IMPLEMENTED;
}

#pragma code_seg()

}  // namespace

#pragma code_seg("PAGE")

_Use_decl_annotations_
NTSTATUS SpCreateMiniportTopology(PUNKNOWN* Unknown, SpDirection Direction)
{
    PAGED_CODE();

    *Unknown = nullptr;
    auto miniport = new (SpPool::NonPaged) CMiniportTopology(Direction);
    if (miniport == nullptr)
    {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    miniport->AddRef();
    *Unknown = static_cast<IMiniportTopology*>(miniport);
    return STATUS_SUCCESS;
}

#pragma code_seg()
