use alsa::pcm::{Access, Format, HwParams, State, PCM};
use alsa::{Direction, ValueOr};
use nix::sys::time::{TimeSpec, TimeValLike};
use hound;

use ta::indicators::SimpleMovingAverage;
use ta::Next;

use ctrlc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use std::convert::TryInto;
use std::collections::VecDeque;
use std::f64::consts::PI;

use libc;

use clap::ArgMatches;

fn sinc_move_inter(buf: &Vec<i16>, ratio: f64, size: usize, num_channels: usize) -> Vec<i16> {
    let out_size = buf.len() - (2 * size + 1) * num_channels;
    let mut out = vec![0; out_size];
    for channel in 0..num_channels {
        for out_it in (channel..out_size).step_by(num_channels) {
            let mut interp = 0.;
            for in_it in
                ((channel + out_it)..(out_it + (2 * size + 1) * num_channels)).step_by(num_channels)
            {
                let cur_r = PI
                    * (ratio + (out_it / num_channels + size) as f64
                        - (in_it / num_channels) as f64);
                interp += (buf[in_it] as f64) * cur_r.sin() / cur_r;
            }
            assert_eq!(out[out_it], 0);
            out[out_it] = (std::i16::MIN as f64).max((std::i16::MAX as f64).min(interp)) as i16;
        }
    }
    return out;
}

fn next_rdr_sample<T: std::io::Read>(reader: &mut hound::WavReader<T>) -> u32 {
    return (reader.len() - reader.samples::<i16>().len() as u32) / reader.spec().channels as u32;
}

pub fn main(args: &ArgMatches) {
    let pcm = PCM::new(
        &args.value_of("device").unwrap_or("hw:0"),
        Direction::Playback,
        false,
    )
    .unwrap();
    let mut reader = hound::WavReader::open(args.value_of("testfile").unwrap()).unwrap();
    let is_correction = !args.is_present("no-correction");
    let is_spinning = !args.is_present("no-spinning");
    let startstamp = TimeSpec::nanoseconds(args
        .value_of("startat")
        .unwrap()
        .parse::<i64>()
        .expect("[ERR] Couldn't parse startat as a integer number"));
    let est_avg_size = args
        .value_of("estimation-avg")
        .unwrap_or("1000")
        .parse::<u32>()
        .expect("[ERR] Couldn't parse average as an unsigned integer");
    let desync_avg_size = args
        .value_of("desync-avg")
        .unwrap_or("1000")
        .parse::<u32>()
        .expect("[ERR] Couldn't parse average as an unsigned integer");
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
    let sinc_overlap = if is_correction {
        args.value_of("quality")
            .unwrap_or("2")
            .parse::<usize>()
            .expect("[ERR] Couldn't parse quality as an unsigned integer")
    } else {
        0
    };
    print!(
        "[INF] Fs: {}, Channels: {}, Period: {}, Buffer: {}",
        fs, num_channels, period_size, buffer_size
    );
    println!("[?25l");

    let sam_num = period_size as usize * num_channels;
    let sam_num_over = sam_num + (2*sinc_overlap + 1)*num_channels;

    let mut desync = SimpleMovingAverage::new(desync_avg_size).unwrap();
    let mut act_desync_avg = SimpleMovingAverage::new(desync_avg_size*10).unwrap();
    let mut correction = 0;

    let sample_duration = 10f64.powi(9) / (fs as f64);
    let mut real_sample_duration = sample_duration;
    let mut real_sample_duration_avg = SimpleMovingAverage::new(est_avg_size).unwrap();

    let mut last_samples_pushed = 0;
    let mut last_delays = Vec::new();
    let mut last_stamps = Vec::new();

    let mut samples_pushed = 0;
    let mut nsts = VecDeque::new();
    let mut est_error_avg = SimpleMovingAverage::new(1000).unwrap();

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        })
        .expect("[ERR] Error setting Ctrl-C handler");
    }

    while running.load(Ordering::SeqCst) {
        samples_pushed += last_samples_pushed;
        let mut stamps = Vec::new();
        let mut delays = Vec::new();

        loop {
            let status = pcm.status().unwrap();
            let libc_stamp = status.get_htstamp();
            let stamp = TimeSpec::seconds(libc_stamp.tv_sec.into()) + TimeSpec::nanoseconds(libc_stamp.tv_nsec.into());
            if Some(&stamp) == stamps.last() {
                continue;
            }

            let delay = status.get_delay();

            //if is_spinning {
                stamps.push(stamp);
                delays.push(delay);
            //} else {
                //std::thread::sleep(std::time::Duration::from_nanos(sample_duration.num_nanoseconds() as u64 / 2));
            //}

            if status.get_state() != State::Running || delay < buffer_fill.into() {
                //if !is_spinning {
                    //stamps.push(stamp);
                    //delays.push(delay);
                //}
                break;
            }
        }
        let mut est_error = 0.;
        for (stamp, delay) in stamps.iter().zip(delays.iter()) {
            loop {
                if let Some((ns, nst)) = nsts.get(0) {
                    let cur_ns = samples_pushed - delay;
                    if cur_ns == *ns {
                        let err: TimeSpec = *nst - *stamp;
                        //println!("[DBG] Est error: {} (est = {}, act = {})", *nst - *stamp, nst, stamp);
                        est_error = est_error_avg.next(err.num_nanoseconds().abs() as f64);
                        nsts.remove(0);
                    } else if cur_ns > *ns {
                        nsts.remove(0);
                        continue;
                    }           
                }
                break;
            }
        }
        if !is_spinning {
            stamps = vec![*stamps.last().unwrap()];
            delays = vec![*delays.last().unwrap()];
        }

        real_sample_duration = real_sample_duration_avg.next(
            if !args.is_present("no-estimation")
                && pcm.state() == State::Running
                && last_samples_pushed > 0
            {
                stamps
                    .iter()
                    .zip(delays.iter())
                    .fold(0., |acc, (stamp, delay)| {
                        //println!("[DBG] s = {}, ls = {}, d = {}, ld = {}, lsp = {}, t = {}", stamp, last_stamp, delay, last_delay, last_samples_pushed, mtime);
                        let mtime = last_stamps
                            .iter()
                            .zip(last_delays.iter())
                            .fold(0., |acc2, (last_stamp, last_delay)| {
                                acc2 + if *last_delay <= 0 {
                                    println!("\n[WRN] Delay less or equal 0!");
                                    real_sample_duration
                                } else {
                                    (*stamp - *last_stamp).num_nanoseconds() as f64 / (last_delay + last_samples_pushed - delay) as f64
                                }
                            }) / last_stamps.len() as f64;
                        acc + if mtime > 0. {
                            mtime 
                        } else {
                            println!("\n[WRN] Mean sample time less or equal to zero!");
                            real_sample_duration
                        }
                    }) / stamps.len() as f64
            } else {
                real_sample_duration
            },
        );

        let mut buf: Vec<i16> = Vec::with_capacity(sam_num_over);
        let mut next_sample_time = stamps
            .iter()
            .zip(delays.iter())
            .fold(TimeSpec::zero(), |acc, (stamp, delay)| {
                acc + (*stamp + TimeSpec::nanoseconds((real_sample_duration * *delay as f64).round() as i64)) / (stamps.len() as i32)
            });
        nsts.push_back((samples_pushed, next_sample_time));

        while startstamp > next_sample_time {
            for _ in 0..num_channels {
                buf.push(0)
            }
            next_sample_time = next_sample_time + TimeSpec::nanoseconds(real_sample_duration as i64);
            if buf.len() == sam_num {
                break;
            } else if buf.len() > sam_num {
                unreachable!()
            }
        }

        let next_sample = (next_sample_time - startstamp).num_nanoseconds() as f64 / sample_duration;
        let next_read = next_rdr_sample(&mut reader).saturating_sub(sinc_overlap as u32 + 1);
        let act_desync = next_sample - next_read as f64;
        let mut avg_act_desync = act_desync;

        let cur_desync = if buf.len() < sam_num {
            avg_act_desync = act_desync_avg.next(act_desync);
            let cur_desync = desync.next(correction as f64 + avg_act_desync + act_desync);
            let jump = (cur_desync - correction as f64).floor() as i64;
            let jumpto = if jump > 0 {
                next_read.saturating_add(jump as u32)
            } else {
                next_read.saturating_sub((-jump) as u32)
            }.saturating_sub(sinc_overlap as u32);
            //println!("[DBG] ===============================");
            //println!("[DBG] j = {}, jt = {}, c = {}, lsp = {}", jump, jumpto, correction, last_samples_pushed);

            if is_correction {
                reader.seek(jumpto).unwrap();
                correction += jump;
            }

            for sample in reader.samples::<i16>() {
                buf.push(match sample {
                    Ok(res) => res,
                    Err(_) => break,
                });
                if buf.len() == sam_num_over {
                    break;
                } else if buf.len() > sam_num_over {
                    unreachable!()
                }
            }

            let ratio = cur_desync - correction as f64;
            if is_correction  {
                buf = if buf.len() > (2*sinc_overlap + 1)*num_channels {
                    sinc_move_inter(&buf, ratio, sinc_overlap, num_channels)
                } else {
                    buf[sinc_overlap..].into()
                }
            }

            if buf.len() == 0 {
                break;
            }

            cur_desync
        } else {
            next_sample as f64
        };

        print!(
            "[INF] Desync: {:+.2}, Diff: {:+.2}, Delay: {}, Freq: {:+.3}%, Error: {:.0} us, Spins: {}[K\r",
            cur_desync,
            avg_act_desync,
            delays.last().unwrap(),
            100. * (sample_duration / real_sample_duration - 1.),
            est_error/1000.,
            delays.len()
        );
        //println!("\n[DBG] ns = {}, nr = {}, nrs = {}, nst = {}", next_sample, next_read, next_read, next_sample_time);

        match io.writei(&buf) {
            Ok(num) => {
                assert_eq!(num, buf.len()/num_channels);
                last_samples_pushed = num.try_into().unwrap();
                last_delays = delays;
                last_stamps = stamps;
            }
            Err(err) => {
                if err == alsa::Error::new("snd_pcm_writei", libc::EPIPE) {
                    println!("\n[ERR] Underflow detected!");
                    pcm.prepare().unwrap();
                    last_samples_pushed = 0;
                } else {
                    panic!(err);
                }
            }
        }
    }
    println!("[?25h");
    pcm.drain().unwrap();
}
