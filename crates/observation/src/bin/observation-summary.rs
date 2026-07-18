fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        eprintln!("usage: observation-summary <run-directory>");
        std::process::exit(2);
    }

    let reader = observation::RunReader::open(&args[0])?;
    let run = reader.load()?;
    print!("{}", observation::summarize(&run));
    Ok(())
}
