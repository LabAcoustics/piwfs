#[macro_use]
extern crate serde_derive;

use docopt::Docopt;

mod master;
mod slave;

const USAGE: &'static str = "
Raspberry Pi Wave Field Synthesis System

Usage:
  piwfs master [--verbose]
  piwfs slave [--device=<dev>] [--verbose]
  piwfs (-h | --help)

Options:
  -h --help        Show this screen.
  --device=<dev>   Select ALSA device [default: hw:0]
  --verbose        Print messages while working
";

#[derive(Debug, Deserialize)]
pub struct Args {
    pub flag_device: String,
    pub flag_verbose: bool,
    cmd_master: bool,
    cmd_slave: bool,
}

fn main() {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.deserialize())
        .unwrap_or_else(|e| e.exit());
    if args.cmd_master {
        master::main(args);
    } else {
        slave::main(args);
    }
}
