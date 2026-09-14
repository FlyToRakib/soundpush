//! macOS 13 system-audio capture with ScreenCaptureKit, for systems without Core Audio process
//! taps (macOS 14.2+, [`crate::macos_tap`]).
//!
//! An `SCStream` on the main display with `capturesAudio` delivers everything the computer plays
//! except SoundPush itself (`excludesCurrentProcessAudio`). ScreenCaptureKit always produces video
//! as well; it is limited to a 2×2 frame once a second and not received. Audio arrives as float PCM
//! sample buffers on ScreenCaptureKit's queue and is converted to the caller's channel count. The
//! stream needs the Screen & System Audio Recording permission; without it opening fails with
//! [`AudioError::PermissionDenied`] (macOS shows its prompt the first time).
//!
//! Apple documentation:
//! - <https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturesaudio>
//! - <https://developer.apple.com/documentation/screencapturekit/scstreamoutput>
//! - <https://developer.apple.com/documentation/coremedia/cmsamplebuffergetaudiobufferlistwithretainedblockbuffer(_:bufferlistsizeneededout:bufferlistout:bufferlistsize:blockbufferallocator:blockbuffermemoryallocator:flags:blockbufferout:)>

use std::ptr::NonNull;
use std::sync::{Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, Message, define_class, msg_send};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMAudioFormatDescriptionGetStreamBasicDescription, CMBlockBuffer, CMSampleBuffer, CMTime,
    CMTimeFlags,
};
use objc2_foundation::{NSArray, NSError, NSOperatingSystemVersion, NSProcessInfo};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCStreamOutput, SCStreamOutputType,
};
use tracing::{info, warn};

use crate::{
    AudioError, AudioStream, CaptureCallback, CaptureConverter, ErrorCallback, StreamInfo,
};

const RATE: u32 = 48_000;
/// How long ScreenCaptureKit may take to answer (its permission prompt included).
const TIMEOUT: Duration = Duration::from_secs(10);
/// `SCStreamErrorUserDeclined`: the recording permission is missing.
const USER_DECLINED: isize = -3801;
const FORMAT_LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
const FLAG_IS_FLOAT: u32 = 1;
const FLAG_NON_INTERLEAVED: u32 = 1 << 5;
/// `kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment`.
const ASSURE_16_BYTE_ALIGNMENT: u32 = 1;

/// Whether Core Audio process taps exist (macOS 14.2+). Before that, system audio comes from
/// ScreenCaptureKit, and the tap functions must not be called (they are weakly linked).
pub fn process_taps_supported() -> bool {
    let version = NSOperatingSystemVersion {
        majorVersion: 14,
        minorVersion: 2,
        patchVersion: 0,
    };
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(version)
}

fn error(e: &NSError) -> AudioError {
    if e.code() == USER_DECLINED {
        AudioError::PermissionDenied
    } else {
        AudioError::Backend(format!("ScreenCaptureKit: {}", e.localizedDescription()))
    }
}

/// Converts ScreenCaptureKit's sample buffers and hands them to the capture callback.
struct Receiver {
    on_audio: CaptureCallback,
    channels: u16,
    /// Rate and channels the converter was made for.
    converter: Option<(u32, u16, CaptureConverter)>,
    /// `AudioBufferList` storage, 8-byte aligned.
    list: Vec<u64>,
    frames: Vec<f32>,
}

impl Receiver {
    fn receive(&mut self, buffer: &CMSampleBuffer) {
        // SAFETY: reading the format of a valid sample buffer.
        let Some(format) = (unsafe { buffer.format_description() }) else {
            return;
        };
        // SAFETY: the description belongs to `format`, which lives until the end of this call.
        let Some(asbd) =
            (unsafe { CMAudioFormatDescriptionGetStreamBasicDescription(&format).as_ref() })
        else {
            return;
        };
        let (rate, channels) = (asbd.mSampleRate as u32, asbd.mChannelsPerFrame as u16);
        if asbd.mFormatID != FORMAT_LINEAR_PCM
            || asbd.mFormatFlags & FLAG_IS_FLOAT == 0
            || asbd.mBitsPerChannel != 32
            || rate == 0
            || channels == 0
        {
            return;
        }
        let mut needed = 0usize;
        // SAFETY: a size query; no list is written.
        let status = unsafe {
            buffer.audio_buffer_list_with_retained_block_buffer(
                &mut needed,
                std::ptr::null_mut(),
                0,
                None,
                None,
                ASSURE_16_BYTE_ALIGNMENT,
                std::ptr::null_mut(),
            )
        };
        if status != 0 || needed == 0 {
            return;
        }
        self.list.resize(needed.div_ceil(8), 0);
        let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
        // SAFETY: `list` has room for `needed` bytes with 8-byte alignment. The retained block
        // buffer owns the audio memory and is released when `_block` drops.
        let status = unsafe {
            buffer.audio_buffer_list_with_retained_block_buffer(
                std::ptr::null_mut(),
                self.list.as_mut_ptr().cast(),
                needed,
                None,
                None,
                ASSURE_16_BYTE_ALIGNMENT,
                &mut block,
            )
        };
        // SAFETY: a block buffer returned by a "Retained" function is owned by the caller.
        let _block = NonNull::new(block).map(|b| unsafe { CFRetained::from_raw(b) });
        if status != 0 {
            return;
        }
        // SAFETY: Core Media filled the list: a header followed by `mNumberBuffers` buffers.
        let buffers: &[AudioBuffer] = unsafe {
            let list = &*self.list.as_ptr().cast::<AudioBufferList>();
            std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize)
        };
        if buffers.iter().any(|b| b.mData.is_null()) {
            return;
        }
        self.frames.clear();
        if asbd.mFormatFlags & FLAG_NON_INTERLEAVED != 0 {
            // One buffer per channel: interleave.
            let count = buffers
                .iter()
                .map(|b| b.mDataByteSize as usize / 4)
                .min()
                .unwrap_or(0);
            for frame in 0..count {
                for b in buffers {
                    // SAFETY: `frame` is below the sample count of every buffer.
                    self.frames
                        .push(unsafe { *b.mData.cast::<f32>().add(frame) });
                }
            }
        } else if let Some(b) = buffers.first() {
            // SAFETY: an interleaved buffer of `mDataByteSize` bytes of f32 samples.
            self.frames.extend_from_slice(unsafe {
                std::slice::from_raw_parts(b.mData.cast::<f32>(), b.mDataByteSize as usize / 4)
            });
        }
        if !matches!(&self.converter, Some((r, c, _)) if *r == rate && *c == channels) {
            self.converter = Some((
                rate,
                channels,
                CaptureConverter::new(rate, channels, self.channels),
            ));
        }
        if let Some((_, _, converter)) = &mut self.converter {
            (self.on_audio)(converter.process(&self.frames));
        }
    }
}

struct Ivars {
    receiver: Mutex<Receiver>,
    on_error: Mutex<ErrorCallback>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `AudioOutput` does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "SoundPushScreenCaptureAudio"]
    #[ivars = Ivars]
    struct AudioOutput;

    unsafe impl NSObjectProtocol for AudioOutput {}

    unsafe impl SCStreamOutput for AudioOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn stream_did_output(
            &self,
            _stream: &SCStream,
            sample_buffer: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind == SCStreamOutputType::Audio
                && let Ok(mut receiver) = self.ivars().receiver.lock()
            {
                receiver.receive(sample_buffer);
            }
        }
    }

    unsafe impl SCStreamDelegate for AudioOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn stream_did_stop(&self, _stream: &SCStream, e: &NSError) {
            warn!(code = e.code(), "ScreenCaptureKit stopped the stream");
            if let Ok(mut on_error) = self.ivars().on_error.lock() {
                on_error(AudioError::DeviceLost);
            }
        }
    }
);

impl AudioOutput {
    fn new(receiver: Receiver, on_error: ErrorCallback) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars {
            receiver: Mutex::new(receiver),
            on_error: Mutex::new(on_error),
        });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// An object from a completion handler, moved to the thread waiting for it.
struct Handoff<T>(T);
// SAFETY: ScreenCaptureKit's shareable content is an immutable snapshot, usable from any thread.
unsafe impl<T> Send for Handoff<T> {}

fn wait<T>(rx: &mpsc::Receiver<T>, what: &str) -> Result<T, AudioError> {
    rx.recv_timeout(TIMEOUT)
        .map_err(|_| AudioError::Backend(format!("ScreenCaptureKit did not {what} in time")))
}

fn start(
    channels: u16,
    on_audio: CaptureCallback,
    on_error: ErrorCallback,
) -> Result<(Retained<SCStream>, Retained<AudioOutput>), AudioError> {
    let (tx, rx) = mpsc::channel();
    let listed = RcBlock::new(move |content: *mut SCShareableContent, e: *mut NSError| {
        // SAFETY: ScreenCaptureKit passes valid objects or nil.
        let result = match unsafe { (content.as_ref(), e.as_ref()) } {
            (Some(content), _) => Ok(Handoff(content.retain())),
            (None, Some(e)) => Err(error(e)),
            (None, None) => Err(AudioError::NoDefaultDevice),
        };
        let _ = tx.send(result);
    });
    // SAFETY: the block has the documented completion handler signature.
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, true, &listed,
        );
    }
    let content = wait(&rx, "list the displays")??.0;
    // SAFETY: plain getters and initializers on live objects.
    let (stream, output) = unsafe {
        let display = content
            .displays()
            .firstObject()
            .ok_or(AudioError::NoDefaultDevice)?;
        let filter = SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            &display,
            &NSArray::new(),
        );
        let config = SCStreamConfiguration::new();
        config.setCapturesAudio(true);
        config.setExcludesCurrentProcessAudio(true);
        config.setSampleRate(RATE as isize);
        config.setChannelCount(2);
        // Video cannot be switched off: keep it as small and rare as possible.
        config.setWidth(2);
        config.setHeight(2);
        config.setMinimumFrameInterval(CMTime {
            value: 1,
            timescale: 1,
            flags: CMTimeFlags::Valid,
            epoch: 0,
        });
        let output = AudioOutput::new(
            Receiver {
                on_audio,
                channels,
                converter: None,
                list: Vec::new(),
                frames: Vec::new(),
            },
            on_error,
        );
        let stream = SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &config,
            Some(ProtocolObject::from_ref(&*output)),
        );
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(
                ProtocolObject::from_ref(&*output),
                SCStreamOutputType::Audio,
                None,
            )
            .map_err(|e| error(&e))?;
        (stream, output)
    };
    let (tx, rx) = mpsc::channel();
    let started = RcBlock::new(move |e: *mut NSError| {
        // SAFETY: nil on success, otherwise a valid error.
        let _ = tx.send(unsafe { e.as_ref() }.map(error));
    });
    // SAFETY: the block has the documented completion handler signature.
    unsafe { stream.startCaptureWithCompletionHandler(Some(&started)) };
    if let Some(e) = wait(&rx, "start")? {
        return Err(e);
    }
    Ok((stream, output))
}

fn stop(stream: &SCStream) {
    let (tx, rx) = mpsc::channel();
    let stopped = RcBlock::new(move |_: *mut NSError| {
        let _ = tx.send(());
    });
    // SAFETY: the block has the documented completion handler signature.
    unsafe { stream.stopCaptureWithCompletionHandler(Some(&stopped)) };
    if wait(&rx, "stop").is_err() {
        warn!("ScreenCaptureKit did not stop in time");
    }
}

/// The stream lives on its own thread, which stops it when the handle is dropped.
struct ScreenCaptureStream {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    info: StreamInfo,
}

impl AudioStream for ScreenCaptureStream {
    fn info(&self) -> StreamInfo {
        self.info
    }
}

impl Drop for ScreenCaptureStream {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Record everything the computer plays except SoundPush, as 48 kHz frames of `channels`.
pub fn open(
    channels: u16,
    on_audio: CaptureCallback,
    on_error: ErrorCallback,
) -> Result<Box<dyn AudioStream>, AudioError> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("sp-audio-screencapture".into())
        .spawn(move || {
            match autoreleasepool(|_| start(channels, on_audio, on_error)) {
                Ok((stream, _output)) => {
                    let _ = ready_tx.send(Ok(()));
                    // Blocks until the handle is dropped.
                    let _ = stop_rx.recv();
                    autoreleasepool(|_| stop(&stream));
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })
        .map_err(|e| AudioError::Backend(e.to_string()))?;
    // A prompt or a stuck service must not hang the caller; a late start sees the closed channel
    // and stops again.
    match ready_rx.recv_timeout(TIMEOUT * 2) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            return Err(AudioError::Backend(
                "ScreenCaptureKit did not start in time".into(),
            ));
        }
    }
    info!(channels, "opening system audio capture (ScreenCaptureKit)");
    Ok(Box::new(ScreenCaptureStream {
        stop: Some(stop_tx),
        thread: Some(thread),
        info: StreamInfo {
            device_sample_rate: RATE,
            device_channels: 2,
            channels,
            latency_ms: 20,
        },
    }))
}
