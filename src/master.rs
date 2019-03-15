use std::thread;
use std::io::Write;
use std::sync::mpsc::{self, TryRecvError};
use std::net::{TcpListener, SocketAddr};

use rppal::gpio::Gpio;
use ta::indicators::SimpleMovingAverage;
use ta::Next;
use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::TcpStream;
use num::pow;
use npy;

use super::Args;

pub fn main(_args : Args) {
    let (tx, rx) = mpsc::channel();

    let wait_time: u64 = 300;
    let sma_num = 1000;
    let int_time: f32 = 0.01;

    let child = thread::spawn(move || {
        let gpio = Gpio::new().unwrap();
        let mut pin = gpio.get(16).unwrap().into_output();
        let mut timer = adi_clock::Timer::new(int_time/2.0);
        let mut sma = SimpleMovingAverage::new(sma_num).unwrap();
        let mut prev_time = 0f32;
        for _ in 0..sma_num {
            sma.next(0f64);
        }

	pin.set_reset_on_drop(false);
	pin.set_high();
        let mut high = true;

        let mut deviation: Vec<i32> = Vec::with_capacity((wait_time as f32/int_time) as usize);

        loop {
            match rx.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            let cur_time = timer.wait();
            pin.toggle();
            high = !high;
            if !high {
                if prev_time != 0f32 {
                    let dev = pow(10.0, 9)*(int_time - (cur_time - prev_time));
                    deviation.push(dev as i32);
                    print!("Deviation: {} ns\r", sma.next(dev as f64) as i32);
                    std::io::stdout().flush().unwrap();
                }
                prev_time = cur_time;
            }
        }
	pin.set_high();
        npy::to_file("deviation.npy", deviation).unwrap();
    });

    let net_th = thread::spawn(move || {
        let addr: SocketAddr = "0.0.0.0:10000".parse().unwrap();
        let server = TcpListener::bind(&addr).unwrap();
        let poll = Poll::new().unwrap();
        let mut events = Events::with_capacity(1024);
        let stream = TcpStream::connect(&server.local_addr().unwrap()).unwrap();

        poll.register(&stream, Token(0), Ready::readable() | Ready::writable(), PollOpt::edge()).unwrap();

        loop {
            if true { break }
            poll.poll(&mut events, None).unwrap();

            for event in &events {
                if event.token() == Token(0) && event.readiness().is_writable() {

                    
                }
            }
        }
    });


    thread::sleep(std::time::Duration::new(wait_time, 0));
    tx.send(()).unwrap();
    net_th.join().unwrap();
    child.join().unwrap();
}
