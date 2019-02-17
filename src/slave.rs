use std::thread;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rppal::gpio::{Gpio, Trigger};
use alsa::Direction;
use alsa::pcm::PCM;
use alsa::direct::pcm::SyncPtrStatus;

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
    let mut pin = gpio.get(4).unwrap().into_input_pulldown();

    let pcm = PCM::new(&args.flag_device, Direction::Playback, false).unwrap();
    let pcm_fd = Arc::new(Mutex::new(pcm_to_fd(&pcm).unwrap()));

    {
        let pcm_fd = pcm_fd.clone();
        pin.set_async_interrupt(Trigger::RisingEdge, move |_level| {
            let status = unsafe {
                SyncPtrStatus::sync_ptr(*pcm_fd.lock().unwrap(), false, None, None).unwrap()
            };
            println!("Interrupted! Time: {}", status.htstamp().tv_nsec);
        }).unwrap();
    }

    thread::sleep(Duration::from_secs(10));
}
