use alsa::{Direction, ValueOr};
use alsa::pcm::{PCM, HwParams, Format, Access, State};
use hound;

use super::Args;

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
    swp.set_start_threshold(buffer_size).unwrap();
    swp.set_tstamp_mode(true).unwrap();
    swp.set_tstamp_type().unwrap();
    pcm.sw_params(&swp).unwrap();
    let sam_num = period_size as usize * num_channels;

    let mut first_time = true;

    loop {
        let samples =  reader.samples::<i16>();
        let mut buf: Vec<i16> = Vec::with_capacity(sam_num);
        for sample in samples {
            buf.push(sample.unwrap());
            if buf.len() >= sam_num {
                break;
            }
        }

        let status = pcm.status().unwrap();
        let time = status.get_htstamp();

        print!("Cur time: {}:{}\r", time.tv_sec, time.tv_nsec);

        assert_eq!(io.writei(&buf).unwrap(), buf.len()/num_channels);
        if first_time {
            first_time = false;
            assert_eq!(pcm.state(), State::Prepared);
            pcm.start().unwrap();
        }
        if buf.len() == 0 { break; }
    }
}
