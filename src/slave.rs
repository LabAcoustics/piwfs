use alsa::{Direction, ValueOr};
use alsa::pcm::{PCM, HwParams, Format, Access, State};
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
    let mut out = vec![0; buf.len() - num_channels];
    for channel in 0..num_channels {
        for out_it in (channel..(buf.len() - num_channels)).step_by(num_channels) {
            let mut interp = 0.;
            for in_it in (channel + out_it.saturating_sub((size + 1)*num_channels)..buf.len().min(out_it + size*num_channels)).step_by(num_channels) {
                let cur_r = PI*(ratio + (out_it/num_channels) as f64 - (in_it/num_channels) as f64);
                interp += (buf[in_it] as f64)*cur_r.sin()/cur_r;
            }
            assert_eq!(out[out_it], 0);
            out[out_it] = (std::i16::MIN as f64).max((std::i16::MAX as f64).min(interp)) as i16;
        }
    }
    return out;
}

fn timespec_to_ns(tstamp: libc::timespec) -> f64 {
    return (tstamp.tv_sec as f64) * pow(10.,9) + (tstamp.tv_nsec as f64);
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
    print!("Fs: {}, Channels: {}, Period: {}, Buffer: {}", fs, num_channels, period_size, buffer_size);
    println!("[?25l");
    let mut corrected_desync = 0;
    let mut desync = SimpleMovingAverage::new(1000).unwrap();

    let mut last_status = pcm.status().unwrap();
    let mut last_samples_pushed = 0.;
    let mut real_sample_duration_avg = SimpleMovingAverage::new(1000).unwrap();

    let sample_duration = pow(10.,9)/(fs as f64);

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        }).expect("Error setting Ctrl-C handler");
    }

   while running.load(Ordering::SeqCst) {

        if pcm.state() == State::Running {
            while pcm.avail_delay().unwrap().1 > buffer_fill.into() {
                std::thread::sleep(std::time::Duration::from_nanos(sample_duration as u64));
            }
        }

        let status = pcm.status().unwrap();
        let htstamp = status.get_driver_htstamp();
        let delay = status.get_delay();
        let samples_played = last_status.get_delay() as f64 + last_samples_pushed - delay as f64;
        let real_sample_duration = real_sample_duration_avg.next(if samples_played > 0. {
            let time_played = timespec_to_ns(htstamp) - timespec_to_ns(last_status.get_driver_htstamp());
            time_played/samples_played
        } else {
            sample_duration
        });
        last_status = status;
        let mut buf: Vec<i16> = Vec::with_capacity(sam_num + 1);
        let mut next_sample_time = timespec_to_ns(htstamp) + (delay as f64)*real_sample_duration;

        while args.flag_startat > next_sample_time {
            for _ in 0..num_channels { buf.push(0) }
            next_sample_time += real_sample_duration;
            if buf.len() >= sam_num {
                break;
            }
        }

        let next_sample = (next_sample_time - args.flag_startat)/sample_duration as f64;
        let next_read = ((reader.len() as usize - reader.samples::<i16>().len())/num_channels) as f64;

        let cur_desync = if buf.len() < sam_num {
            let cur_desync = desync.next(corrected_desync as f64 + next_sample - next_read);
            let jump = (cur_desync - corrected_desync as f64).floor() as i64;

            if jump != 0 {
                reader.seek((next_read as i64 + jump) as u32).unwrap();
                corrected_desync += jump;
            }


            for sample in reader.samples::<i16>() {
                buf.push(match sample {
                    Ok(res) => res,
                    Err(_) => break
                });
                if buf.len() > sam_num {
                    break;
                }
            }

            let ratio = cur_desync - corrected_desync as f64;
            buf = sinc_move_inter(&buf, ratio, 3, num_channels);
            reader.seek((next_read as i64 + jump as i64 + (buf.len()/num_channels) as i64) as u32).unwrap();
            cur_desync
        } else {
            next_sample
        };

        print!("Desync: {:.2}  Correction: {}  Delay: {}  Freq: {:.2}%      \r", cur_desync, corrected_desync, delay, 100.*(real_sample_duration/sample_duration));

        match io.writei(&buf) {
            Ok(num) => last_samples_pushed = num as f64,
            Err(err) => if err == alsa::Error::new("snd_pcm_writei", libc::EPIPE) {
                println!("\nERR: Underflow detected!");
                pcm.prepare().unwrap();
            } else {
                panic!(err);
            }
        }

        if buf.len() == 0 { break; }
    }
    println!("[?25h");
    pcm.drain().unwrap();
}
