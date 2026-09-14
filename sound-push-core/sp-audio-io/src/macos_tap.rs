//! macOS system-audio capture with Core Audio process taps (macOS 14.2+).
//!
//! A global stereo tap of every process except SoundPush is wrapped in a temporary
//! aggregate device. cpal then opens that aggregate like any other input device.
//! Both objects are destroyed when [`SystemTap`] is dropped (and by the OS if the
//! process exits). The first capture shows the system "record system audio" prompt.

use std::ffi::{CStr, c_void};
use std::ptr::NonNull;

use objc2::AnyThread;
use objc2::Message;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_audio::{
    AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap,
    AudioObjectGetPropertyData, AudioObjectPropertyAddress, CATapDescription, CATapMuteBehavior,
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceMainSubDeviceKey,
    kAudioAggregateDeviceNameKey, kAudioAggregateDeviceSubDeviceListKey,
    kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey,
    kAudioAggregateDeviceUIDKey, kAudioDevicePropertyDeviceUID,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyTranslatePIDToProcessObject,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    kAudioSubDeviceUIDKey, kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey,
};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

/// Name of the temporary input device cpal opens.
pub const TAP_DEVICE_NAME: &str = "SoundPush System Audio";

pub struct SystemTap {
    tap_id: u32,
    aggregate_id: u32,
}

fn read_property<T: Copy>(object: u32, selector: u32, qualifier: Option<&u32>) -> Result<T, i32> {
    let address = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut size = std::mem::size_of::<T>() as u32;
    let mut value = std::mem::MaybeUninit::<T>::zeroed();
    let (qualifier_size, qualifier_ptr) = match qualifier {
        Some(q) => (4, std::ptr::from_ref(q).cast::<c_void>()),
        None => (0, std::ptr::null()),
    };
    // SAFETY: all pointers are valid for the duration of the call and `size` matches `T`.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            qualifier_size,
            qualifier_ptr,
            NonNull::from(&mut size),
            NonNull::new_unchecked(value.as_mut_ptr().cast()),
        )
    };
    if status == 0 {
        // SAFETY: Core Audio filled the value on success.
        Ok(unsafe { value.assume_init() })
    } else {
        Err(status)
    }
}

fn ns_key(key: &CStr) -> Retained<NSString> {
    NSString::from_str(key.to_str().unwrap_or_default())
}

fn as_object<T: Message>(value: &T) -> &AnyObject {
    // SAFETY: every Objective-C object type is layout-compatible with AnyObject.
    unsafe { &*std::ptr::from_ref(value).cast::<AnyObject>() }
}

impl SystemTap {
    pub fn create() -> Result<Self, String> {
        // Exclude our own process so received audio is never captured back (feedback loop).
        let pid = std::process::id();
        let own_process: u32 = read_property(
            kAudioObjectSystemObject as u32,
            kAudioHardwarePropertyTranslatePIDToProcessObject,
            Some(&pid),
        )
        .unwrap_or(0);
        let excluded: Vec<Retained<NSNumber>> = if own_process != 0 {
            vec![NSNumber::numberWithUnsignedInt(own_process)]
        } else {
            Vec::new()
        };
        let excluded = NSArray::from_retained_slice(&excluded);

        // SAFETY: standard alloc/init; the description is configured before use.
        let description = unsafe {
            CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &excluded,
            )
        };
        // SAFETY: plain property setters on a live object.
        unsafe {
            // Must match the aggregate's visibility: a private tap only attaches to a private aggregate.
            description.setPrivate(false);
            description.setMuteBehavior(CATapMuteBehavior::Unmuted);
            description.setName(&NSString::from_str(TAP_DEVICE_NAME));
        }

        let mut tap_id = 0u32;
        // SAFETY: `description` is valid and `tap_id` is a valid out pointer.
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap_id) };
        if status != 0 || tap_id == 0 {
            return Err(format!(
                "could not create system audio tap (status {status})"
            ));
        }
        let destroy_tap = |msg: String| {
            // SAFETY: tap_id came from AudioHardwareCreateProcessTap.
            unsafe { AudioHardwareDestroyProcessTap(tap_id) };
            msg
        };

        // SAFETY: UUID is always set on a tap description.
        let tap_uid = unsafe { description.UUID().UUIDString() };
        let output: u32 = read_property(
            kAudioObjectSystemObject as u32,
            kAudioHardwarePropertyDefaultOutputDevice,
            None,
        )
        .map_err(|s| destroy_tap(format!("no default output device (status {s})")))?;
        let output_uid_ptr: *mut NSString =
            read_property(output, kAudioDevicePropertyDeviceUID, None).map_err(|s| {
                destroy_tap(format!("could not read output device UID (status {s})"))
            })?;
        // SAFETY: the property returns a +1 retained CFString, toll-free bridged to NSString.
        let output_uid = unsafe { Retained::from_raw(output_uid_ptr) }
            .ok_or_else(|| destroy_tap("output device has no UID".into()))?;

        let yes = NSNumber::numberWithBool(true);
        let no = NSNumber::numberWithBool(false);
        let name = NSString::from_str(TAP_DEVICE_NAME);
        let aggregate_uid = NSString::from_str(&format!("net.soundpush.system-audio.{tap_uid}"));

        let (k_sub_uid, k_tap_uid, k_drift) = (
            ns_key(kAudioSubDeviceUIDKey),
            ns_key(kAudioSubTapUIDKey),
            ns_key(kAudioSubTapDriftCompensationKey),
        );
        let sub_device = NSDictionary::from_slices(&[&*k_sub_uid], &[as_object(&*output_uid)]);
        let sub_tap = NSDictionary::from_slices(
            &[&*k_tap_uid, &*k_drift],
            &[as_object(&*tap_uid), as_object(&*yes)],
        );
        let sub_devices = NSArray::from_retained_slice(&[sub_device]);
        let taps = NSArray::from_retained_slice(&[sub_tap]);

        let keys = [
            ns_key(kAudioAggregateDeviceNameKey),
            ns_key(kAudioAggregateDeviceUIDKey),
            ns_key(kAudioAggregateDeviceMainSubDeviceKey),
            ns_key(kAudioAggregateDeviceIsPrivateKey),
            ns_key(kAudioAggregateDeviceTapAutoStartKey),
            ns_key(kAudioAggregateDeviceSubDeviceListKey),
            ns_key(kAudioAggregateDeviceTapListKey),
        ];
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        // Not private: cpal only enumerates public devices. It exists only while streaming.
        let description = NSDictionary::from_slices(
            &key_refs,
            &[
                as_object(&*name),
                as_object(&*aggregate_uid),
                as_object(&*output_uid),
                as_object(&*no),
                as_object(&*yes),
                as_object(&*sub_devices),
                as_object(&*taps),
            ],
        );

        let mut aggregate_id = 0u32;
        // SAFETY: NSDictionary is toll-free bridged to CFDictionary; out pointer is valid.
        let status = unsafe {
            let cf = &*Retained::as_ptr(&description).cast::<CFDictionary>();
            AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut aggregate_id))
        };
        if status != 0 || aggregate_id == 0 {
            return Err(destroy_tap(format!(
                "could not create capture device (status {status})"
            )));
        }
        Ok(Self {
            tap_id,
            aggregate_id,
        })
    }
}

impl Drop for SystemTap {
    fn drop(&mut self) {
        // SAFETY: both ids were created by this struct and are destroyed exactly once.
        unsafe {
            AudioHardwareDestroyAggregateDevice(self.aggregate_id);
            AudioHardwareDestroyProcessTap(self.tap_id);
        }
    }
}
