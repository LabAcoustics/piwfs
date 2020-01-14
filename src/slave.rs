use alsa::{Direction, ValueOr};
use alsa::pcm::{PCM, HwParams, Format, Access, State};
use hound;

use ta::indicators::SimpleMovingAverage;
use ta::Next;

use num::pow;

use super::Args;

fn round_to_zero(n: f64, offset: f64) -> f64 {
    return (n.abs() - offset).round()*n.signum()
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
    swp.set_start_threshold((period_size as i32 * num_channels as i32).into()).unwrap();
    swp.set_tstamp_mode(true).unwrap();
    swp.set_tstamp_type().unwrap();
    pcm.sw_params(&swp).unwrap();
    let sam_num = period_size as usize * num_channels;
    println!("Fs: {}, Channels: {}, Period: {}, Buffer: {}", fs, num_channels, period_size, buffer_size);

    let mut first_time = true;
    let mut corrected_desync = 0;
    let mut desync = SimpleMovingAverage::new(1000).unwrap();

    let sample_duration = pow(10.,9)/(fs as f64);

    let min_delay = period_size;

    loop {
        while pcm.avail_delay().unwrap().1 > min_delay {
            std::thread::sleep(std::time::Duration::from_nanos(sample_duration as u64));
        }
        let status = pcm.status().unwrap();
        let htstamp = status.get_driver_htstamp();
        let delay = status.get_delay();
        let mut buf: Vec<i16> = Vec::with_capacity(sam_num);
        let mut next_sample_time = (htstamp.tv_sec as f64)*pow(10.,9) + (delay as f64)*sample_duration + htstamp.tv_nsec as f64;
        while args.flag_startat > next_sample_time {
            for _ in 0..num_channels { buf.push(0) }
            next_sample_time += sample_duration;
            if buf.len() >= sam_num {
                break;
            }
        }
        if buf.len() < sam_num {
            let next_sample = (next_sample_time - args.flag_startat)/sample_duration as f64;
            let next_read = ((reader.len() as usize - reader.samples::<i16>().len())/num_channels) as f64;
            let cur_desync = desync.next(corrected_desync as f64 + next_sample - next_read);
            let jump = (cur_desync - corrected_desync as f64).floor() as i64;
            print!("Desync: {:.2}, Correction: {}, Delay: {}    \r", cur_desync, corrected_desync, delay);
            if jump != 0 {
                reader.seek((next_read as i64 + jump) as u32).unwrap();
                corrected_desync += jump;
            }

            let mut prev_sample = reader.samples::<i16>().next().unwrap().unwrap();
            let ratio = cur_desync - corrected_desync as f64;

            for sample in reader.samples::<i16>() {
                let cur_sample = sample.unwrap();
                buf.push(((prev_sample as f64)*(1.-ratio) + cur_sample as f64*ratio) as i16);
                prev_sample = cur_sample;
                if buf.len() >= sam_num {
                    break;
                }
            }
            reader.seek((next_read as i64 + jump as i64 + (buf.len()/num_channels) as i64) as u32).unwrap();

        }
        assert_eq!(io.writei(&buf).unwrap(), buf.len()/num_channels);

        if first_time {
            first_time = false;
            assert_eq!(pcm.state(), State::Prepared);
        }
        if buf.len() == 0 { break; }
    }
    println!("");
    pcm.drain().unwrap();
}
