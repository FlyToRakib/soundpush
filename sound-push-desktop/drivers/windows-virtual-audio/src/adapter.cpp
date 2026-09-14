// SoundPush Virtual Audio: driver entry, device start and the adapter object.
//
// Start-up sequence (all at PASSIVE_LEVEL):
//   DriverEntry  -> PcInitializeAdapterDriver: PortCls owns the driver's dispatch table.
//   AddDevice    -> PcAddAdapterDevice: PortCls creates the functional device object.
//   StartDevice  -> creates the adapter object (ring buffer + power state), registers it for
//                   power management, then installs two endpoints. Each endpoint is a WaveRT
//                   filter and a topology filter joined by a physical connection.
//
// Teardown is reference counted: PortCls releases the ports when the device is removed,
// ports release their miniports, and miniports release the adapter, which frees the ring.

// PortCls declares its interface and class GUIDs (IID_IMiniportWaveRT, CLSID_PortWaveRT...)
// with DEFINE_GUID. Including initguid.h first makes this one file define them.
#include <initguid.h>
#include "common.h"
#include "cable.h"

// ------------------------------------------------------------------ allocation

_Use_decl_annotations_
void* __cdecl operator new(size_t Size, SpPool Pool)
{
    UNREFERENCED_PARAMETER(Pool);
    // ExAllocatePool2 zeroes the allocation and returns nullptr on failure.
    return ExAllocatePool2(POOL_FLAG_NON_PAGED, Size, SP_POOL_TAG);
}

// Matching placement delete (only used if a constructor could throw, which never happens).
_Use_decl_annotations_
void __cdecl operator delete(void* Pointer, SpPool Pool)
{
    UNREFERENCED_PARAMETER(Pool);
    if (Pointer != nullptr)
    {
        ExFreePoolWithTag(Pointer, SP_POOL_TAG);
    }
}

// The replaceable global deletes are predeclared by the compiler without SAL annotations.
void __cdecl operator delete(void* Pointer)
{
    if (Pointer != nullptr)
    {
        ExFreePoolWithTag(Pointer, SP_POOL_TAG);
    }
}

void __cdecl operator delete(void* Pointer, size_t Size)
{
    UNREFERENCED_PARAMETER(Size);
    if (Pointer != nullptr)
    {
        ExFreePoolWithTag(Pointer, SP_POOL_TAG);
    }
}

// ------------------------------------------------------------------ adapter object

namespace
{

class CAdapter final : public ISoundPushAdapter, public IAdapterPowerManagement
{
public:
    CAdapter() = default;
    CAdapter(const CAdapter&) = delete;
    CAdapter& operator=(const CAdapter&) = delete;

    // Inline members cannot be placed in the PAGE segment, so this wrapper has no PAGED_CODE
    // check (code analysis C28172); CCable::Initialize, which is paged, asserts the IRQL.
    _IRQL_requires_(PASSIVE_LEVEL)
    NTSTATUS Initialize()
    {
        return m_Cable.Initialize();
    }

    // IUnknown
    STDMETHODIMP QueryInterface(_In_ REFIID Iid, _COM_Outptr_ PVOID* Object) override
    {
        if (Object == nullptr)
        {
            return STATUS_INVALID_PARAMETER;
        }
        if (IsEqualGUID(Iid, IID_IUnknown) || IsEqualGUID(Iid, __uuidof(ISoundPushAdapter)))
        {
            *Object = static_cast<ISoundPushAdapter*>(this);
        }
        else if (IsEqualGUID(Iid, IID_IAdapterPowerManagement))
        {
            *Object = static_cast<IAdapterPowerManagement*>(this);
        }
        else
        {
            *Object = nullptr;
            return STATUS_INVALID_PARAMETER;
        }
        AddRef();
        return STATUS_SUCCESS;
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

    // ISoundPushAdapter
    CCable* STDMETHODCALLTYPE Cable() override
    {
        return &m_Cable;
    }

    BOOLEAN STDMETHODCALLTYPE IsPoweredOn() override
    {
        return InterlockedCompareExchange(&m_DeviceState, 0, 0) == (LONG)PowerDeviceD0;
    }

    // IAdapterPowerManagement. PortCls calls these at PASSIVE_LEVEL while it handles power
    // IRPs. They stay in non-paged code because they run while the system powers down.
    IMP_IAdapterPowerManagement;

private:
    // Only Release may destroy the object.
    ~CAdapter()
    {
        m_Cable.Cleanup();
    }

    LONG m_Refs = 0;
    // A virtual device is ready as soon as it starts.
    LONG m_DeviceState = (LONG)PowerDeviceD0;
    CCable m_Cable;
};

_Use_decl_annotations_
STDMETHODIMP_(void) CAdapter::PowerChangeState(POWER_STATE NewState)
{
    // There is no hardware to program: remember the state so streams move silence outside D0.
    // Streams are paused by the audio engine before the device leaves D0.
    if (NewState.DeviceState >= PowerDeviceD0 && NewState.DeviceState <= PowerDeviceD3)
    {
        InterlockedExchange(&m_DeviceState, (LONG)NewState.DeviceState);
    }
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CAdapter::QueryPowerChangeState(POWER_STATE NewStateQuery)
{
    UNREFERENCED_PARAMETER(NewStateQuery);
    // Nothing prevents any transition.
    return STATUS_SUCCESS;
}

_Use_decl_annotations_
STDMETHODIMP_(NTSTATUS) CAdapter::QueryDeviceCapabilities(PDEVICE_CAPABILITIES PowerDeviceCaps)
{
    UNREFERENCED_PARAMETER(PowerDeviceCaps);
    // Keep the capabilities the bus reported.
    return STATUS_SUCCESS;
}

}  // namespace

// ------------------------------------------------------------------ device start

extern "C" DRIVER_INITIALIZE DriverEntry;
DRIVER_ADD_DEVICE SpAddDevice;

// Each endpoint uses two subdevices (a WaveRT and a topology filter).
static constexpr ULONG kMaxSubdevices = 4;

#pragma code_seg("PAGE")

// Creates a port of the given class, binds it to the miniport and registers it as a KS
// filter named Name. On success *Port holds a reference the caller must release.
_IRQL_requires_(PASSIVE_LEVEL)
static NTSTATUS InstallSubdevice(_In_ PDEVICE_OBJECT DeviceObject,
                                 _In_ PIRP Irp,
                                 _In_ PCWSTR Name,
                                 _In_ REFCLSID PortClass,
                                 _In_ PUNKNOWN Miniport,
                                 _In_ PUNKNOWN Adapter,
                                 _In_ PRESOURCELIST ResourceList,
                                 _Outptr_result_maybenull_ PUNKNOWN* Port)
{
    PAGED_CODE();

    *Port = nullptr;

    PPORT port = nullptr;
    NTSTATUS status = PcNewPort(&port, PortClass);
    if (!NT_SUCCESS(status))
    {
        return status;
    }

    status = port->Init(DeviceObject, Irp, Miniport, Adapter, ResourceList);
    if (NT_SUCCESS(status))
    {
        // PcRegisterSubdevice takes a non-const name but does not modify it.
        status = PcRegisterSubdevice(DeviceObject, const_cast<PWSTR>(Name), port);
    }
    if (!NT_SUCCESS(status))
    {
        port->Release();
        return status;
    }

    *Port = port;
    return STATUS_SUCCESS;
}

// Installs one endpoint: its WaveRT filter, its topology filter and the connection between them.
_IRQL_requires_(PASSIVE_LEVEL)
static NTSTATUS InstallEndpoint(_In_ PDEVICE_OBJECT DeviceObject,
                                _In_ PIRP Irp,
                                _In_ PRESOURCELIST ResourceList,
                                _In_ PUNKNOWN Adapter,
                                _In_ SpDirection Direction)
{
    PAGED_CODE();

    const bool render = Direction == SpDirection::Render;
    PUNKNOWN waveMiniport = nullptr;
    PUNKNOWN topologyMiniport = nullptr;
    PUNKNOWN wavePort = nullptr;
    PUNKNOWN topologyPort = nullptr;

    NTSTATUS status = SpCreateMiniportWaveRT(&waveMiniport, Direction);
    if (NT_SUCCESS(status))
    {
        status = SpCreateMiniportTopology(&topologyMiniport, Direction);
    }
    if (NT_SUCCESS(status))
    {
        status = InstallSubdevice(DeviceObject, Irp,
                                  render ? SP_NAME_TOPOLOGY_RENDER : SP_NAME_TOPOLOGY_CAPTURE,
                                  CLSID_PortTopology, topologyMiniport, Adapter, ResourceList,
                                  &topologyPort);
    }
    if (NT_SUCCESS(status))
    {
        status = InstallSubdevice(DeviceObject, Irp,
                                  render ? SP_NAME_WAVE_RENDER : SP_NAME_WAVE_CAPTURE,
                                  CLSID_PortWaveRT, waveMiniport, Adapter, ResourceList,
                                  &wavePort);
    }
    if (NT_SUCCESS(status) && (wavePort == nullptr || topologyPort == nullptr))
    {
        status = STATUS_UNSUCCESSFUL;  // unreachable: InstallSubdevice sets the port on success
    }
    if (NT_SUCCESS(status))
    {
        // Audio flows wave -> topology when rendering and topology -> wave when capturing.
        // Windows follows this connection from the streaming pin to the jack pin to build
        // the endpoint.
        status = render
            ? PcRegisterPhysicalConnection(DeviceObject, wavePort, SP_WAVE_PIN_BRIDGE,
                                           topologyPort, SP_TOPO_PIN_WAVE)
            : PcRegisterPhysicalConnection(DeviceObject, topologyPort, SP_TOPO_PIN_WAVE,
                                           wavePort, SP_WAVE_PIN_BRIDGE);
    }

    // Registered ports keep their own references; on failure PortCls unregisters whatever
    // was registered when the failed start is followed by device removal.
    if (wavePort != nullptr)
    {
        wavePort->Release();
    }
    if (topologyPort != nullptr)
    {
        topologyPort->Release();
    }
    if (waveMiniport != nullptr)
    {
        waveMiniport->Release();
    }
    if (topologyMiniport != nullptr)
    {
        topologyMiniport->Release();
    }
    return status;
}

// PortCls calls this when the PnP manager starts the device.
_IRQL_requires_(PASSIVE_LEVEL)
static NTSTATUS SpStartDevice(_In_ PDEVICE_OBJECT DeviceObject,
                              _In_ PIRP Irp,
                              _In_ PRESOURCELIST ResourceList)
{
    PAGED_CODE();

    // A virtual device has no hardware resources.
    CAdapter* adapter = new (SpPool::NonPaged) CAdapter();
    if (adapter == nullptr)
    {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    adapter->AddRef();  // this function's reference

    const PUNKNOWN adapterUnknown = static_cast<ISoundPushAdapter*>(adapter);

    NTSTATUS status = adapter->Initialize();
    if (NT_SUCCESS(status))
    {
        // PortCls forwards D-state changes to IAdapterPowerManagement and keeps a reference.
        status = PcRegisterAdapterPowerManagement(adapterUnknown, DeviceObject);
    }
    if (NT_SUCCESS(status))
    {
        status = InstallEndpoint(DeviceObject, Irp, ResourceList, adapterUnknown, SpDirection::Render);
    }
    if (NT_SUCCESS(status))
    {
        status = InstallEndpoint(DeviceObject, Irp, ResourceList, adapterUnknown, SpDirection::Capture);
    }

    adapter->Release();
    return status;
}

_Use_decl_annotations_
NTSTATUS SpAddDevice(PDRIVER_OBJECT DriverObject, PDEVICE_OBJECT PhysicalDeviceObject)
{
    PAGED_CODE();

    // Device extension size 0: the driver keeps no per-device state outside the adapter object.
    return PcAddAdapterDevice(DriverObject, PhysicalDeviceObject, SpStartDevice, kMaxSubdevices, 0);
}

#pragma code_seg()

#pragma code_seg("INIT")

_Use_decl_annotations_
NTSTATUS DriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath)
{
    // PortCls installs its own dispatch routines and unload handler. The driver has no
    // global state, so it needs nothing else.
    return PcInitializeAdapterDriver(DriverObject, RegistryPath, SpAddDevice);
}

#pragma code_seg()
