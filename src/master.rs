use std::thread;
use std::sync::mpsc::{self, TryRecvError};

use rppal::gpio::Gpio;

use super::Args;

pub fn main(args : Args) {
    let (tx, rx) = mpsc::channel();

    let child = thread::spawn(move || {
        let gpio = Gpio::new().unwrap();
        let mut pin = gpio.get(4).unwrap().into_output();
        let mut timer = adi_clock::Timer::new(0.005);

        loop {
            match rx.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            timer.wait();
            pin.toggle();
        }
    });

    thread::sleep(std::time::Duration::new(1, 0));
    tx.send(()).unwrap();
    child.join().unwrap();;
}
