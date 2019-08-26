extern crate num_traits;
extern crate serde_derive;
extern crate serde_json;

use serde_derive::Deserialize;

use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

mod noptica;

#[derive(Deserialize, Debug)]
struct Config {
    sample_command: String,
    sample_rate: f64,
    freq_min: f64,
    freq_max: f64,
    bit_ref: u8,
    bit_meas: u8,
    refpll_ki: i64,
    refpll_kl: i64
}

fn read_config_from_file<P: AsRef<Path>>(path: P) -> Result<Config, Box<Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let u = serde_json::from_reader(reader)?;
    Ok(u)
}

fn main() {
    let config = read_config_from_file("config.json").unwrap();
    let mut refpll = noptica::Dpll::new(
        noptica::Dpll::frequency_to_ftw(config.freq_min, config.sample_rate),
        noptica::Dpll::frequency_to_ftw(config.freq_max, config.sample_rate),
        config.refpll_ki,
        config.refpll_kl);
    let mut tracker = noptica::Tracker::new();
    let mut decimator = noptica::Decimator::new(200000);
    noptica::sample(&config.sample_command, |rising, _falling| {
        refpll.tick(rising & (1 << config.bit_ref) != 0);
        if rising & (1 << config.bit_meas) != 0 {
            let position = tracker.edge(refpll.get_phase_unwrapped());
            if let Some(position_avg) = decimator.input(position) {
                println!("{}", position_avg);
            }
        }
    })
}
