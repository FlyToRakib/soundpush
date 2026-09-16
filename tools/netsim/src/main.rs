//! Network impairment profiles (plan §11.1 `tools/netsim`).
//!
//! The same [`Impairments`] the simulation tests use, rendered as `tc netem` arguments (Linux) or
//! clumsy options (Windows) and — on Linux — applied to a real interface. One source of truth, so a
//! simulated run and a real run are impaired the same way.
//!
//! See `tools/netsim/README.md` for usage.

use std::process::{Command, ExitCode};

use sp_testkit::net::{Impairments, PROFILES};

type Error = Box<dyn std::error::Error>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("netsim: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    interface: String,
    dry_run: bool,
}

fn run() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "--help".into());
    let mut profile = None;
    let mut o = Options {
        interface: "eth0".into(),
        dry_run: false,
    };
    let mut rest: Vec<String> = args.collect();
    // The profile name, when there is one, comes before the options.
    if matches!(command.as_str(), "show" | "apply") {
        profile = Some(
            rest.first()
                .cloned()
                .ok_or_else(|| format!("{command} needs a profile name (see `netsim list`)"))?,
        );
        rest.remove(0);
    }
    let mut it = rest.into_iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--interface" | "-i" => o.interface = value()?,
            "--dry-run" => o.dry_run = true,
            other => return Err(format!("unknown argument {other} (see --help)").into()),
        }
    }

    match command.as_str() {
        "list" => list(),
        "show" => show(
            &named(profile.as_deref())?,
            profile.as_deref().unwrap_or(""),
        ),
        "apply" => apply(
            &named(profile.as_deref())?,
            &o,
            profile.as_deref().unwrap_or(""),
        ),
        "clear" => clear(&o),
        "--help" | "-h" | "help" => {
            println!("{}", include_str!("../README.md"));
            Ok(())
        }
        other => Err(format!("unknown command {other} (see --help)").into()),
    }
}

fn named(profile: Option<&str>) -> Result<Impairments, Error> {
    let name = profile.unwrap_or_default();
    Impairments::named(name)
        .ok_or_else(|| format!("unknown profile {name} (see `netsim list`)").into())
}

fn list() -> Result<(), Error> {
    for (name, description) in PROFILES {
        let i = Impairments::named(name).unwrap_or_default();
        println!("{name:<12} {description}");
        println!(
            "{:<12}   loss {:.1} %, delay {} ms + 0..{} ms, duplicate {:.1} %{}",
            "",
            i.loss * 100.0,
            i.delay.as_millis(),
            i.jitter.as_millis(),
            i.duplicate * 100.0,
            match i.blackout {
                Some((from, to)) =>
                    format!(", blackout {}..{} s", from.as_secs_f32(), to.as_secs_f32()),
                None => String::new(),
            }
        );
    }
    Ok(())
}

fn show(i: &Impairments, name: &str) -> Result<(), Error> {
    println!("{name}");
    println!(
        "  tc:     tc qdisc replace dev <interface> root {}",
        i.netem_args().join(" ")
    );
    let clumsy = i.clumsy_args();
    if clumsy.is_empty() {
        println!("  clumsy: nothing to set (a clean link)");
    } else {
        println!("  clumsy: clumsy.exe --filter \"udp\" {}", clumsy.join(" "));
    }
    if let Some((from, to)) = i.blackout {
        println!(
            "  outage: disable the interface from {} s to {} s after the stream starts",
            from.as_secs_f32(),
            to.as_secs_f32()
        );
    }
    Ok(())
}

fn apply(i: &Impairments, o: &Options, name: &str) -> Result<(), Error> {
    let mut args = vec![
        "qdisc".to_string(),
        "replace".into(),
        "dev".into(),
        o.interface.clone(),
        "root".into(),
    ];
    args.extend(i.netem_args());
    if !cfg!(target_os = "linux") {
        println!("netsim can only drive tc, which is Linux-only. For this platform:");
        return show(i, name);
    }
    tc(&args, o)
}

fn clear(o: &Options) -> Result<(), Error> {
    if !cfg!(target_os = "linux") {
        println!("Nothing to clear: tc is Linux-only.");
        return Ok(());
    }
    tc(
        &[
            "qdisc".into(),
            "del".into(),
            "dev".into(),
            o.interface.clone(),
            "root".into(),
        ],
        o,
    )
}

fn tc(args: &[String], o: &Options) -> Result<(), Error> {
    println!("tc {}", args.join(" "));
    if o.dry_run {
        return Ok(());
    }
    let status = Command::new("tc")
        .args(args)
        .status()
        .map_err(|e| format!("could not run tc ({e}); install iproute2 and run as root"))?;
    if !status.success() {
        return Err(format!("tc exited with {status}").into());
    }
    Ok(())
}
