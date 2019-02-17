use std::sync::{Arc, Mutex};

use rppal::gpio::{Gpio, Trigger};
use alsa::{Direction, ValueOr};
use alsa::pcm::{PCM, HwParams, Format, Access, State};
use alsa::direct::pcm::SyncPtrStatus;

use super::Args;

fn pcm_to_fd(p: &PCM) -> alsa::Result<std::os::unix::io::RawFd> {
    let mut fds: [libc::pollfd; 1] = unsafe { std::mem::zeroed() };
    let c = (p as &alsa::PollDescriptors).fill(&mut fds)?;
    if c != 1 {
        return Err(alsa::Error::unsupported("snd_pcm_poll_descriptors returned wrong number of fds"))
    }
    Ok(fds[0].fd)
}

pub fn main(args : Args) {
    let gpio = Gpio::new().unwrap();
    let mut pin = gpio.get(4).unwrap().into_input_pulldown();

    let pcm = PCM::new(&args.flag_device, Direction::Playback, false).unwrap();
    let pcm_fd = Arc::new(Mutex::new(pcm_to_fd(&pcm).unwrap()));

    let guard = {
        let pcm_fd = pcm_fd.clone();
        pin.set_async_interrupt(Trigger::RisingEdge, move |_level| {
            let status = unsafe {
                SyncPtrStatus::sync_ptr(*pcm_fd.lock().unwrap(), false, None, None).unwrap()
            };
            println!("Interrupted! Time: {}:{}", status.htstamp().tv_sec, status.htstamp().tv_nsec);
        }).unwrap();
    };

    // Set hardware parameters: 44100 Hz / Mono / 16 bit
    let hwp = HwParams::any(&pcm).unwrap();
    hwp.set_channels(1).unwrap();
    hwp.set_rate(44100, ValueOr::Nearest).unwrap();
    hwp.set_format(Format::s16()).unwrap();
    hwp.set_access(Access::RWInterleaved).unwrap();
    pcm.hw_params(&hwp).unwrap();
    let io = pcm.io_i16().unwrap();

    // Make sure we don't start the stream too early
    let hwp = pcm.hw_params_current().unwrap();
    let swp = pcm.sw_params_current().unwrap();
    swp.set_start_threshold(hwp.get_buffer_size().unwrap() - hwp.get_period_size().unwrap()).unwrap();
    swp.set_tstamp_mode(true).unwrap();
    pcm.sw_params(&swp).unwrap();

    // Make a sine wave
    let mut buf = [0i16; 1024];
    for (i, a) in buf.iter_mut().enumerate() {
        *a = ((i as f32 * 2.0 * ::std::f32::consts::PI / 128.0).sin() * 8192.0) as i16
    }

    // Play it back for 10 seconds.
    for _ in 0..10*44100/1024 {
        assert_eq!(io.writei(&buf[..]).unwrap(), 1024);
    }

    // In case the buffer was larger than 2 seconds, start the stream manually.
    if pcm.state() != State::Running { pcm.start().unwrap() };
    // Wait for the stream to finish playback.
    pcm.drain().unwrap();
    drop(guard);
}
