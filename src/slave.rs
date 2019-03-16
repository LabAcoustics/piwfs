use std::sync::{Arc, Mutex, Barrier};
use std::sync::mpsc::{self, TryRecvError, Receiver};
use std::io::Write;
use std::thread;

use rppal::gpio::{Gpio, Trigger, InputPin};
use alsa::{Direction, ValueOr};
use alsa::pcm::{PCM, HwParams, Format, Access, State};
use alsa::direct::pcm::SyncPtrStatus;
use num::pow;
use ta::indicators::SimpleMovingAverage;
use ta::Next;
use hound;

use super::Args;

fn pcm_to_fd(p: &PCM) -> alsa::Result<std::os::unix::io::RawFd> {
    let mut fds: [libc::pollfd; 1] = unsafe { std::mem::zeroed() };
    let c = (p as &alsa::PollDescriptors).fill(&mut fds)?;
    if c != 1 {
        return Err(alsa::Error::unsupported("snd_pcm_poll_descriptors returned wrong number of fds"))
    }
    Ok(fds[0].fd)
}

fn synch_status(pin: &mut InputPin, pcm_fd: &std::os::unix::io::RawFd, sma_val: &Arc<Mutex<f64>>,
                int_time: u32, rx: &Receiver<()>, sma_num: u32, barrier: &Arc<Barrier>)
{
    let mut sma = SimpleMovingAverage::new(sma_num).unwrap();
    let mut first_time = true;
    for _ in 0..sma_num {
        sma.next(0f64);
    }
    let mut deviation: Vec<i32> = Vec::with_capacity((300f64/int_time as f64) as usize);
    let mut prev_time: u64 = 0;
    pin.set_interrupt(Trigger::RisingEdge).unwrap();
    loop {
        match pin.poll_interrupt(true, Some(std::time::Duration::from_nanos(2*int_time as u64))) {
            Ok(None) => {
                prev_time = 0;
            }
            Ok(_) => {
                match unsafe { SyncPtrStatus::sync_ptr(*pcm_fd, true, None, None) } {
                    Ok(status) => {
                        if first_time { first_time = false; barrier.wait(); }
                        let cur_time = status.htstamp().tv_sec as u64 * pow::pow(10u64,9) + status.htstamp().tv_nsec as u64;
                        if prev_time != 0 {
                            let dev = int_time as i32 - (cur_time - prev_time) as i32;
                            let next_val = sma.next(dev as f64);
                            deviation.push(dev);
                            if let Ok(mut val) = sma_val.try_lock() {
                                *val = next_val;
                            }
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
    npy::to_file("deviation.npy", deviation).unwrap();
}

pub fn main(args: Args) {
    let gpio: rppal::gpio::Gpio = Gpio::new().unwrap();
    let mut pin: InputPin = gpio.get(16).unwrap().into_input_pullup();

    let pcm = PCM::new(&args.flag_device, Direction::Playback, false).unwrap();

    let (tx, rx) = mpsc::channel();

    let mut reader = hound::WavReader::open("test.wav").unwrap();
    let reader_spec = reader.spec();

    let fs = reader_spec.sample_rate;
    let num_channels = reader_spec.channels as u32;
    let int_time: u32 = 2 * 5 * pow(10, 6);

    let sma_val = Arc::new(Mutex::new(0f64));
    let barrier = Arc::new(Barrier::new(2));
    let sync_thr = {
        let pcm_fd = pcm_to_fd(&pcm).unwrap();
        let sma_val = sma_val.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            synch_status(&mut pin, &pcm_fd, &sma_val, int_time, &rx, 1000, &barrier)
        })
    };

    let hwp = HwParams::any(&pcm).unwrap();
    hwp.set_channels(num_channels).unwrap();
    hwp.set_rate(fs, ValueOr::Nearest).unwrap();
    hwp.set_format(Format::s16()).unwrap();
    hwp.set_access(Access::RWInterleaved).unwrap();
    pcm.hw_params(&hwp).unwrap();
    let io = pcm.io_i16().unwrap();

    let hwp = pcm.hw_params_current().unwrap();
    let swp = pcm.sw_params_current().unwrap();
    let period_size = hwp.get_period_size().unwrap();
    let buffer_size = hwp.get_buffer_size().unwrap();
    swp.set_start_threshold(period_size - buffer_size).unwrap();
    swp.set_tstamp_mode(true).unwrap();
    pcm.sw_params(&swp).unwrap();

    let sam_num = period_size as usize * num_channels as usize;
    let mut first_time = true;
    loop {
        let samples = reader.samples::<i16>();

        if samples.len() == 0 { break; }
        let mut buf: Vec<i16> = Vec::with_capacity(sam_num);

        for sample in samples {
            buf.push(sample.unwrap());
            if buf.len() >= sam_num {
                break;
            }
        }

        if first_time {
            first_time = false;
            barrier.wait();
            assert_eq!(io.writei(&buf[..]).unwrap(), buf.len()/num_channels as usize);
            if pcm.state() != State::Running { pcm.start().unwrap() };
        } else {
            assert_eq!(io.writei(&buf[..]).unwrap(), buf.len()/num_channels as usize);
            let dev = *sma_val.lock().unwrap() as i32;
            if args.flag_verbose {
                print!("Deviation: {} ns \r", dev);
                std::io::stdout().flush().unwrap();
            }
        }
    }

    pcm.drain().unwrap();
    tx.send(()).unwrap();
    sync_thr.join().unwrap();
}
