use std::sync::mpsc::{self, TryRecvError, Receiver};
use std::thread;

use rppal::gpio::{Gpio, Trigger, InputPin};
use alsa::{Direction, ValueOr};
use alsa::pcm::{PCM, HwParams, Format, Access, State};
use alsa::direct::pcm::SyncPtrStatus;
use num::pow;
use ta::indicators::SimpleMovingAverage;
use ta::Next;

use super::Args;

fn pcm_to_fd(p: &PCM) -> alsa::Result<std::os::unix::io::RawFd> {
    let mut fds: [libc::pollfd; 1] = unsafe { std::mem::zeroed() };
    let c = (p as &alsa::PollDescriptors).fill(&mut fds)?;
    if c != 1 {
        return Err(alsa::Error::unsupported("snd_pcm_poll_descriptors returned wrong number of fds"))
    }
    Ok(fds[0].fd)
}

fn synch_status(pin: &mut InputPin, pcm_fd: &std::os::unix::io::RawFd, sma: &mut SimpleMovingAverage,
                int_time: u64, rx: &Receiver<()>)
{
    let mut prev_time: u64 = 0;
    loop {
        match pin.poll_interrupt(true, Some(std::time::Duration::from_nanos(15*int_time/10))) {
            Ok(None) => {
                prev_time = 0;
            }
            Ok(_) => {
                match unsafe { SyncPtrStatus::sync_ptr(*pcm_fd, true, None, None) } {
                    Ok(status) => {
                        let cur_time = status.htstamp().tv_sec as u64 * pow::pow(10u64,9) + status.htstamp().tv_nsec as u64;
                        if prev_time != 0 {
                            sma.next(int_time as f64 - (cur_time as f64 - prev_time as f64));
                        }
                        prev_time = cur_time;
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
}

pub fn main(args: Args) {
    let gpio: rppal::gpio::Gpio = Gpio::new().unwrap();
    let mut pin: InputPin = gpio.get(4).unwrap().into_input_pullup();

    let pcm = PCM::new(&args.flag_device, Direction::Playback, false).unwrap();
    let mut sma: SimpleMovingAverage = SimpleMovingAverage::new(100).unwrap();

    let (tx, rx) = mpsc::channel();
    pin.set_interrupt(Trigger::RisingEdge).unwrap();

    let fs = 44100;
    let num_channels : u32 = 2;
    let buf_size : usize = 1024;
    let chan_size : usize = buf_size/num_channels as usize;
    let int_time: u64 = 5 * pow(10, 6);

    let pcm_fd = pcm_to_fd(&pcm).unwrap();
    thread::spawn(move || {
        synch_status(&mut pin, &pcm_fd, &mut sma, int_time, &rx)
    });


    let hwp = HwParams::any(&pcm).unwrap();
    hwp.set_channels(num_channels).unwrap();
    hwp.set_rate(fs, ValueOr::Nearest).unwrap();
    hwp.set_format(Format::s16()).unwrap();
    hwp.set_access(Access::RWInterleaved).unwrap();
    pcm.hw_params(&hwp).unwrap();
    let io = pcm.io_i16().unwrap();

    let hwp = pcm.hw_params_current().unwrap();
    let swp = pcm.sw_params_current().unwrap();
    swp.set_start_threshold(hwp.get_buffer_size().unwrap() - hwp.get_period_size().unwrap()).unwrap();
    swp.set_tstamp_mode(true).unwrap();
    pcm.sw_params(&swp).unwrap();

    let mut buf = vec![0i16; buf_size];
    for (i, a) in buf.iter_mut().enumerate() {
        *a = ((i as f32 * 2.0 * std::f32::consts::PI / 128.0).sin() * 8192.0) as i16
    }

    // Play it back for 10 seconds.
    for _ in 0..10*fs/chan_size as u32 {
        assert_eq!(io.writei(&buf[..]).unwrap(), chan_size);
    }

    // In case the buffer was larger than 2 seconds, start the stream manually.
    if pcm.state() != State::Running { pcm.start().unwrap() };
    // Wait for the stream to finish playback.
    pcm.drain().unwrap();

    tx.send(()).unwrap();
}
