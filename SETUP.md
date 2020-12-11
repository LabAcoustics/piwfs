## How to run PiWFS

This guide will take you step-by-step through the process of running
synchronized playback on distributed devices using the PiWFS system.

All devices used in the system should be connected in the same LAN, preferably
through a cable connection, as this will increase precision. The simplest form
of the system has two playback devices (e.g. Raspberry Pis) and a PTP
Grandmaster device (see next chapter) all connected to the same network switch.

This guide assumes all your devices run Linux as PiWFS is supported only on Linux.

# PTP setup

PiWFS requires all of the playback devices' clock to be synchronized, the most
readily available way of preciese network clock synchronization is PTP (Precise
Time Protocol), until PiWFS has its own implementation it is required to run
PTP software on every playback system. Additionaly you will need a device that
will act as a PTP Grandmaster, it can be one of the playback devices, however
if your playback devices are Raspberry Pis, using some other device that
supports harwdare-based timestamping is prefelable (for example a laptop with a
modern enough NIC), to check if your device suuports HW timestamping run `sudo
ethtool -T <network interface>` if you see `hardware-transmit` and
`hardware-receive` in capabilities then great, you've got it, otherwise don't
worry, you can still run PiWFS but it will be a bit less precies.

1. Install `linuxptp` on all of your playback devices and on your Grandmaster
   (e.g. `sudo apt install linuxptp`)
2. Configure PTP on your devices via the config file `/etc/ptp4l.conf` or
   `/etc/linuxptp/ptp4l.conf`. Change `logSyncInterval` to a smaller value,
   depending on your network infrastructure, we recommend a value of `-2` for a
   small system which means that a Sync message is sent every 10^-2 seconds.
   Additionaly we recommend changing `tsproc_mode` to `filter_weight` and
   `delay_filter` to `moving_median`, we found these values to be more
   resistant to network jitter. You can also configure `time_stamping` to be
   either `software` or `hardware` depending on capabilities of your NIC, or
   you can add `-S` or `-H` switch to `ptp4l` commandline later.
3. Run `ptp4l` on all of your devices starting with the Grandmaster via `sudo
   ptp4l -i <network interface> -f <path to config file> -m`, you should use a
   terminal multiplexer such as GNU Screen or Tmux to avoid having to keep many
   SSH connections open.
4. Check if all of your playback devices have succesfully connected with the
   Grandmaster and synchronized, you should see a `UNCALIBRATED to SLAVE on
   MASTER_CLOCK_SELECTED` printed and subsequently you should see current
   master offset printed every second, the printed values should start becoming
   respectively small (on the order of milliseconds) after a few moments.


# Playback setup

To compile PiWFS you need Rust istalled (see [rustup](https://rustup.rs/) if
you dont know how to install it) then download this repo and execute `cargo
build` this should donwload and compile all dependecies and the final
executable.

Every playback device needs to have the `piwfs` executable as well as a audio
file in a WAV format (currently only 16-bit is supported, we do not recommend
using higher sampling frequency than 48 kHz as this can greatly inrease
processing power required).

1. Obtain a starting time by running `echo (echo 10^9'*(10+'(date +%s)')' |
   bc)` on one of the devices and copying the obtained value, this will provide
   a timestamp at which the playback should be started which is 10 seconds from
   now. Now you have 10 seconds to start all af playback devices.

2. Run `piwfs` on every playback device (see `piwfs --help` for usage), for
   example `sudo piwfs slave --testfile <path to WAV file> --startat <startig
   timestamp>`, you will see diagnostic information printed on your terminal
   which will tell you how good is the synchronization. We recommend running
   piwfs in a terminal multiplexer as well.

3. Everything ready, playback should start at the provided timestamp and should
   be synchronized, you can tweak `piwfs` and `ptp4l` parameters to see which
   work for your setup the best.
