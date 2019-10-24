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
    scan_displacement: f64, // Optical path difference across one scan cycle in m (use -c).
    scan_decimation: u32,   // Decimation factor for the scan positions.
    scan_frequency: f64,    // Frequency of the scan in Hz.
    scan_speed_tol: f64,    // Fraction of the nominal scan speed that is accepted as deviation
                            // from "smooth" motion.
    scan_blanking: f64,     // Fraction of the scan period that is blanked at the beginning
                            // and end of each slope (4 times per scan period).
}

fn read_config_from_file<P: AsRef<Path>>(path: P) -> Result<Config, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let u = serde_json::from_reader(reader)?;
    Ok(u)
}

fn do_calibrate(config: &Config) {
    let mut sample_count = 0;
    let max_sample_count = (config.sample_rate/4.0) as u32;
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
        if rising & (1 << config.bit_meas) != 0 {
            let position = position_tracker.edge(refpll.get_phase_unwrapped());
            if position > position_max {
                position_max = position;
            }
            if position < position_min {
                position_min = position;
            }
        }

        sample_count += 1;
        if sample_count == max_sample_count {
            let displacement = ((position_max-position_min) as f64)/(noptica::Dpll::TURN as f64)*config.ref_wavelength;
            println!("{} um", 1.0e6*displacement);
            sample_count = 0;
            position_min = i64::max_value();
            position_max = i64::min_value();
        }
    })
}

enum MotionTrackerState {
    BlankingIn(u32),
    Smooth(u32),
    BlankingOut
}

pub struct MotionTracker {
    abs_speed_min: i64,
    abs_speed_max: i64,
    len_blanking_in: u32,
    len_smooth: u32,

    state: MotionTrackerState,
    last_position: i64,
    last_position_ago: u32,
    speed: i64
}

impl MotionTracker {
    pub fn new(abs_speed_min: i64, abs_speed_max: i64, len_blanking_in: u32, len_smooth: u32) -> MotionTracker {
        MotionTracker {
            abs_speed_min: abs_speed_min,
            abs_speed_max: abs_speed_max,
            len_blanking_in: len_blanking_in,
            len_smooth: len_smooth,
            state: MotionTrackerState::BlankingIn(0),
            last_position: 0,
            last_position_ago: 0,
            speed: 0
        }
    }

    fn speed_ok(&self) -> bool {
        let abs_speed = self.speed.abs();
        (self.abs_speed_min < abs_speed) && (abs_speed < self.abs_speed_max)
    }

    pub fn extrapolated_position(&self) -> i64 {
        self.last_position + self.speed*(self.last_position_ago as i64)
    }

    pub fn tick(&mut self, position: Option<i64>, mut callback: impl FnMut(bool)) {
        self.last_position_ago += 1;
        if let Some(position) = position {
            self.speed = (position - self.last_position)/(self.last_position_ago as i64);
            self.last_position = position;
            self.last_position_ago = 0;
        }

        match self.state {
            MotionTrackerState::BlankingIn(count) => {
                let count = if self.speed_ok() { count + 1 } else { 0 };
                if count == self.len_blanking_in {
                    self.state = MotionTrackerState::Smooth(0);
                    callback(true);
                }  else {
                    self.state = MotionTrackerState::BlankingIn(count);
                }
            },
            MotionTrackerState::Smooth(count) => {
                if self.speed_ok() {
                    let count = count + 1;
                    if count == self.len_smooth {
                        self.state = MotionTrackerState::BlankingOut;
                        callback(false);
                    } else {
                        self.state = MotionTrackerState::Smooth(count);
                    }
                } else {
                    // abort
                    self.state = MotionTrackerState::BlankingIn(0);
                }
            },
            MotionTrackerState::BlankingOut => {
                if !self.speed_ok() {
                    self.state = MotionTrackerState::BlankingIn(0);
                }
            }
        }
    }
}

fn do_wavemeter(config: &Config) {
    let mut first_fringe_position: i64 = 0;
    let mut last_fringe_position: i64 = 0;
    let mut fringe_count = 0;
    let mut counting_fringes = false;

    let mut refpll = noptica::Dpll::new(
        noptica::Dpll::frequency_to_ftw(config.ref_min, config.sample_rate),
        noptica::Dpll::frequency_to_ftw(config.ref_max, config.sample_rate),
        config.refpll_ki,
        config.refpll_kp);
    let mut position_tracker = noptica::PositionTracker::new();
    let mut position_decimator = noptica::Decimator::new(config.scan_decimation);

    let abs_speed_nominal = 2.0*config.scan_displacement*config.scan_frequency;
    let abs_speed_min = abs_speed_nominal*(1.0 - config.scan_speed_tol);
    let abs_speed_max = abs_speed_nominal*(1.0 + config.scan_speed_tol);
    let len_blanking_in = 2.0*config.scan_blanking/config.scan_frequency;
    let len_smooth = (1.0 - 4.0*config.scan_blanking)/(2.0*config.scan_frequency);
    let mut motion_tracker = MotionTracker::new(
        (abs_speed_min/config.ref_wavelength*(noptica::Dpll::TURN as f64)) as i64,
        (abs_speed_max/config.ref_wavelength*(noptica::Dpll::TURN as f64)) as i64,
        (len_blanking_in*config.sample_rate) as u32,
        (len_smooth*config.sample_rate) as u32);

    noptica::sample(&config.sample_command, |rising, _falling| {
        refpll.tick(rising & (1 << config.bit_ref) != 0);
        let position = if rising & (1 << config.bit_meas) != 0 {
            position_decimator.input(position_tracker.edge(refpll.get_phase_unwrapped()))
        } else {
            None
        };
        motion_tracker.tick(position,
            |enter| {
                if enter {
                    counting_fringes = true;
                    fringe_count = 0;
                } else {
                    counting_fringes = false;
                    if fringe_count > 1 {
                        let wavelength = (last_fringe_position - first_fringe_position).abs()/(fringe_count - 1);
                        let wavelength_m = (wavelength as f64)/(noptica::Dpll::TURN as f64)*config.ref_wavelength;
                        println!("{:.3} nm", 1.0e9*wavelength_m);
                    }
                }
            });
        if counting_fringes && (rising & (1 << config.bit_input) != 0) {
            let fringe_position = motion_tracker.extrapolated_position();
            if fringe_count == 0 {
                first_fringe_position = fringe_position;
            }
            last_fringe_position = fringe_position;
            fringe_count += 1;
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
