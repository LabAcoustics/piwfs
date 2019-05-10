use std::thread;
use std::net::{TcpListener, SocketAddr};

use rppal::pwm::{Channel, Polarity, Pwm};
use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::TcpStream;

use super::Args;

pub fn main(args : Args) {
    let wait_time: u64 = args.flag_time;

    let pwm = Pwm::with_frequency(Channel::Pwm0, args.flag_frequency as f64, 0.5, Polarity::Normal, true).unwrap();

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
    pwm.disable().unwrap();
    net_th.join().unwrap();
}
