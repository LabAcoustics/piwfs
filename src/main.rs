use clap::{App, Arg, SubCommand};

mod indicator;
mod master;
mod slave;

fn main() {
    let matches = App::new("piwfs")
        .version("0.2.2")
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
                .version("0.2.2")
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
                    Arg::with_name("desync-avg")
                        .long("desync-avg")
                        .short("a")
                        .value_name("AVG_SIZE")
                        .help("Sets length of moving average for desync calculation")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("estimation-avg")
                        .long("estimation-avg")
                        .value_name("AVG_SIZE")
                        .help("Sets length of moving average for sample length estimation")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("quality")
                        .short("q")
                        .long("quality")
                        .value_name("SINC_SAMPLES")
                        .help("Interpolation quality")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("no-correction")
                        .long("no-correction")
                        .help("Disables resampling"),
                )
                .arg(
                    Arg::with_name("no-spinning")
                        .long("no-spinning")
                        .help("Disables spinning for multiple pcm statuses"),
                )
                .arg(
                    Arg::with_name("no-estimation")
                        .long("no-estimation")
                        .help("Disables sample length estimation"),
                ),
        )
        .get_matches();
    if let Some(matches) = matches.subcommand_matches("master") {
        master::main(matches);
    } else if let Some(matches) = matches.subcommand_matches("slave") {
        slave::main(matches);
    }
}
