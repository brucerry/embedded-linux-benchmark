use crate::common::Deadline;
use crate::linux_poll;
use crate::peripheral::{self, Kind};
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::ptr;
use std::time::Instant;

const VIDEO_CAPTURE: u32 = 1;
const VIDEO_CAPTURE_MPLANE: u32 = 9;
const MMAP: u32 = 1;
const CAP_VIDEO_CAPTURE: u32 = 0x00000001;
const CAP_VIDEO_CAPTURE_MPLANE: u32 = 0x00001000;
const CAP_STREAMING: u32 = 0x04000000;
const CAP_DEVICE_CAPS: u32 = 0x80000000;
const CAP_M2M: u32 = 0x0000c000;
const BUF_ERROR: u32 = 0x0040;
const FIELD_ALTERNATE: u32 = 7;
const MAX_PLANES: usize = 8;
const WANT_BUFFERS: u32 = 4;
const MAX_BUFFERS: u32 = 8;
const MAX_FRAME_BYTES: u64 = 24 * 1024 * 1024;
const MAX_MAPPED_BYTES: u64 = MAX_FRAME_BYTES * WANT_BUFFERS as u64;

#[repr(C)]
#[derive(Default)]
struct Capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

#[repr(C)]
union FormatData {
    raw: [u8; 200],
    alignment: usize,
}

#[repr(C)]
struct Format {
    kind: u32,
    data: FormatData,
}

#[repr(C)]
struct Parameters {
    kind: u32,
    words: [u32; 50],
}

#[repr(C)]
#[derive(Default)]
struct RequestBuffers {
    count: u32,
    kind: u32,
    memory: u32,
    capabilities: u32,
    flags: u8,
    reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
union Location {
    offset: u32,
    planes: *mut Plane,
    value: usize,
}

impl Default for Location {
    fn default() -> Self {
        Self { value: 0 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Plane {
    bytes_used: u32,
    length: u32,
    memory: Location,
    data_offset: u32,
    reserved: [u32; 11],
}

#[repr(C)]
#[derive(Default)]
struct Buffer {
    index: u32,
    kind: u32,
    bytes_used: u32,
    flags: u32,
    field: u32,
    timestamp: [i64; 2],
    timecode: [u32; 4],
    sequence: u32,
    memory: u32,
    location: Location,
    length: u32,
    reserved2: u32,
    request_fd: i32,
}

const _: [(); 104] = [(); std::mem::size_of::<Capability>()];
const _: [(); 208] = [(); std::mem::size_of::<Format>()];
const _: [(); 204] = [(); std::mem::size_of::<Parameters>()];
const _: [(); 20] = [(); std::mem::size_of::<RequestBuffers>()];
const _: [(); 88] = [(); std::mem::size_of::<Buffer>()];
const _: [(); 64] = [(); std::mem::size_of::<Plane>()];

const QUERY_CAP: u32 = peripheral::ioctl_read::<Capability>(b'V', 0);
const GET_FORMAT: u32 = peripheral::ioctl_read_write::<Format>(b'V', 4);
const REQUEST_BUFFERS: u32 = peripheral::ioctl_read_write::<RequestBuffers>(b'V', 8);
const QUERY_BUFFER: u32 = peripheral::ioctl_read_write::<Buffer>(b'V', 9);
const QUEUE_BUFFER: u32 = peripheral::ioctl_read_write::<Buffer>(b'V', 15);
const DEQUEUE_BUFFER: u32 = peripheral::ioctl_read_write::<Buffer>(b'V', 17);
const STREAM_ON: u32 = peripheral::ioctl_write::<u32>(b'V', 18);
const STREAM_OFF: u32 = peripheral::ioctl_write::<u32>(b'V', 19);
const GET_PARAMETERS: u32 = peripheral::ioctl_read_write::<Parameters>(b'V', 21);

extern "C" {
    fn mmap(
        address: *mut c_void,
        length: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, length: usize) -> i32;
}

struct Mapping(*mut c_void, usize);

impl Mapping {
    fn map(device: &File, offset: u32, length: u32) -> Result<Self, String> {
        let length = length as usize;
        // PROT_READ | PROT_WRITE and MAP_SHARED as required by V4L2. No mapping is dereferenced.
        let address = unsafe {
            mmap(
                ptr::null_mut(),
                length,
                3,
                1,
                device.as_raw_fd(),
                offset.into(),
            )
        };
        if address as isize == -1 {
            return Err(format!(
                "cannot map capture buffer: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(Self(address, length))
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // This is the same successful mmap address and length.
        unsafe { munmap(self.0, self.1) };
    }
}

// Implementations must uphold the Linux UAPI pointer contract for each request.
trait Device {
    type Mapping;
    unsafe fn ioctl<T>(&self, request: u32, argument: &mut T) -> io::Result<usize>;
    fn map(&self, offset: u32, length: u32) -> Result<Self::Mapping, String>;
    fn wait_read(&self, deadline: Deadline) -> Result<(), String>;
}

impl Device for File {
    type Mapping = Mapping;

    unsafe fn ioctl<T>(&self, request: u32, argument: &mut T) -> io::Result<usize> {
        unsafe { peripheral::ioctl_ptr(self, request, argument) }
    }

    fn map(&self, offset: u32, length: u32) -> Result<Mapping, String> {
        Mapping::map(self, offset, length)
    }

    fn wait_read(&self, deadline: Deadline) -> Result<(), String> {
        linux_poll::wait(self.as_raw_fd(), linux_poll::READ, deadline)
    }
}

struct Settings {
    kind: u32,
    width: u32,
    height: u32,
    fourcc: u32,
    field: u32,
    planes: usize,
    interval: Option<(u32, u32)>,
}

fn settings(device: &impl Device) -> Result<Settings, String> {
    let mut capability = Capability::default();
    // Each argument below uses the verified V4L2 layout for its ioctl.
    unsafe { device.ioctl(QUERY_CAP, &mut capability) }
        .map_err(|error| format!("cannot query V4L2 capabilities: {error}"))?;
    let caps = if capability.capabilities & CAP_DEVICE_CAPS != 0 {
        capability.device_caps
    } else {
        capability.capabilities
    };
    if caps & CAP_STREAMING == 0 || caps & CAP_M2M != 0 {
        return Err("V4L2 node does not provide direct streaming capture".into());
    }
    let kind = if caps & CAP_VIDEO_CAPTURE_MPLANE != 0 {
        VIDEO_CAPTURE_MPLANE
    } else if caps & CAP_VIDEO_CAPTURE != 0 {
        VIDEO_CAPTURE
    } else {
        return Err("V4L2 node has no video capture queue".into());
    };
    let mut format = Format {
        kind,
        data: FormatData { raw: [0; 200] },
    };
    unsafe { device.ioctl(GET_FORMAT, &mut format) }
        .map_err(|error| format!("cannot read current V4L2 capture format: {error}"))?;
    let data = unsafe { format.data.raw };
    let word = |offset| u32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap());
    let planes = if kind == VIDEO_CAPTURE_MPLANE {
        usize::from(data[180])
    } else {
        1
    };
    if planes == 0 || planes > MAX_PLANES {
        return Err("V4L2 returned an invalid plane count".into());
    }
    let frame_bytes = if kind == VIDEO_CAPTURE_MPLANE {
        (0..planes)
            .map(|index| u64::from(word(20 + 20 * index)))
            .sum()
    } else {
        u64::from(word(20))
    };
    if !(1..=MAX_FRAME_BYTES).contains(&frame_bytes) || word(0) == 0 || word(4) == 0 {
        return Err(
            "V4L2 current format has invalid dimensions or exceeds the 24 MiB frame cap".into(),
        );
    }

    let mut parameters = Parameters {
        kind,
        words: [0; 50],
    };
    let interval = unsafe { device.ioctl(GET_PARAMETERS, &mut parameters) }
        .ok()
        .filter(|_| {
            parameters.words[0] & 0x1000 != 0
                && parameters.words[2] != 0
                && parameters.words[3] != 0
        })
        .map(|_| (parameters.words[2], parameters.words[3]));
    Ok(Settings {
        kind,
        width: word(0),
        height: word(4),
        fourcc: word(8),
        field: word(12),
        planes,
        interval,
    })
}

impl Settings {
    fn buffer(&self, index: u32, planes: &mut [Plane; MAX_PLANES]) -> Buffer {
        let mut buffer = Buffer {
            index,
            kind: self.kind,
            memory: MMAP,
            ..Buffer::default()
        };
        if self.kind == VIDEO_CAPTURE_MPLANE {
            buffer.location = Location {
                planes: planes.as_mut_ptr(),
            };
            buffer.length = self.planes as u32;
        }
        buffer
    }
}

struct Stream<'a, D: Device> {
    device: &'a D,
    settings: &'a Settings,
    count: u32,
    streaming: bool,
    mappings: Vec<D::Mapping>,
}

impl<'a, D: Device> Stream<'a, D> {
    fn new(device: &'a D, settings: &'a Settings, deadline: Deadline) -> Result<Self, String> {
        deadline.check()?;
        let mut request = RequestBuffers {
            count: WANT_BUFFERS,
            kind: settings.kind,
            memory: MMAP,
            ..RequestBuffers::default()
        };
        unsafe { device.ioctl(REQUEST_BUFFERS, &mut request) }
            .map_err(|error| format!("cannot allocate V4L2 MMAP capture buffers: {error}"))?;
        let mut stream = Self {
            device,
            settings,
            count: request.count,
            streaming: false,
            mappings: Vec::new(),
        };
        if !(2..=MAX_BUFFERS).contains(&request.count) {
            return Err(format!(
                "V4L2 returned {} buffers outside the allowed 2–8",
                request.count
            ));
        }
        let mut total = 0;
        for index in 0..request.count {
            deadline.check()?;
            let mut planes = [Plane::default(); MAX_PLANES];
            let mut buffer = settings.buffer(index, &mut planes);
            unsafe { device.ioctl(QUERY_BUFFER, &mut buffer) }
                .map_err(|error| format!("cannot query capture buffer {index}: {error}"))?;
            if settings.kind == VIDEO_CAPTURE_MPLANE && buffer.length as usize != settings.planes {
                return Err("V4L2 capture buffer plane count changed".into());
            }
            let mut frame_size = 0;
            for plane in &planes[..settings.planes] {
                let (offset, length) = if settings.kind == VIDEO_CAPTURE_MPLANE {
                    (unsafe { plane.memory.offset }, plane.length)
                } else {
                    (unsafe { buffer.location.offset }, buffer.length)
                };
                frame_size += u64::from(length);
                total += u64::from(length);
                if length == 0 || frame_size > MAX_FRAME_BYTES || total > MAX_MAPPED_BYTES {
                    return Err(
                        "V4L2 buffers exceed the 24 MiB frame or 96 MiB total mapping cap".into(),
                    );
                }
                stream.mappings.push(device.map(offset, length)?);
            }
            stream.queue(index)?;
        }
        deadline.check()?;
        let mut kind = settings.kind;
        unsafe { device.ioctl(STREAM_ON, &mut kind) }
            .map_err(|error| format!("cannot start V4L2 capture: {error}"))?;
        stream.streaming = true;
        Ok(stream)
    }

    fn queue(&self, index: u32) -> Result<(), String> {
        let mut planes = [Plane::default(); MAX_PLANES];
        let mut buffer = self.settings.buffer(index, &mut planes);
        // The plane array stays alive through QBUF.
        unsafe { self.device.ioctl(QUEUE_BUFFER, &mut buffer) }
            .map_err(|error| format!("cannot queue capture buffer {index}: {error}"))?;
        Ok(())
    }

    fn next(&self, deadline: Deadline) -> Result<Observation, String> {
        loop {
            deadline.check()?;
            let mut planes = [Plane::default(); MAX_PLANES];
            let mut buffer = self.settings.buffer(0, &mut planes);
            match unsafe { self.device.ioctl(DEQUEUE_BUFFER, &mut buffer) } {
                Ok(_) => {
                    if buffer.index >= self.count {
                        return Err("V4L2 returned an out-of-range buffer index".into());
                    }
                    let observation = Observation {
                        at: Instant::now(),
                        sequence: buffer.sequence,
                        error: buffer.flags & BUF_ERROR != 0,
                    };
                    self.queue(buffer.index)?;
                    return Ok(observation);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    self.device.wait_read(deadline)?
                }
                Err(error) => return Err(format!("cannot dequeue V4L2 buffer: {error}")),
            }
        }
    }

    fn stop(&mut self) -> Result<(), String> {
        if self.streaming {
            let mut kind = self.settings.kind;
            unsafe { self.device.ioctl(STREAM_OFF, &mut kind) }
                .map_err(|error| format!("cannot stop V4L2 capture: {error}"))?;
            self.streaming = false;
        }
        Ok(())
    }
}

impl<D: Device> Drop for Stream<'_, D> {
    fn drop(&mut self) {
        let _ = self.stop();
        self.mappings.clear();
        let mut request = RequestBuffers {
            count: 0,
            kind: self.settings.kind,
            memory: MMAP,
            ..RequestBuffers::default()
        };
        // This call frees buffers after their mappings have been released.
        let _ = unsafe { self.device.ioctl(REQUEST_BUFFERS, &mut request) };
    }
}

struct Observation {
    at: Instant,
    sequence: u32,
    error: bool,
}

#[derive(Default)]
struct Stats {
    count: u64,
    errors: u64,
    gaps: u64,
    repeated_or_reset: u64,
    last: Option<Observation>,
    mean: f64,
    variance_sum: f64,
}

impl Stats {
    fn add(&mut self, observation: Observation) {
        if let Some(last) = self.last.as_ref() {
            match observation.sequence.wrapping_sub(last.sequence) {
                1 => (),
                step @ 2..=0x7fff_ffff => self.gaps += u64::from(step - 1),
                _ => self.repeated_or_reset += 1,
            }
            let interval = observation
                .at
                .saturating_duration_since(last.at)
                .as_secs_f64();
            let delta = interval - self.mean;
            self.mean += delta / self.count as f64;
            self.variance_sum += delta * (interval - self.mean);
        }
        self.errors += u64::from(observation.error);
        self.count += 1;
        self.last = Some(observation);
    }

    fn jitter(&self) -> f64 {
        (self.variance_sum / (self.count - 1) as f64)
            .max(0.0)
            .sqrt()
    }
}

fn collect(
    frames: u64,
    mut frame: impl FnMut() -> Result<Observation, String>,
) -> Result<Stats, String> {
    let mut stats = Stats::default();
    for _ in 0..frames {
        stats.add(
            frame().map_err(|error| {
                format!("capture stopped after {} buffers: {error}", stats.count)
            })?,
        );
    }
    Ok(stats)
}

pub(crate) fn run(path: &Path, frames: u64, deadline: Deadline, hdmi: bool) -> Result<(), String> {
    deadline.check()?;
    let device = peripheral::open_device(path, Kind::Video, true)?;
    let settings = settings(&device)?;
    let mut stream = Stream::new(&device, &settings, deadline)?;
    let measured = collect(frames, || stream.next(deadline));
    let stop = stream.stop();
    let stats = measured?;
    stop?;
    deadline.check()?;
    let format: String = settings
        .fourcc
        .to_le_bytes()
        .into_iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || byte == b' ' {
                byte as char
            } else {
                '?'
            }
        })
        .collect();
    println!(
        "{} (V4L2 userspace dequeues)",
        if hdmi {
            "HDMI capture fixture"
        } else {
            "Camera capture"
        }
    );
    println!("Device: {}", path.display());
    println!(
        "Format: {}x{} {format:?}, {} plane(s), field mode {}",
        settings.width, settings.height, settings.planes, settings.field
    );
    if let Some((numerator, denominator)) = settings.interval {
        println!("Driver frame interval: {numerator}/{denominator} s");
    }
    println!("Capture buffers allocated: {}", stream.count);
    println!("Received buffers: {}", stats.count);
    println!("Buffers marked erroneous: {}", stats.errors);
    println!("Driver sequence gaps: {}", stats.gaps);
    println!("Repeated or reset sequences: {}", stats.repeated_or_reset);
    if stats.mean > 0.0 {
        let unit = if settings.field == FIELD_ALTERNATE {
            "fields/s"
        } else {
            "frames/s"
        };
        println!("Observed delivery rate: {:.2} {unit}", 1.0 / stats.mean);
    }
    println!("Inter-arrival mean: {:.3} ms", stats.mean * 1000.0);
    println!(
        "Inter-arrival jitter (standard deviation): {:.3} ms",
        stats.jitter() * 1000.0
    );
    if hdmi {
        println!("Delivery at capture only. Output-to-capture latency requires a synchronized stimulus and reference.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn captures_cadence_errors_wrap_and_gaps() {
        let start = Instant::now();
        let mut fixture = [
            (0, u32::MAX, false),
            (10, 0, false),
            (30, 3, true),
            (40, 3, false),
        ]
        .into_iter();
        let stats = collect(4, || {
            let (milliseconds, sequence, error) = fixture.next().unwrap();
            Ok(Observation {
                at: start + Duration::from_millis(milliseconds),
                sequence,
                error,
            })
        })
        .unwrap();
        assert_eq!(
            (
                stats.count,
                stats.errors,
                stats.gaps,
                stats.repeated_or_reset
            ),
            (4, 1, 2, 1)
        );
        assert!((stats.mean - 0.04 / 3.0).abs() < 1e-9);
        assert!((stats.jitter() - 0.0047140452).abs() < 1e-9);
        assert!(collect(4, || Err("fixture timeout".into()))
            .err()
            .unwrap()
            .contains("after 0 buffers"));
    }

    #[derive(Default)]
    struct FakeDevice {
        calls: std::cell::RefCell<Vec<u32>>,
        maps: std::cell::Cell<u32>,
        dequeues: std::cell::Cell<u32>,
        fail_dequeue: bool,
    }

    impl Device for FakeDevice {
        type Mapping = ();

        unsafe fn ioctl<T>(&self, request: u32, argument: &mut T) -> io::Result<usize> {
            self.calls.borrow_mut().push(request);
            let pointer = argument as *mut T;
            // Each fixture branch checks the ioctl code before using its argument layout.
            unsafe {
                match request {
                    QUERY_CAP => {
                        (*pointer.cast::<Capability>()).capabilities =
                            CAP_VIDEO_CAPTURE_MPLANE | CAP_STREAMING;
                    }
                    GET_FORMAT => {
                        let format = &mut *pointer.cast::<Format>();
                        assert_eq!(format.kind, VIDEO_CAPTURE_MPLANE);
                        let mut bytes = [0; 200];
                        for (offset, value) in [
                            (0, 640u32),
                            (4, 480),
                            (8, u32::from_le_bytes(*b"NV12")),
                            (20, 4096),
                            (40, 4096),
                        ] {
                            bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
                        }
                        bytes[180] = 2;
                        format.data = FormatData { raw: bytes };
                    }
                    GET_PARAMETERS => return Err(io::ErrorKind::Unsupported.into()),
                    REQUEST_BUFFERS => {
                        let request = &mut *pointer.cast::<RequestBuffers>();
                        assert_eq!(request.kind, VIDEO_CAPTURE_MPLANE);
                        assert_eq!(request.memory, MMAP);
                        assert!(matches!(request.count, 0 | WANT_BUFFERS));
                    }
                    QUERY_BUFFER => {
                        let buffer = &mut *pointer.cast::<Buffer>();
                        assert_eq!(buffer.length, 2);
                        let planes = buffer.location.planes;
                        for (index, offset) in [0u32, 4096].into_iter().enumerate() {
                            *planes.add(index) = Plane {
                                length: 4096,
                                memory: Location {
                                    offset: buffer.index * 8192 + offset,
                                },
                                ..Plane::default()
                            };
                        }
                    }
                    QUEUE_BUFFER => {
                        let buffer = &*pointer.cast::<Buffer>();
                        assert_eq!(buffer.length, 2);
                        assert_eq!(buffer.flags, 0);
                        assert!(buffer.index < WANT_BUFFERS);
                    }
                    STREAM_ON | STREAM_OFF => {
                        assert_eq!(*pointer.cast::<u32>(), VIDEO_CAPTURE_MPLANE)
                    }
                    DEQUEUE_BUFFER if self.fail_dequeue => {
                        return Err(io::ErrorKind::BrokenPipe.into())
                    }
                    DEQUEUE_BUFFER => {
                        let index = self.dequeues.get();
                        self.dequeues.set(index + 1);
                        let buffer = &mut *pointer.cast::<Buffer>();
                        buffer.index = index % WANT_BUFFERS;
                        buffer.sequence = index;
                        buffer.flags = if index == 1 { BUF_ERROR } else { 0 };
                    }
                    _ => panic!("unexpected V4L2 request {request:#x}"),
                }
            }
            Ok(0)
        }

        fn map(&self, offset: u32, length: u32) -> Result<(), String> {
            assert_eq!(length, 4096);
            assert_eq!(offset, self.maps.get() * length);
            self.maps.set(self.maps.get() + 1);
            Ok(())
        }

        fn wait_read(&self, _deadline: Deadline) -> Result<(), String> {
            panic!("fixture delivers buffers immediately")
        }
    }

    #[test]
    fn simulated_multiplane_stream_requeues_and_cleans_up_on_success_and_failure() {
        for fail_dequeue in [false, true] {
            let device = FakeDevice {
                fail_dequeue,
                ..FakeDevice::default()
            };
            let settings = settings(&device).unwrap();
            assert_eq!(
                (settings.width, settings.height, settings.planes),
                (640, 480, 2)
            );
            let deadline = Deadline::after(Duration::from_secs(3));
            {
                let stream = Stream::new(&device, &settings, deadline).unwrap();
                let result = collect(3, || stream.next(deadline));
                if fail_dequeue {
                    assert!(result.err().unwrap().contains("cannot dequeue"));
                } else {
                    let stats = result.ok().unwrap();
                    assert_eq!((stats.count, stats.errors, stats.gaps), (3, 1, 0));
                }
            }
            assert_eq!(device.maps.get(), 8);
            let calls = device.calls.borrow();
            assert!(calls.ends_with(&[STREAM_OFF, REQUEST_BUFFERS]));
            assert_eq!(
                calls.iter().filter(|&&call| call == QUEUE_BUFFER).count(),
                if fail_dequeue { 4 } else { 7 }
            );
        }
    }

    #[test]
    fn v4l2_codes_and_offsets_match_lp64() {
        assert_eq!(
            (
                QUERY_CAP,
                GET_FORMAT,
                REQUEST_BUFFERS,
                QUERY_BUFFER,
                QUEUE_BUFFER,
                DEQUEUE_BUFFER,
                STREAM_ON,
                STREAM_OFF,
                GET_PARAMETERS
            ),
            (
                0x80685600, 0xc0d05604, 0xc0145608, 0xc0585609, 0xc058560f, 0xc0585611, 0x40045612,
                0x40045613, 0xc0cc5615
            )
        );
        let buffer = Buffer::default();
        let base = &buffer as *const Buffer as usize;
        assert_eq!(&buffer.timestamp as *const [i64; 2] as usize - base, 24);
        assert_eq!(&buffer.sequence as *const u32 as usize - base, 56);
        assert_eq!(&buffer.location as *const Location as usize - base, 64);
    }
}
