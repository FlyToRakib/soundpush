// SoundPush Microphone: a CoreAudio AudioServerPlugIn virtual microphone for macOS.
//
// One device, two streams:
//   - an output stream that SoundPush plays the phone's microphone into, and
//   - an input stream that other apps (Meet, Zoom, recorders) record from.
// Audio written at output sample time T is read back when the input sample time
// reaches T, through a ring buffer on the device's shared clock.
//
// The output side cannot become the default output device, so it never appears as a
// speaker in System Settings. When nothing is feeding the device, apps read silence.

#include <CoreAudio/AudioServerPlugIn.h>
#include <mach/mach_time.h>
#include <pthread.h>
#include <string.h>

#define kDeviceName     "SoundPush Microphone"
#define kDeviceUID      "net.soundpush.microphone.device"
#define kDeviceModelUID "net.soundpush.microphone.model"
#define kManufacturer   "SoundPush"

enum {
    kObjectID_PlugIn = kAudioObjectPlugInObject,
    kObjectID_Device = 2,
    kObjectID_InputStream = 3,
    kObjectID_OutputStream = 4,
};

static const Float64 kSampleRate = 48000.0;
static const UInt32 kChannels = 2;
/// Ring buffer length in frames; also the zero-timestamp period (~0.34 s).
#define kRingFrames 16384

// ------------------------------------------------------------------ state

static pthread_mutex_t gStateMutex = PTHREAD_MUTEX_INITIALIZER;
static AudioServerPlugInHostRef gHost = NULL;
static UInt32 gRefCount = 0;
static UInt32 gIOClients = 0;
static UInt64 gAnchorHostTime = 0;
static Float64 gHostTicksPerFrame = 0;
static Boolean gInputStreamActive = true;
static Boolean gOutputStreamActive = true;

// Touched only by the device's IO thread (all IO operations for a device run on it).
static Float32 gRing[kRingFrames * 2];
/// Sample time just past the last written frame; < 0 when nothing has been written.
static Float64 gWriteEnd = -1;

// ------------------------------------------------------------------ interface

static HRESULT PlugIn_QueryInterface(void* inDriver, REFIID inUUID, LPVOID* outInterface);
static ULONG PlugIn_AddRef(void* inDriver);
static ULONG PlugIn_Release(void* inDriver);
static OSStatus PlugIn_Initialize(AudioServerPlugInDriverRef inDriver, AudioServerPlugInHostRef inHost);
static OSStatus PlugIn_CreateDevice(AudioServerPlugInDriverRef inDriver, CFDictionaryRef inDescription,
                                    const AudioServerPlugInClientInfo* inClientInfo, AudioObjectID* outDeviceObjectID);
static OSStatus PlugIn_DestroyDevice(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID);
static OSStatus Device_AddClient(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                 const AudioServerPlugInClientInfo* inClientInfo);
static OSStatus Device_RemoveClient(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                    const AudioServerPlugInClientInfo* inClientInfo);
static OSStatus Device_PerformConfigurationChange(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                                  UInt64 inChangeAction, void* inChangeInfo);
static OSStatus Device_AbortConfigurationChange(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                                UInt64 inChangeAction, void* inChangeInfo);
static Boolean HasProperty(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                           const AudioObjectPropertyAddress* inAddress);
static OSStatus IsPropertySettable(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                                   const AudioObjectPropertyAddress* inAddress, Boolean* outIsSettable);
static OSStatus GetPropertyDataSize(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                                    const AudioObjectPropertyAddress* inAddress, UInt32 inQualifierDataSize,
                                    const void* inQualifierData, UInt32* outDataSize);
static OSStatus GetPropertyData(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                                const AudioObjectPropertyAddress* inAddress, UInt32 inQualifierDataSize,
                                const void* inQualifierData, UInt32 inDataSize, UInt32* outDataSize, void* outData);
static OSStatus SetPropertyData(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                                const AudioObjectPropertyAddress* inAddress, UInt32 inQualifierDataSize,
                                const void* inQualifierData, UInt32 inDataSize, const void* inData);
static OSStatus Device_StartIO(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID, UInt32 inClientID);
static OSStatus Device_StopIO(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID, UInt32 inClientID);
static OSStatus Device_GetZeroTimeStamp(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                        UInt32 inClientID, Float64* outSampleTime, UInt64* outHostTime, UInt64* outSeed);
static OSStatus Device_WillDoIOOperation(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                         UInt32 inClientID, UInt32 inOperationID, Boolean* outWillDo,
                                         Boolean* outWillDoInPlace);
static OSStatus Device_BeginIOOperation(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                        UInt32 inClientID, UInt32 inOperationID, UInt32 inIOBufferFrameSize,
                                        const AudioServerPlugInIOCycleInfo* inIOCycleInfo);
static OSStatus Device_DoIOOperation(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                     AudioObjectID inStreamObjectID, UInt32 inClientID, UInt32 inOperationID,
                                     UInt32 inIOBufferFrameSize, const AudioServerPlugInIOCycleInfo* inIOCycleInfo,
                                     void* ioMainBuffer, void* ioSecondaryBuffer);
static OSStatus Device_EndIOOperation(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                      UInt32 inClientID, UInt32 inOperationID, UInt32 inIOBufferFrameSize,
                                      const AudioServerPlugInIOCycleInfo* inIOCycleInfo);

static AudioServerPlugInDriverInterface gInterface = {
    NULL,
    PlugIn_QueryInterface,
    PlugIn_AddRef,
    PlugIn_Release,
    PlugIn_Initialize,
    PlugIn_CreateDevice,
    PlugIn_DestroyDevice,
    Device_AddClient,
    Device_RemoveClient,
    Device_PerformConfigurationChange,
    Device_AbortConfigurationChange,
    HasProperty,
    IsPropertySettable,
    GetPropertyDataSize,
    GetPropertyData,
    SetPropertyData,
    Device_StartIO,
    Device_StopIO,
    Device_GetZeroTimeStamp,
    Device_WillDoIOOperation,
    Device_BeginIOOperation,
    Device_DoIOOperation,
    Device_EndIOOperation,
};
static AudioServerPlugInDriverInterface* gInterfacePtr = &gInterface;
static AudioServerPlugInDriverRef gDriverRef = &gInterfacePtr;

// ------------------------------------------------------------------ factory

__attribute__((visibility("default")))
void* SoundPush_Create(CFAllocatorRef inAllocator, CFUUIDRef inRequestedTypeUUID) {
    (void)inAllocator;
    if (CFEqual(inRequestedTypeUUID, kAudioServerPlugInTypeUUID)) {
        return gDriverRef;
    }
    return NULL;
}

static HRESULT PlugIn_QueryInterface(void* inDriver, REFIID inUUID, LPVOID* outInterface) {
    if (inDriver != gDriverRef || outInterface == NULL) {
        return kAudioHardwareBadObjectError;
    }
    CFUUIDRef requested = CFUUIDCreateFromUUIDBytes(NULL, inUUID);
    if (requested == NULL) {
        return kAudioHardwareIllegalOperationError;
    }
    HRESULT result = E_NOINTERFACE;
    if (CFEqual(requested, IUnknownUUID) || CFEqual(requested, kAudioServerPlugInDriverInterfaceUUID)) {
        pthread_mutex_lock(&gStateMutex);
        gRefCount++;
        pthread_mutex_unlock(&gStateMutex);
        *outInterface = gDriverRef;
        result = S_OK;
    }
    CFRelease(requested);
    return result;
}

static ULONG PlugIn_AddRef(void* inDriver) {
    if (inDriver != gDriverRef) {
        return 0;
    }
    pthread_mutex_lock(&gStateMutex);
    ULONG count = ++gRefCount;
    pthread_mutex_unlock(&gStateMutex);
    return count;
}

static ULONG PlugIn_Release(void* inDriver) {
    if (inDriver != gDriverRef) {
        return 0;
    }
    pthread_mutex_lock(&gStateMutex);
    if (gRefCount > 0) {
        gRefCount--;
    }
    ULONG count = gRefCount;
    pthread_mutex_unlock(&gStateMutex);
    return count;
}

static OSStatus PlugIn_Initialize(AudioServerPlugInDriverRef inDriver, AudioServerPlugInHostRef inHost) {
    if (inDriver != gDriverRef) {
        return kAudioHardwareBadObjectError;
    }
    gHost = inHost;
    mach_timebase_info_data_t timebase;
    mach_timebase_info(&timebase);
    Float64 ticksPerSecond = 1.0e9 * (Float64)timebase.denom / (Float64)timebase.numer;
    gHostTicksPerFrame = ticksPerSecond / kSampleRate;
    return noErr;
}

static OSStatus PlugIn_CreateDevice(AudioServerPlugInDriverRef inDriver, CFDictionaryRef inDescription,
                                    const AudioServerPlugInClientInfo* inClientInfo, AudioObjectID* outDeviceObjectID) {
    (void)inDriver, (void)inDescription, (void)inClientInfo, (void)outDeviceObjectID;
    return kAudioHardwareUnsupportedOperationError;
}

static OSStatus PlugIn_DestroyDevice(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID) {
    (void)inDriver, (void)inDeviceObjectID;
    return kAudioHardwareUnsupportedOperationError;
}

static OSStatus Device_AddClient(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                 const AudioServerPlugInClientInfo* inClientInfo) {
    (void)inClientInfo;
    return (inDriver == gDriverRef && inDeviceObjectID == kObjectID_Device) ? noErr : kAudioHardwareBadObjectError;
}

static OSStatus Device_RemoveClient(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                    const AudioServerPlugInClientInfo* inClientInfo) {
    (void)inClientInfo;
    return (inDriver == gDriverRef && inDeviceObjectID == kObjectID_Device) ? noErr : kAudioHardwareBadObjectError;
}

static OSStatus Device_PerformConfigurationChange(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                                  UInt64 inChangeAction, void* inChangeInfo) {
    (void)inChangeAction, (void)inChangeInfo;
    return (inDriver == gDriverRef && inDeviceObjectID == kObjectID_Device) ? noErr : kAudioHardwareBadObjectError;
}

static OSStatus Device_AbortConfigurationChange(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                                UInt64 inChangeAction, void* inChangeInfo) {
    (void)inChangeAction, (void)inChangeInfo;
    return (inDriver == gDriverRef && inDeviceObjectID == kObjectID_Device) ? noErr : kAudioHardwareBadObjectError;
}

// ------------------------------------------------------------------ properties

static void FillFormat(AudioStreamBasicDescription* format) {
    format->mSampleRate = kSampleRate;
    format->mFormatID = kAudioFormatLinearPCM;
    format->mFormatFlags = kAudioFormatFlagsNativeFloatPacked;
    format->mBytesPerPacket = sizeof(Float32) * kChannels;
    format->mFramesPerPacket = 1;
    format->mBytesPerFrame = sizeof(Float32) * kChannels;
    format->mChannelsPerFrame = kChannels;
    format->mBitsPerChannel = 32;
    format->mReserved = 0;
}

static Boolean IsSupportedFormat(const AudioStreamBasicDescription* format) {
    return format->mSampleRate == kSampleRate && format->mFormatID == kAudioFormatLinearPCM &&
           format->mChannelsPerFrame == kChannels && format->mBitsPerChannel == 32 &&
           (format->mFormatFlags & kAudioFormatFlagIsFloat) != 0;
}

// Helpers for GetPropertyData. With outData == NULL only the size is reported.
#define RETURN_VALUE(type, value)                                     \
    do {                                                              \
        if (outData != NULL) {                                        \
            if (inDataSize < sizeof(type)) {                          \
                return kAudioHardwareBadPropertySizeError;            \
            }                                                         \
            *(type*)outData = (value);                                \
        }                                                             \
        *outDataSize = sizeof(type);                                  \
        return noErr;                                                 \
    } while (0)

static OSStatus ReturnObjectList(const AudioObjectID* ids, UInt32 count, UInt32 inDataSize, UInt32* outDataSize,
                                 void* outData) {
    if (outData == NULL) {
        *outDataSize = count * sizeof(AudioObjectID);
        return noErr;
    }
    UInt32 fits = inDataSize / sizeof(AudioObjectID);
    UInt32 n = count < fits ? count : fits;
    if (n > 0) {
        memcpy(outData, ids, n * sizeof(AudioObjectID));
    }
    *outDataSize = n * sizeof(AudioObjectID);
    return noErr;
}

/// Streams visible in a scope: input, output, or both for the global scope.
static UInt32 StreamsForScope(AudioObjectPropertyScope scope, AudioObjectID* ids) {
    UInt32 count = 0;
    if (scope == kAudioObjectPropertyScopeGlobal || scope == kAudioObjectPropertyScopeInput) {
        ids[count++] = kObjectID_InputStream;
    }
    if (scope == kAudioObjectPropertyScopeGlobal || scope == kAudioObjectPropertyScopeOutput) {
        ids[count++] = kObjectID_OutputStream;
    }
    return count;
}

static OSStatus PlugInProperty(const AudioObjectPropertyAddress* address, UInt32 inQualifierDataSize,
                               const void* inQualifierData, UInt32 inDataSize, UInt32* outDataSize, void* outData) {
    static const AudioObjectID devices[] = {kObjectID_Device};
    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
            RETURN_VALUE(AudioClassID, kAudioObjectClassID);
        case kAudioObjectPropertyClass:
            RETURN_VALUE(AudioClassID, kAudioPlugInClassID);
        case kAudioObjectPropertyOwner:
            RETURN_VALUE(AudioObjectID, kAudioObjectUnknown);
        case kAudioObjectPropertyManufacturer:
            RETURN_VALUE(CFStringRef, CFSTR(kManufacturer));
        case kAudioObjectPropertyOwnedObjects:
        case kAudioPlugInPropertyDeviceList:
            return ReturnObjectList(devices, 1, inDataSize, outDataSize, outData);
        case kAudioPlugInPropertyTranslateUIDToDevice: {
            AudioObjectID found = kAudioObjectUnknown;
            if (outData != NULL && inQualifierDataSize == sizeof(CFStringRef) && inQualifierData != NULL) {
                CFStringRef uid = *(const CFStringRef*)inQualifierData;
                if (uid != NULL && CFStringCompare(uid, CFSTR(kDeviceUID), 0) == kCFCompareEqualTo) {
                    found = kObjectID_Device;
                }
            }
            RETURN_VALUE(AudioObjectID, found);
        }
        case kAudioPlugInPropertyResourceBundle:
            RETURN_VALUE(CFStringRef, CFSTR(""));
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus DeviceProperty(const AudioObjectPropertyAddress* address, UInt32 inDataSize, UInt32* outDataSize,
                               void* outData) {
    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
            RETURN_VALUE(AudioClassID, kAudioObjectClassID);
        case kAudioObjectPropertyClass:
            RETURN_VALUE(AudioClassID, kAudioDeviceClassID);
        case kAudioObjectPropertyOwner:
            RETURN_VALUE(AudioObjectID, kObjectID_PlugIn);
        case kAudioObjectPropertyName:
            RETURN_VALUE(CFStringRef, CFSTR(kDeviceName));
        case kAudioObjectPropertyManufacturer:
            RETURN_VALUE(CFStringRef, CFSTR(kManufacturer));
        case kAudioDevicePropertyDeviceUID:
            RETURN_VALUE(CFStringRef, CFSTR(kDeviceUID));
        case kAudioDevicePropertyModelUID:
            RETURN_VALUE(CFStringRef, CFSTR(kDeviceModelUID));
        case kAudioObjectPropertyOwnedObjects:
        case kAudioDevicePropertyStreams: {
            AudioObjectID ids[2];
            UInt32 count = StreamsForScope(address->mScope, ids);
            return ReturnObjectList(ids, count, inDataSize, outDataSize, outData);
        }
        case kAudioObjectPropertyControlList:
            *outDataSize = 0;
            return noErr;
        case kAudioDevicePropertyRelatedDevices: {
            static const AudioObjectID related[] = {kObjectID_Device};
            return ReturnObjectList(related, 1, inDataSize, outDataSize, outData);
        }
        case kAudioDevicePropertyTransportType:
            RETURN_VALUE(UInt32, kAudioDeviceTransportTypeVirtual);
        case kAudioDevicePropertyClockDomain:
            RETURN_VALUE(UInt32, 0);
        case kAudioDevicePropertyDeviceIsAlive:
            RETURN_VALUE(UInt32, 1);
        case kAudioDevicePropertyDeviceIsRunning: {
            pthread_mutex_lock(&gStateMutex);
            UInt32 running = gIOClients > 0 ? 1 : 0;
            pthread_mutex_unlock(&gStateMutex);
            RETURN_VALUE(UInt32, running);
        }
        case kAudioDevicePropertyDeviceCanBeDefaultDevice:
            // Apps may use it as their default microphone; it is never offered as a speaker.
            RETURN_VALUE(UInt32, address->mScope == kAudioObjectPropertyScopeInput ? 1 : 0);
        case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice:
            RETURN_VALUE(UInt32, 0);
        case kAudioDevicePropertyLatency:
        case kAudioDevicePropertySafetyOffset:
            RETURN_VALUE(UInt32, 0);
        case kAudioDevicePropertyNominalSampleRate:
            RETURN_VALUE(Float64, kSampleRate);
        case kAudioDevicePropertyAvailableNominalSampleRates: {
            AudioValueRange range = {kSampleRate, kSampleRate};
            RETURN_VALUE(AudioValueRange, range);
        }
        case kAudioDevicePropertyIsHidden:
            RETURN_VALUE(UInt32, 0);
        case kAudioDevicePropertyPreferredChannelsForStereo: {
            if (outData != NULL) {
                if (inDataSize < 2 * sizeof(UInt32)) {
                    return kAudioHardwareBadPropertySizeError;
                }
                ((UInt32*)outData)[0] = 1;
                ((UInt32*)outData)[1] = 2;
            }
            *outDataSize = 2 * sizeof(UInt32);
            return noErr;
        }
        case kAudioDevicePropertyZeroTimeStampPeriod:
            RETURN_VALUE(UInt32, kRingFrames);
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus StreamProperty(AudioObjectID stream, const AudioObjectPropertyAddress* address, UInt32 inDataSize,
                               UInt32* outDataSize, void* outData) {
    Boolean isInput = stream == kObjectID_InputStream;
    switch (address->mSelector) {
        case kAudioObjectPropertyBaseClass:
            RETURN_VALUE(AudioClassID, kAudioObjectClassID);
        case kAudioObjectPropertyClass:
            RETURN_VALUE(AudioClassID, kAudioStreamClassID);
        case kAudioObjectPropertyOwner:
            RETURN_VALUE(AudioObjectID, kObjectID_Device);
        case kAudioObjectPropertyOwnedObjects:
            *outDataSize = 0;
            return noErr;
        case kAudioStreamPropertyIsActive: {
            pthread_mutex_lock(&gStateMutex);
            UInt32 active = isInput ? gInputStreamActive : gOutputStreamActive;
            pthread_mutex_unlock(&gStateMutex);
            RETURN_VALUE(UInt32, active);
        }
        case kAudioStreamPropertyDirection:
            RETURN_VALUE(UInt32, isInput ? 1 : 0);
        case kAudioStreamPropertyTerminalType:
            RETURN_VALUE(UInt32, isInput ? kAudioStreamTerminalTypeMicrophone : kAudioStreamTerminalTypeSpeaker);
        case kAudioStreamPropertyStartingChannel:
            RETURN_VALUE(UInt32, 1);
        case kAudioStreamPropertyLatency:
            RETURN_VALUE(UInt32, 0);
        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyPhysicalFormat: {
            AudioStreamBasicDescription format;
            FillFormat(&format);
            RETURN_VALUE(AudioStreamBasicDescription, format);
        }
        case kAudioStreamPropertyAvailableVirtualFormats:
        case kAudioStreamPropertyAvailablePhysicalFormats: {
            AudioStreamRangedDescription ranged;
            FillFormat(&ranged.mFormat);
            ranged.mSampleRateRange.mMinimum = kSampleRate;
            ranged.mSampleRateRange.mMaximum = kSampleRate;
            RETURN_VALUE(AudioStreamRangedDescription, ranged);
        }
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus PropertyData(AudioObjectID objectID, const AudioObjectPropertyAddress* address,
                             UInt32 inQualifierDataSize, const void* inQualifierData, UInt32 inDataSize,
                             UInt32* outDataSize, void* outData) {
    if (address == NULL || outDataSize == NULL) {
        return kAudioHardwareIllegalOperationError;
    }
    switch (objectID) {
        case kObjectID_PlugIn:
            return PlugInProperty(address, inQualifierDataSize, inQualifierData, inDataSize, outDataSize, outData);
        case kObjectID_Device:
            return DeviceProperty(address, inDataSize, outDataSize, outData);
        case kObjectID_InputStream:
        case kObjectID_OutputStream:
            return StreamProperty(objectID, address, inDataSize, outDataSize, outData);
        default:
            return kAudioHardwareBadObjectError;
    }
}

static Boolean HasProperty(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                           const AudioObjectPropertyAddress* inAddress) {
    (void)inClientProcessID;
    if (inDriver != gDriverRef) {
        return false;
    }
    UInt32 size = 0;
    return PropertyData(inObjectID, inAddress, 0, NULL, 0, &size, NULL) == noErr;
}

static OSStatus IsPropertySettable(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                                   const AudioObjectPropertyAddress* inAddress, Boolean* outIsSettable) {
    (void)inClientProcessID;
    if (inDriver != gDriverRef) {
        return kAudioHardwareBadObjectError;
    }
    if (inAddress == NULL || outIsSettable == NULL) {
        return kAudioHardwareIllegalOperationError;
    }
    UInt32 size = 0;
    OSStatus status = PropertyData(inObjectID, inAddress, 0, NULL, 0, &size, NULL);
    if (status != noErr) {
        return status;
    }
    switch (inAddress->mSelector) {
        case kAudioDevicePropertyNominalSampleRate:
            *outIsSettable = inObjectID == kObjectID_Device;
            break;
        case kAudioStreamPropertyIsActive:
        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyPhysicalFormat:
            *outIsSettable = inObjectID == kObjectID_InputStream || inObjectID == kObjectID_OutputStream;
            break;
        default:
            *outIsSettable = false;
    }
    return noErr;
}

static OSStatus GetPropertyDataSize(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                                    const AudioObjectPropertyAddress* inAddress, UInt32 inQualifierDataSize,
                                    const void* inQualifierData, UInt32* outDataSize) {
    (void)inClientProcessID;
    if (inDriver != gDriverRef) {
        return kAudioHardwareBadObjectError;
    }
    return PropertyData(inObjectID, inAddress, inQualifierDataSize, inQualifierData, 0, outDataSize, NULL);
}

static OSStatus GetPropertyData(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                                const AudioObjectPropertyAddress* inAddress, UInt32 inQualifierDataSize,
                                const void* inQualifierData, UInt32 inDataSize, UInt32* outDataSize, void* outData) {
    (void)inClientProcessID;
    if (inDriver != gDriverRef) {
        return kAudioHardwareBadObjectError;
    }
    if (outData == NULL) {
        return kAudioHardwareIllegalOperationError;
    }
    return PropertyData(inObjectID, inAddress, inQualifierDataSize, inQualifierData, inDataSize, outDataSize, outData);
}

static OSStatus SetPropertyData(AudioServerPlugInDriverRef inDriver, AudioObjectID inObjectID, pid_t inClientProcessID,
                                const AudioObjectPropertyAddress* inAddress, UInt32 inQualifierDataSize,
                                const void* inQualifierData, UInt32 inDataSize, const void* inData) {
    (void)inClientProcessID, (void)inQualifierDataSize, (void)inQualifierData;
    if (inDriver != gDriverRef) {
        return kAudioHardwareBadObjectError;
    }
    if (inAddress == NULL || inData == NULL) {
        return kAudioHardwareIllegalOperationError;
    }
    Boolean isStream = inObjectID == kObjectID_InputStream || inObjectID == kObjectID_OutputStream;
    switch (inAddress->mSelector) {
        case kAudioDevicePropertyNominalSampleRate:
            if (inObjectID != kObjectID_Device) {
                return kAudioHardwareUnknownPropertyError;
            }
            if (inDataSize != sizeof(Float64)) {
                return kAudioHardwareBadPropertySizeError;
            }
            // The device runs at a single rate; apps and SoundPush resample.
            return *(const Float64*)inData == kSampleRate ? noErr : kAudioHardwareIllegalOperationError;
        case kAudioStreamPropertyIsActive: {
            if (!isStream) {
                return kAudioHardwareUnknownPropertyError;
            }
            if (inDataSize != sizeof(UInt32)) {
                return kAudioHardwareBadPropertySizeError;
            }
            Boolean active = *(const UInt32*)inData != 0;
            pthread_mutex_lock(&gStateMutex);
            Boolean* flag = inObjectID == kObjectID_InputStream ? &gInputStreamActive : &gOutputStreamActive;
            Boolean changed = *flag != active;
            *flag = active;
            pthread_mutex_unlock(&gStateMutex);
            if (changed && gHost != NULL) {
                AudioObjectPropertyAddress changedAddress = {kAudioStreamPropertyIsActive, kAudioObjectPropertyScopeGlobal,
                                                             kAudioObjectPropertyElementMain};
                gHost->PropertiesChanged(gHost, inObjectID, 1, &changedAddress);
            }
            return noErr;
        }
        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyPhysicalFormat:
            if (!isStream) {
                return kAudioHardwareUnknownPropertyError;
            }
            if (inDataSize != sizeof(AudioStreamBasicDescription)) {
                return kAudioHardwareBadPropertySizeError;
            }
            return IsSupportedFormat((const AudioStreamBasicDescription*)inData) ? noErr
                                                                                  : kAudioDeviceUnsupportedFormatError;
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

// ------------------------------------------------------------------ IO

static void NotifyRunningChanged(void) {
    if (gHost != NULL) {
        AudioObjectPropertyAddress address = {kAudioDevicePropertyDeviceIsRunning, kAudioObjectPropertyScopeGlobal,
                                              kAudioObjectPropertyElementMain};
        gHost->PropertiesChanged(gHost, kObjectID_Device, 1, &address);
    }
}

static OSStatus Device_StartIO(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID, UInt32 inClientID) {
    (void)inClientID;
    if (inDriver != gDriverRef || inDeviceObjectID != kObjectID_Device) {
        return kAudioHardwareBadObjectError;
    }
    pthread_mutex_lock(&gStateMutex);
    Boolean started = gIOClients == 0;
    if (started) {
        // IO is not running yet, so the IO-thread state can be reset safely here.
        gAnchorHostTime = mach_absolute_time();
        memset(gRing, 0, sizeof(gRing));
        gWriteEnd = -1;
    }
    gIOClients++;
    pthread_mutex_unlock(&gStateMutex);
    if (started) {
        NotifyRunningChanged();
    }
    return noErr;
}

static OSStatus Device_StopIO(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID, UInt32 inClientID) {
    (void)inClientID;
    if (inDriver != gDriverRef || inDeviceObjectID != kObjectID_Device) {
        return kAudioHardwareBadObjectError;
    }
    pthread_mutex_lock(&gStateMutex);
    if (gIOClients == 0) {
        pthread_mutex_unlock(&gStateMutex);
        return kAudioHardwareIllegalOperationError;
    }
    gIOClients--;
    Boolean stopped = gIOClients == 0;
    pthread_mutex_unlock(&gStateMutex);
    if (stopped) {
        NotifyRunningChanged();
    }
    return noErr;
}

static OSStatus Device_GetZeroTimeStamp(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                        UInt32 inClientID, Float64* outSampleTime, UInt64* outHostTime, UInt64* outSeed) {
    (void)inClientID;
    if (inDriver != gDriverRef || inDeviceObjectID != kObjectID_Device) {
        return kAudioHardwareBadObjectError;
    }
    // A free-running clock: one timestamp per ring period since IO started.
    Float64 ticksPerPeriod = gHostTicksPerFrame * kRingFrames;
    UInt64 elapsed = mach_absolute_time() - gAnchorHostTime;
    UInt64 periods = (UInt64)((Float64)elapsed / ticksPerPeriod);
    *outSampleTime = (Float64)(periods * kRingFrames);
    *outHostTime = gAnchorHostTime + (UInt64)((Float64)periods * ticksPerPeriod);
    *outSeed = 1;
    return noErr;
}

static OSStatus Device_WillDoIOOperation(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                         UInt32 inClientID, UInt32 inOperationID, Boolean* outWillDo,
                                         Boolean* outWillDoInPlace) {
    (void)inClientID;
    if (inDriver != gDriverRef || inDeviceObjectID != kObjectID_Device) {
        return kAudioHardwareBadObjectError;
    }
    Boolean willDo = inOperationID == kAudioServerPlugInIOOperationReadInput ||
                     inOperationID == kAudioServerPlugInIOOperationWriteMix;
    if (outWillDo != NULL) {
        *outWillDo = willDo;
    }
    if (outWillDoInPlace != NULL) {
        *outWillDoInPlace = true;
    }
    return noErr;
}

static OSStatus Device_BeginIOOperation(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                        UInt32 inClientID, UInt32 inOperationID, UInt32 inIOBufferFrameSize,
                                        const AudioServerPlugInIOCycleInfo* inIOCycleInfo) {
    (void)inClientID, (void)inOperationID, (void)inIOBufferFrameSize, (void)inIOCycleInfo;
    return (inDriver == gDriverRef && inDeviceObjectID == kObjectID_Device) ? noErr : kAudioHardwareBadObjectError;
}

static inline UInt32 RingIndex(SInt64 frame) {
    SInt64 index = frame % kRingFrames;
    return (UInt32)(index < 0 ? index + kRingFrames : index);
}

static OSStatus Device_DoIOOperation(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                     AudioObjectID inStreamObjectID, UInt32 inClientID, UInt32 inOperationID,
                                     UInt32 inIOBufferFrameSize, const AudioServerPlugInIOCycleInfo* inIOCycleInfo,
                                     void* ioMainBuffer, void* ioSecondaryBuffer) {
    (void)inClientID, (void)ioSecondaryBuffer;
    if (inDriver != gDriverRef || inDeviceObjectID != kObjectID_Device) {
        return kAudioHardwareBadObjectError;
    }
    if (ioMainBuffer == NULL || inIOCycleInfo == NULL || inIOBufferFrameSize > kRingFrames) {
        return noErr;
    }
    Float32* buffer = (Float32*)ioMainBuffer;
    UInt32 frames = inIOBufferFrameSize;

    if (inOperationID == kAudioServerPlugInIOOperationWriteMix && inStreamObjectID == kObjectID_OutputStream) {
        SInt64 start = (SInt64)inIOCycleInfo->mOutputTime.mSampleTime;
        for (UInt32 i = 0; i < frames; i++) {
            UInt32 index = RingIndex(start + i) * kChannels;
            gRing[index] = buffer[i * kChannels];
            gRing[index + 1] = buffer[i * kChannels + 1];
        }
        gWriteEnd = (Float64)(start + frames);
    } else if (inOperationID == kAudioServerPlugInIOOperationReadInput && inStreamObjectID == kObjectID_InputStream) {
        SInt64 start = (SInt64)inIOCycleInfo->mInputTime.mSampleTime;
        SInt64 writeEnd = (SInt64)gWriteEnd;
        for (UInt32 i = 0; i < frames; i++) {
            SInt64 frame = start + i;
            // Only frames written within the last ring period are valid; anything else is silence.
            Boolean valid = gWriteEnd >= 0 && frame < writeEnd && frame >= writeEnd - kRingFrames;
            UInt32 index = RingIndex(frame) * kChannels;
            buffer[i * kChannels] = valid ? gRing[index] : 0.0f;
            buffer[i * kChannels + 1] = valid ? gRing[index + 1] : 0.0f;
        }
    }
    return noErr;
}

static OSStatus Device_EndIOOperation(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID,
                                      UInt32 inClientID, UInt32 inOperationID, UInt32 inIOBufferFrameSize,
                                      const AudioServerPlugInIOCycleInfo* inIOCycleInfo) {
    (void)inClientID, (void)inOperationID, (void)inIOBufferFrameSize, (void)inIOCycleInfo;
    return (inDriver == gDriverRef && inDeviceObjectID == kObjectID_Device) ? noErr : kAudioHardwareBadObjectError;
}
