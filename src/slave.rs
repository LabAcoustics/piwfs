use alsa::pcm::{Access, Format, HwParams, State, TstampType, PCM};
use alsa::{Direction, ValueOr};
use hound;

use indicator::{Average, Indicator, LinearRegression, Median, Variance};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use std::collections::VecDeque;
use std::convert::TryInto;
use std::f32::consts::PI;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::panic::panic_any;

use clap::ArgMatches;

fn sinc_move_inter(buf: &Vec<i16>, ratio: f32, size: usize, num_channels: usize) -> Vec<i16> {
    let out_size = buf.len() - (2 * size + 1) * num_channels;
    let mut out = vec![0; out_size];
    for channel in 0..num_channels {
        for out_it in (channel..out_size).step_by(num_channels) {
            let mut interp = 0.;
            for in_it in
                ((channel + out_it)..(out_it + (2 * size + 1) * num_channels)).step_by(num_channels)
            {
                let cur_r = PI
                    * (ratio + (out_it / num_channels + size) as f32
                        - (in_it / num_channels) as f32);
                interp += (buf[in_it] as f32) * cur_r.sin() / cur_r;
            }
            out[out_it] = (std::i16::MIN as f32).max((std::i16::MAX as f32).min(interp)) as i16;
        }
    }
    return out;
}

fn next_rdr_sample<T: std::io::Read>(reader: &mut hound::WavReader<T>) -> u32 {
    return (reader.len() - reader.samples::<i16>().len() as u32) / reader.spec().channels as u32;
}

fn duration_diff_secs_f64(lhs: SystemTime, rhs: SystemTime) -> f64 {
    return if lhs > rhs {
        lhs.duration_since(rhs).unwrap().as_secs_f64()
    } else {
        -rhs.duration_since(lhs).unwrap().as_secs_f64()
    };
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
    let startstamp = UNIX_EPOCH
        + Duration::from_nanos(
            args.value_of("startat")
                .unwrap()
                .parse::<u64>()
                .expect("[ERR] Couldn't parse startat as a unsigned integer number"),
        );
    let est_avg_size = args
        .value_of("estimation-avg")
        .unwrap_or("1000")
        .parse::<usize>()
        .expect("[ERR] Couldn't parse average as an unsigned integer");
    let desync_avg_size = args
        .value_of("desync-avg")
        .unwrap_or("1000")
        .parse::<usize>()
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
    swp.set_tstamp_type(TstampType::Gettimeofday).unwrap();
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
    let sam_num_over = sam_num + (2 * sinc_overlap + 1) * num_channels;

    let mut desync = LinearRegression::new(desync_avg_size).unwrap();
    let mut act_desync_avg = Average::new(10000).unwrap();
    let mut correction = 0.;

    let sample_duration = 1. / (fs as f64);
    let mut real_sample_duration = sample_duration;
    let mut real_sample_duration_avg = Median::new(est_avg_size).unwrap();

    let mut last_samples_pushed = 0;

    let mut samples_pushed = 0;
    let mut nsts = VecDeque::new();
    let mut est_error_var = Variance::new(1000).unwrap();

    let sigint = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&sigint))
        .expect("[ERR] Error setting SIGINT hook");

    while !sigint.load(Ordering::Relaxed) {
        let loop_start = std::time::Instant::now();
        let mut elapsed_times = Vec::new();
        samples_pushed += last_samples_pushed;
        let mut stamps = Vec::new();
        let mut delays = Vec::new();

        loop {
            let status = pcm.status().unwrap();
            let libc_stamp = status.get_htstamp();
            let stamp = UNIX_EPOCH
                + Duration::new(
                    libc_stamp.tv_sec.try_into().unwrap(),
                    libc_stamp.tv_nsec.try_into().unwrap(),
                );
            if Some(&stamp) == stamps.last() {
                continue;
            }

            let delay = status.get_delay();

            if is_spinning {
                stamps.push(stamp);
                delays.push(delay);
            } else {
                std::thread::sleep(Duration::from_secs_f64(sample_duration / 2.));
            }

            if status.get_state() != State::Running || delay < buffer_fill.into() {
                if !is_spinning {
                    stamps.push(stamp);
                    delays.push(delay);
                }
                break;
            }
        }
        elapsed_times.push(("Spinning", loop_start.elapsed()));
        let mut est_error = [0., 0.];
        for (stamp, delay) in stamps.iter().zip(delays.iter()) {
            loop {
                if let Some((ns, nst)) = nsts.get(0) {
                    let cur_ns = samples_pushed - delay;
                    if cur_ns == *ns {
                        let err = duration_diff_secs_f64(*nst, *stamp) * 1_000_000.;
                        //println!("[DBG] Est error: {} (est = {}, act = {})", *nst - *stamp, nst, stamp);
                        est_error_var.next(err);
                        if let Some(var) = est_error_var.value() {
                            est_error = [var, est_error_var.average().unwrap()];
                        }
                        nsts.remove(0);
                    } else if cur_ns > *ns {
                        nsts.remove(0);
                        continue;
                    }
                }
                break;
            }
        }
        elapsed_times.push(("Error estimation", loop_start.elapsed()));

        real_sample_duration_avg.next(
            if !args.is_present("no-estimation")
                && pcm.state() == State::Running
                && stamps.len() > 1
            {
                stamps
                    .windows(2)
                    .zip(delays.windows(2))
                    .fold(0., |acc, (stampw, delayw)| {
                        let mtime = stampw[1].duration_since(stampw[0]).unwrap().as_secs_f64()/(delayw[0] - delayw[1]) as f64;
                        acc + if mtime > 0. {
                            mtime
                        } else {
                            println!("[WRN] Non-continous status times or delays");
                            real_sample_duration
                        }
                    }) / (stamps.len() - 1) as f64
            } else {
                real_sample_duration
            },
        );
        real_sample_duration = real_sample_duration_avg.value().unwrap();
        elapsed_times.push(("Sample duration estimation", loop_start.elapsed()));

        let mut buf: Vec<i16> = Vec::with_capacity(sam_num_over);
        let mut next_sample_time = UNIX_EPOCH
            + stamps
                .iter()
                .zip(delays.iter())
                .fold(Duration::new(0, 0), |acc, (stamp, delay)| {
                    acc + (stamp.duration_since(UNIX_EPOCH).unwrap()
                        + Duration::from_secs_f64(real_sample_duration * *delay as f64))
                        / stamps.len().try_into().unwrap()
                });
        nsts.push_back((samples_pushed, next_sample_time));
        elapsed_times.push(("Next sample time estimation", loop_start.elapsed()));

        let mut zeros_pushed = 0.;
        while startstamp
            > next_sample_time + Duration::from_secs_f64(real_sample_duration * zeros_pushed)
        {
            for _ in 0..num_channels {
                buf.push(0)
            }
            zeros_pushed += 1.;
            if buf.len() == sam_num {
                break;
            } else if buf.len() > sam_num {
                unreachable!()
            }
        }
        next_sample_time += Duration::from_secs_f64(real_sample_duration * zeros_pushed);

        let (cur_desync, avg_act_desync) = if buf.len() < sam_num {
            let next_sample = next_sample_time
                .duration_since(startstamp)
                .unwrap()
                .as_secs_f64()
                / sample_duration;
            let next_read = next_rdr_sample(&mut reader).saturating_sub(sinc_overlap as u32 + 1);
            let act_desync = next_sample - next_read as f64;
            act_desync_avg.next(act_desync);
            let next_sample_time_f64 = next_sample_time
                .duration_since(startstamp)
                .unwrap()
                .as_secs_f64();
            desync.next((next_sample_time_f64, correction as f64 + act_desync));
            let (desync_a, desync_b) = desync.value().unwrap_or((0., 0.));
            let cur_desync = desync_a + desync_b * next_sample_time_f64;
            let jump = (cur_desync - correction).floor() as i64;
            let max_jump = 100;
            let jump = if jump.abs() > max_jump {
                jump.signum() * max_jump
            } else {
                jump
            };
            let jumpto = if jump > 0 {
                next_read.saturating_add(jump as u32)
            } else {
                next_read.saturating_sub((-jump) as u32)
            }
            .saturating_sub(sinc_overlap as u32);
            let jumpto = if jumpto > reader.len() / num_channels as u32 {
                reader.len() / num_channels as u32
            } else {
                jumpto
            };
            //println!("[DBG] ===============================");
            //println!("[DBG] j = {}, jt = {}, c = {}, lsp = {}", jump, jumpto, correction, last_samples_pushed);

            if is_correction {
                correction += jumpto as f64 - next_read.saturating_sub(sinc_overlap as u32) as f64;
                reader.seek(jumpto).unwrap();
            }
            elapsed_times.push(("Seeking", loop_start.elapsed()));

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

            let ratio = cur_desync - correction;
            if is_correction {
                buf = if buf.len() > (2 * sinc_overlap + 1) * num_channels {
                    sinc_move_inter(&buf, ratio as f32, sinc_overlap, num_channels)
                } else {
                    buf[sinc_overlap..].into()
                }
            }
            elapsed_times.push(("Interpolation", loop_start.elapsed()));

            if buf.len() == 0 {
                break;
            }

            (cur_desync, act_desync_avg.value().unwrap())
        } else {
            (
                -startstamp
                    .duration_since(next_sample_time)
                    .unwrap_or(Duration::new(0, 0))
                    .as_secs_f64()
                    / sample_duration,
                0.,
            )
        };

        print!(
            "[INF] Desync: {:+.1}, Diff: {:+.3}, Delay: {}, Freq: {:+.3}%, Error: {:+.0}±{:.0} us, Spins: {}[K\r",
            cur_desync,
            avg_act_desync,
            delays.last().unwrap(),
            100. * (sample_duration / real_sample_duration - 1.),
            est_error[1],
            est_error[0].sqrt(),
            delays.len()
        );
        //println!("\n[DBG] ns = {}, nr = {}, nrs = {}, nst = {}", next_sample, next_read, next_read, next_sample_time);
        elapsed_times.push(("Printing", loop_start.elapsed()));

        match io.writei(&buf) {
            Ok(num) => {
                assert_eq!(num, buf.len() / num_channels);
                last_samples_pushed = num.try_into().unwrap();
            }
            Err(err) => {
                if let Some(errno) = err.errno() {
                    if errno == nix::errno::Errno::EPIPE {
                        println!("\n[ERR] ALSA buffer underrun!");
                        println!("----- Execution times breakdown:");
                        for ind in 0..elapsed_times.len() {
                            let took_time = if ind > 0 {
                                elapsed_times[ind].1 - elapsed_times[ind - 1].1
                            } else {
                                elapsed_times[ind].1
                            };
                            println!(
                                "----> {} ended at {:?} (took {:?})",
                                elapsed_times[ind].0, elapsed_times[ind].1, took_time
                            );
                        }
                        println!(
                            "----- Estimated time budget: {:?}",
                            Duration::from_secs_f64(
                                *delays.first().unwrap() as f64 * real_sample_duration
                            )
                        );
                        last_samples_pushed = 0;
                        pcm.prepare().unwrap();
                    } else {
                        panic_any(err);
                    }
                } else {
                    panic_any(err);
                }
            }
        }
    }
    println!("[?25h");
    pcm.drain().unwrap();
}
