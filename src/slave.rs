use std::sync::{Arc,Mutex};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use rppal::gpio::{Gpio, Trigger};
use alsa::{Direction, ValueOr};
use alsa::pcm::{PCM, HwParams, Format, Access, State};
use alsa::direct::pcm::SyncPtrStatus;
use num::pow;

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
    let mut pin = gpio.get(4).unwrap().into_input_pullup();

    let pcm = PCM::new(&args.flag_device, Direction::Playback, false).unwrap();
    let int_times = Arc::new(Mutex::new(std::vec::Vec::new()));

    let (tx, rx) = mpsc::channel();

    let _child = {
        let int_times = int_times.clone();
        let pcm_fd = pcm_to_fd(&pcm).unwrap();
        pin.set_interrupt(Trigger::RisingEdge).unwrap();
        thread::spawn(move || {
            let mut int_times = int_times.lock().unwrap();
            loop {
                match pin.poll_interrupt(true, Some(std::time::Duration::from_secs(1))) {
                    Ok(None) => {}
                    Ok(_) => {
                        match unsafe { SyncPtrStatus::sync_ptr(pcm_fd, true, None, None) } {
                            Ok(status) => {
                                int_times.push(status.htstamp().tv_sec as i64 * pow::pow(10i64,9) + status.htstamp().tv_nsec as i64);
                            }
                            Err(e) => println!("Error syncing pointer: {:?}", e),
                        }
                    }
                    Err(_) => panic!("Error polling interrupt!")
                }
                match rx.try_recv() {
                    Ok(_) | Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {}
                }
            }
        });
    };
    // Set hardware parameters: 44100 Hz / Mono / 16 bit
    let hwp = HwParams::any(&pcm).unwrap();
    hwp.set_channels(2).unwrap();
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
    for _ in 0..10*44100/512 {
        assert_eq!(io.writei(&buf[..]).unwrap(), 512);
    }

    // In case the buffer was larger than 2 seconds, start the stream manually.
    if pcm.state() != State::Running { pcm.start().unwrap() };
    // Wait for the stream to finish playback.
    pcm.drain().unwrap();

    tx.send(()).unwrap();
    let int_times = int_times.lock().unwrap();
    if int_times.len() > 1 {
        let int_times_sum : i64 = int_times[1..].iter()
            .zip(&int_times[..int_times.len()-1])
            .map(|(x, y)| x-y)
            .sum();
        let int_times_mean = int_times_sum/(int_times.len() as i64 - 1i64);

        println!("Received {} interrupts. The mean time difference is {}", int_times.len(), int_times_mean);
    } else if int_times.len() == 1 {
        println!("Received 1 interrupt.");
    } else {
        println!("Received no interrupts.");
    }
}
