use alsa::pcm::{Access, Format, HwParams, State, PCM};
use alsa::{Direction, ValueOr};
use hound;

use ta::indicators::SimpleMovingAverage;
use ta::Next;

use ctrlc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use num::pow;
use std::f64::consts::PI;

use libc;

use super::Args;

fn sinc_move_inter(buf: &Vec<i16>, ratio: f64, size: usize, num_channels: usize) -> Vec<i16> {
    let out_size = buf.len() - (2 * size - 1) * num_channels;
    let mut out = vec![0; out_size];
    for channel in 0..num_channels {
        for out_it in (channel..out_size).step_by(num_channels) {
            let mut interp = 0.;
            for in_it in
                ((channel + out_it)..(out_it + (2 * size - 1) * num_channels)).step_by(num_channels)
            {
                let cur_r = PI
                    * (ratio + (out_it / num_channels + size - 1) as f64
                        - (in_it / num_channels) as f64);
                interp += (buf[in_it] as f64) * cur_r.sin() / cur_r;
            }
            assert_eq!(out[out_it], 0);
            out[out_it] = (std::i16::MIN as f64).max((std::i16::MAX as f64).min(interp)) as i16;
        }
    }
    return out;
}

fn timespec_to_ns(tstamp: libc::timespec) -> f64 {
    return (tstamp.tv_sec as f64) * pow(10., 9) + (tstamp.tv_nsec as f64);
}

pub fn main(args: Args) {
    let pcm = PCM::new(&args.flag_device, Direction::Playback, false).unwrap();

    let mut reader = hound::WavReader::open(args.flag_testfile).unwrap();
    let reader_spec = reader.spec();

    let fs = reader_spec.sample_rate;
    let num_channels = reader_spec.channels as usize;

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
    let buffer_fill = 2 * period_size as i32 * num_channels as i32;
    swp.set_start_threshold(buffer_fill.into()).unwrap();
    swp.set_tstamp_mode(true).unwrap();
    swp.set_tstamp_type().unwrap();
    pcm.sw_params(&swp).unwrap();
    let sam_num = period_size as usize * num_channels;
    let sinc_overlap = if !args.flag_no_correction { 3 } else { 0 };
    let sam_num_over = sam_num + (2 * sinc_overlap - 1) * num_channels;
    print!(
        "Fs: {}, Channels: {}, Period: {}, Buffer: {}",
        fs, num_channels, period_size, buffer_size
    );
    println!("[?25l");
    let mut corrected_desync = 0;
    let mut desync = SimpleMovingAverage::new(1000).unwrap();

    let sample_duration = pow(10., 9) / (fs as f64);

    let mut last_samples_pushed = 0.;
    let mut last_delay = 0.;
    let mut last_stamp = 0.;
    let mut real_sample_duration_avg = SimpleMovingAverage::new(1000).unwrap();

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        })
        .expect("Error setting Ctrl-C handler");
    }

    while running.load(Ordering::SeqCst) {
        let mut stamps = Vec::new();
        let mut delays = Vec::new();

        loop {
            let status = pcm.status().unwrap();
            stamps.push(timespec_to_ns(status.get_htstamp()));
            let delay = status.get_delay() as f64;
            delays.push(delay);

            if status.get_state() == State::Running && delay > buffer_fill.into() {
                std::thread::sleep(std::time::Duration::from_nanos(sample_duration as u64));
            } else {
                break;
            }
        }

        let real_sample_duration = real_sample_duration_avg.next(
            if pcm.state() == State::Running && last_delay > 0. && last_samples_pushed > 0. {
                let mut skipped = 0;
                stamps
                    .iter()
                    .zip(delays.iter())
                    .fold(0., |acc, (stamp, delay)| {
                        let mtime =
                            (stamp - last_stamp) / (last_delay + last_samples_pushed - delay);
                        //println!("DBG: s = {}, ls = {}, d = {}, ld = {}, lsp = {}, t = {}", stamp, last_stamp, delay, last_delay, last_samples_pushed, mtime);
                        acc + if last_samples_pushed == *delay {
                            skipped += 1;
                            0.
                        } else {
                            assert!(mtime > 0., format!("ERR: Mean sample time less than zero!"));
                            mtime
                        }
                    })
                    / (stamps.len() - skipped) as f64
            } else {
                sample_duration
            },
        );

        let mut buf: Vec<i16> = Vec::with_capacity(sam_num_over);
        let mut next_sample_time = stamps
            .iter()
            .zip(delays.iter())
            .fold(0., |acc, (stamp, delay)| {
                acc + stamp + delay * real_sample_duration
            })
            / (stamps.len() as f64);

        while args.flag_startat > next_sample_time {
            for _ in 0..num_channels {
                buf.push(0)
            }
            next_sample_time += real_sample_duration;
            if buf.len() >= sam_num {
                break;
            }
        }

        let next_sample = (next_sample_time - args.flag_startat) / sample_duration as f64;
        let next_read = ((reader.len() as usize - reader.samples::<i16>().len()) / num_channels)
            as f64
            + sinc_overlap as f64
            - 1.;

        let cur_desync = if buf.len() < sam_num {
            let cur_desync = desync.next(corrected_desync as f64 + next_sample - next_read);
            let jump = (cur_desync - corrected_desync as f64).floor() as i64;
            let jumpto = next_read as i64 - sinc_overlap as i64 + 1 + jump;

            if !args.flag_no_correction && jump != 0 && jumpto >= 0 {
                reader.seek(jumpto as u32).unwrap();
                corrected_desync += jump;
            }

            for sample in reader.samples::<i16>() {
                buf.push(match sample {
                    Ok(res) => res,
                    Err(_) => break,
                });
                if buf.len() > sam_num_over {
                    break;
                }
            }

            let ratio = cur_desync - corrected_desync as f64;
            buf = if !args.flag_no_correction {
                let b = sinc_move_inter(&buf, ratio, sinc_overlap, num_channels);
                reader
                    .seek(
                        (next_read as i64 + jump as i64 + (buf.len() / num_channels) as i64
                            - sinc_overlap as i64
                            + 1) as u32,
                    )
                    .unwrap();
                b
            } else {
                buf
            };
            cur_desync
        } else {
            next_sample
        };

        print!(
            "Desync: {:.2}  Correction: {}  Delay: {}  Freq: {:+.3}%  Mean: {}    \r",
            cur_desync,
            corrected_desync,
            delays.last().unwrap(),
            100. * (real_sample_duration / sample_duration - 1.),
            delays.len()
        );

        match io.writei(&buf) {
            Ok(num) => {
                last_samples_pushed = num as f64;
                last_delay = *delays.first().unwrap();
                last_stamp = stamps
                    .iter()
                    .zip(delays.iter())
                    .fold(0., |acc, (stamp, delay)| {
                        acc + stamp - (last_delay - delay) * real_sample_duration
                    })
                    / stamps.len() as f64;
            }
            Err(err) => {
                if err == alsa::Error::new("snd_pcm_writei", libc::EPIPE) {
                    println!("\nERR: Underflow detected!");
                    pcm.prepare().unwrap();
                    last_samples_pushed = 0.;
                } else {
                    panic!(err);
                }
            }
        }

        if buf.len() == 0 {
            break;
        }
    }
    println!("[?25h");
    pcm.drain().unwrap();
}
