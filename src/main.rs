#[macro_use]
extern crate serde_derive;
extern crate docopt;

mod master;
mod slave;

use docopt::Docopt;

const USAGE: &'static str = "
Raspberry Pi Wave Field Synthesis System

Usage:
  wifs master
  wifs slave
  wifs (-h | --help)

Options:
  -h --help     Show this screen.
";

#[derive(Debug, Deserialize)]
struct Args {
    cmd_master: bool,
    cmd_slave: bool,
}

fn main() {
    let args: Args = Docopt::new(USAGE)
                            .and_then(|d| d.deserialize())
                            .unwrap_or_else(|e| e.exit());
    if args.cmd_master {
        master::main();
    } else {
        slave::main();
    }
}
