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
use samplerate::{Samplerate, ConverterType};
use hound;
use thread_priority::{ThreadPriority, ThreadSchedulePolicy, NormalThreadSchedulePolicy};

use super::Args;

fn pcm_to_fd(p: &PCM) -> alsa::Result<std::os::unix::io::RawFd> {
    let mut fds: [libc::pollfd; 1] = unsafe { std::mem::zeroed() };
    let c = (p as &alsa::PollDescriptors).fill(&mut fds)?;
    if c != 1 {
        return Err(alsa::Error::unsupported("snd_pcm_poll_descriptors returned wrong number of fds"))
    }
    Ok(fds[0].fd)
}

fn vec_f32_to_i16(vec: &[f32]) -> Vec<i16> {
    return vec.into_iter().map(|&e| {
        (e * (std::i16::MAX as f32)) as i16
    }).collect();
}

fn synch_status(pin: &mut InputPin, pcm_fd: &std::os::unix::io::RawFd, sma_val: &Arc<Mutex<f64>>,
                int_counter: &Arc<Mutex<u64>>, int_time: u32, rx: &Receiver<u64>, sma_num: u32,
                barrier: &Arc<Barrier>) {
    thread_priority::set_thread_priority(thread_priority::thread_native_id(),
        ThreadPriority::Max,
        ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Normal)).unwrap();
    let mut sma = SimpleMovingAverage::new(sma_num).unwrap();
    let mut counter = 0;
    let mut next_barrier = 0;
    let max_dev = (int_time / 1000) as i32;
    let mut prev_time: u64 = 0;
    pin.set_interrupt(Trigger::RisingEdge).unwrap();
    loop {
        match pin.poll_interrupt(true, None) {
            Ok(_) => {
                match unsafe { SyncPtrStatus::sync_ptr(*pcm_fd, true, None, None) } {
                    Ok(status) => {
                        counter += 1;
                        if let Ok(mut val) = int_counter.try_lock() {
                            *val = counter;
                        }
                        if counter == next_barrier { barrier.wait(); }
                        let cur_time = status.htstamp().tv_sec as u64 * pow::pow(10u64,9) + status.htstamp().tv_nsec as u64;
                        if prev_time != 0 {
                            let dev = int_time as i32 - (cur_time - prev_time) as i32;
                            if dev.abs() < max_dev {
                                let next_val = sma.next(dev as f64);
                                if let Ok(mut val) = sma_val.try_lock() {
                                    *val = next_val;
                                }
                            }
                        }
                        prev_time = cur_time;
                    }
                    Err(e) => {
                        if let Ok(0) = rx.try_recv() {
                            break;
                        } else {
                            panic!("Error syncing pointer: {:?}", e);
                        }
                    }
                }
            }
            Err(_) => panic!("Error polling interrupt!")
        }
        match rx.try_recv() {
            Ok(0) | Err(TryRecvError::Disconnected) => break,
            Ok(n_bar) => next_barrier = n_bar,
            Err(TryRecvError::Empty) => {}
        }
    }
}

pub fn main(args: Args) {
    let gpio: rppal::gpio::Gpio = Gpio::new().unwrap();
    let mut pin: InputPin = gpio.get(16).unwrap().into_input_pullup();

    let pcm = PCM::new(&args.flag_device, Direction::Playback, false).unwrap();

    let (tx, rx) = mpsc::channel();

    let mut reader = hound::WavReader::open(args.flag_testfile).unwrap();
    let reader_spec = reader.spec();

    let fs = reader_spec.sample_rate;
    let num_channels = reader_spec.channels as usize;
    let int_time: u32 = 2 * 5 * pow(10, 6);

    let sma_val = Arc::new(Mutex::new(0f64));
    let sma_num = 1000;
    let int_counter = Arc::new(Mutex::new(0u64));
    let barrier = Arc::new(Barrier::new(2));
    let sync_thr = {
        let pcm_fd = pcm_to_fd(&pcm).unwrap();
        let sma_val = sma_val.clone();
        let int_counter = int_counter.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            synch_status(&mut pin, &pcm_fd, &sma_val, &int_counter, int_time, &rx, sma_num, &barrier)
        })
    };

    let hwp = HwParams::any(&pcm).unwrap();
    hwp.set_channels(num_channels as u32).unwrap();
    hwp.set_rate(fs, ValueOr::Nearest).unwrap();
    hwp.set_format(Format::s16()).unwrap();
    hwp.set_access(Access::RWInterleaved).unwrap();
    pcm.hw_params(&hwp).unwrap();
    let io = pcm.io_i16().unwrap();

    let hwp = pcm.hw_params_current().unwrap();
    let swp = pcm.sw_params_current().unwrap();
    let period_size = hwp.get_period_size().unwrap();
    let buffer_size = hwp.get_buffer_size().unwrap();
    swp.set_start_threshold(buffer_size).unwrap();
    swp.set_tstamp_mode(true).unwrap();
    pcm.sw_params(&swp).unwrap();

    let sam_num = period_size as usize * num_channels;
    let mut first_time = true;
    let mut converter = Samplerate::new(ConverterType::Linear, 1, 1, num_channels).unwrap();
    loop {
        let samples =  reader.samples::<i16>();

        if samples.len() == 0 { break; }
        let mut buf: Vec<f32> = Vec::with_capacity(sam_num);

        for sample in samples {
            buf.push((sample.unwrap() as f32) / (std::i16::MAX as f32));
            if buf.len() >= sam_num {
                break;
            }
        }

        if first_time {
            first_time = false;
            let wait_for = sma_num as u64;
            let zeros = vec![0i16; sam_num];
            assert_eq!(io.writei(&zeros).unwrap(), zeros.len()/num_channels);
            pcm.start().unwrap();
            if args.flag_verbose { println!("Measuring deviation..."); }
            while *int_counter.lock().unwrap() < wait_for {
                assert_eq!(io.writei(&zeros).unwrap(), zeros.len()/num_channels);
                if args.flag_verbose {
                    let dev = *sma_val.lock().unwrap() as i32;
                    print!("Deviation: {} ns \r", dev);
                    std::io::stdout().flush().unwrap();
                }
            }
            let dev = *sma_val.lock().unwrap() as i32;
            pcm.drop().unwrap();
            pcm.prepare().unwrap();
            converter = Samplerate::new(ConverterType::Linear,
                                        int_time, (int_time as i32 + dev) as u32,
                                        num_channels).unwrap();
            let resampled = converter.process(&buf[..]).unwrap();
            if args.flag_verbose { println!("\nSamplerate converter prepared..."); }
            assert_eq!(io.writei(&vec_f32_to_i16(&resampled)).unwrap(), resampled.len()/num_channels);
            assert_eq!(pcm.state(), State::Prepared);
            tx.send(wait_for + 100).unwrap();
            if args.flag_verbose { println!("Waiting for interrupt..."); }
            barrier.wait();
            pcm.start().unwrap();
        } else {
            let dev = *sma_val.lock().unwrap() as i32;
            converter.set_to_rate((int_time as i32 + dev) as u32);
            let resampled = converter.process(&buf[..]).unwrap();
            assert_eq!(io.writei(&vec_f32_to_i16(&resampled)).unwrap(), resampled.len()/num_channels);
            if args.flag_verbose {
                print!("Deviation: {} ns \r", dev);
                std::io::stdout().flush().unwrap();
            }
        }
    }
    if args.flag_verbose { println!("\nWaiting for PCM..."); }
    pcm.drain().unwrap();
    if args.flag_verbose { println!("Stopping sync thread..."); }
    tx.send(0).unwrap();
    sync_thr.join().unwrap();
}
