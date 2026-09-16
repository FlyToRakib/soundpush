# netsim — network impairment profiles

The profiles the simulation tests run against (`sp-testkit`), applied to a real interface so a
desktop and a phone on real hardware see the same link (plan §11.1, §29.1).

    netsim list                         # the profiles and what each one does
    netsim show <profile>               # the tc and clumsy commands for it
    netsim apply <profile> [options]    # Linux: run tc; elsewhere: print what to run
    netsim clear [options]              # remove the qdisc again

Options:

    --interface <name>   Network interface (default: eth0)
    --dry-run            Print the command instead of running it

Examples:

    netsim list
    netsim show congested
    sudo netsim apply wifi-24ghz --interface wlan0
    sudo netsim clear --interface wlan0

On Linux `apply` runs `tc qdisc replace dev <interface> root netem …`, which needs root and the
`iproute2` package. It shapes **outgoing** traffic on that interface, so run it on both machines to
impair both directions.

On Windows there is no command-line shaper: `apply` prints the [clumsy](https://jagt.github.io/clumsy/)
options for the same profile, which you pass to `clumsy.exe` (or set in its window). A profile with
a blackout window is a timed outage, not a qdisc — the simulator applies it, and on real hardware
you reproduce it by unplugging or disabling the interface for that long.

The impairment values are the ones in `sp-testkit`, so `cargo test -p sp-engine --test sim_network`,
`latency-probe --link <profile>` and `netsim apply <profile>` all mean the same thing.
