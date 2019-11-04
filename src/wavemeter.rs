extern crate argparse;
extern crate num_traits;
extern crate serde_derive;
extern crate serde_json;

use argparse::{ArgumentParser, StoreTrue, Store};

use serde_derive::Deserialize;

use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

mod noptica;

#[derive(Deserialize, Debug)]
struct Config {
    sample_command: String, // Shell command to start the logic analyzer.
    sample_rate: f64,       // Sample rate of the logic analyzer in Hz.

    // The logic analyzer command must produce a stream of 4-bit nibbles on its
    // standard output, which are continuously sampled at the nominal sample rate.
    // Each of the signals below are mapped to one bit within each nibble.
    bit_ref: u8,            // Bit# for REF signal of the reference laser head (HP 5501B).
    bit_meas: u8,           // Bit# for displacement measurement detector (HP 10780).
    bit_input: u8,          // Bit# for input laser interference detector.

    // The REF DPLL locks to the REF output of the reference laser and provides REF phase
    // information at each sample of the logic analyzer.
    // ref_min and ref_max are used to initialize the DPLL and clamp its NCO frequency.
    ref_min: f64,           // Minimum REF frequency in Hz.
    ref_max: f64,           // Maximum REF frequency in Hz.
    refpll_ki: i64,         // Integration constant of the DPLL loop filter.
    refpll_kp: i64,         // Proportionality constant of the DPLL loop filter.

    ref_wavelength: f64,    // Wavelength of the reference laser in m.
}

fn read_config_from_file<P: AsRef<Path>>(path: P) -> Result<Config, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let u = serde_json::from_reader(reader)?;
    Ok(u)
}

fn do_calibrate(config: &Config) {
    let mut sample_count = 0;
    let avg_ref = (config.ref_min + config.ref_max)/2.0;
    let max_sample_count = (avg_ref/4.0) as u32;
    let mut position_min = i64::max_value();
    let mut position_max = i64::min_value();

    let mut refpll = noptica::Dpll::new(
        noptica::Dpll::frequency_to_ftw(config.ref_min, config.sample_rate),
        noptica::Dpll::frequency_to_ftw(config.ref_max, config.sample_rate),
        config.refpll_ki,
        config.refpll_kp);
    let mut position_tracker = noptica::PositionTracker::new();

    noptica::sample(&config.sample_command, |rising, _falling| {
        refpll.tick(rising & (1 << config.bit_ref) != 0);
        if refpll.locked() {
            if rising & (1 << config.bit_meas) != 0 {
                let position = position_tracker.edge(refpll.get_phase_unwrapped());
                if position > position_max {
                    position_max = position;
                }
                if position < position_min {
                    position_min = position;
                }
                sample_count += 1;
                if sample_count == max_sample_count {
                    let displacement = ((position_max-position_min) as f64)/(noptica::Dpll::TURN as f64)*config.ref_wavelength;
                    println!("{} um", 1.0e6*displacement);
                    sample_count = 0;
                    position_min = i64::max_value();
                    position_max = i64::min_value();
                }
            }
        } else {
            sample_count = 0;
            position_min = i64::max_value();
            position_max = i64::min_value();
        }
    })
}


pub struct MotionTracker {
    last_position: i64,
    last_position_ago: u32,
    speed: i64
}

impl MotionTracker {
    pub fn new() -> MotionTracker {
        MotionTracker {
            last_position: 0,
            last_position_ago: 0,
            speed: 0
        }
    }

    pub fn extrapolated_position(&self) -> i64 {
        self.last_position + self.speed*(self.last_position_ago as i64)
    }

    pub fn tick(&mut self, position: Option<i64>) {
        self.last_position_ago += 1;
        if let Some(position) = position {
            self.speed = (position - self.last_position)/(self.last_position_ago as i64);
            self.last_position = position;
            self.last_position_ago = 0;
        }
    }
}

fn do_wavemeter(config: &Config) {
    let mut refpll = noptica::Dpll::new(
        noptica::Dpll::frequency_to_ftw(config.ref_min, config.sample_rate),
        noptica::Dpll::frequency_to_ftw(config.ref_max, config.sample_rate),
        config.refpll_ki,
        config.refpll_kp);
    let mut position_tracker = noptica::PositionTracker::new();
    let mut motion_tracker = MotionTracker::new();

    noptica::sample(&config.sample_command, |rising, _falling| {
        refpll.tick(rising & (1 << config.bit_ref) != 0);
        if refpll.locked() {
            let position = if rising & (1 << config.bit_meas) != 0 {
                Some(position_tracker.edge(refpll.get_phase_unwrapped()))
            } else {
                None
            };
            motion_tracker.tick(position);
            if rising & (1 << config.bit_input) != 0 {
                let fringe_position = motion_tracker.extrapolated_position();
                println!("{}", (fringe_position as f64)/(noptica::Dpll::TURN as f64));
            }
        }
    })
}

fn main() {
    let mut calibrate = false;
    let mut config_file = "wavemeter.json".to_string();
    {
        let mut ap = ArgumentParser::new();
        ap.refer(&mut calibrate)
            .add_option(&["-c", "--calibrate"], StoreTrue,
            "Calibrate scan displacement");
        ap.refer(&mut config_file)
            .add_option(&["--config"], Store,
            "Configuration file");
        ap.parse_args_or_exit();
    }
    let config = read_config_from_file(config_file).unwrap();
    if calibrate {
        do_calibrate(&config);
    } else {
        do_wavemeter(&config);
    }
}
