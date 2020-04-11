use clap::{App, Arg, SubCommand};

mod master;
mod slave;

fn main() {
    let matches = App::new("piwfs")
        .version("0.2.1")
        .author("Szymon Mikulicz <szymon.mikulicz@posteo.net>")
        .about("Wafe Field Synthesis for Raspberry Pi")
        .subcommand(
            SubCommand::with_name("master")
                .about("The authoritative instance")
                .version("0")
                .author("Noone"),
        )
        .subcommand(
            SubCommand::with_name("slave")
                .about("The slave instance")
                .version("0.2.1")
                .author("Szymon Mikulicz <szymon.mikulicz@posteo.net>")
                .arg(
                    Arg::with_name("device")
                        .short("d")
                        .long("device")
                        .value_name("DEVICE")
                        .help("Sets ALSA device")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("startat")
                        .short("s")
                        .long("startat")
                        .value_name("TIMESTAMP")
                        .required(true)
                        .help("Sets start point for playback")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("testfile")
                        .short("t")
                        .long("testfile")
                        .value_name("PATH")
                        .help("Sets path to file to play")
                        .required(true)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("no-correction")
                        .long("no-correction")
                        .help("Disables resampling"),
                ),
        )
        .get_matches();
    if let Some(matches) = matches.subcommand_matches("master") {
        master::main(matches);
    } else if let Some(matches) = matches.subcommand_matches("slave") {
        slave::main(matches);
    }
}
