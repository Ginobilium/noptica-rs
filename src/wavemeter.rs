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

    position_mon_time: f64, // The time during which position is monitored to compute min/max
    duty_cycle: f64         // Fraction of the scan used for counting input laser fringes
}

fn read_config_from_file<P: AsRef<Path>>(path: P) -> Result<Config, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let u = serde_json::from_reader(reader)?;
    Ok(u)
}

struct MinMaxMonitor {
    cycle_sample_count: u32,
    current_sample_count: u32,
    current_position_min: i64,
    current_position_max: i64,
}

impl MinMaxMonitor {
    pub fn new(cycle_sample_count: u32) -> MinMaxMonitor {
        MinMaxMonitor {
            cycle_sample_count: cycle_sample_count,
            current_sample_count: 0,
            current_position_min: i64::max_value(),
            current_position_max: i64::min_value(),
            /*position_min: i64::max_value(),
            position_max: i64::min_value(),
            // Trick: position > position_middle is always false before the first monitor cycle.
            position_middle: i64::max_value(),*/
        }
    }

    pub fn reset(&mut self) {
        self.current_sample_count = 0;
        self.current_position_min = i64::max_value();
        self.current_position_max = i64::min_value();
    }

    pub fn input(&mut self, position: i64, mut callback: impl FnMut(i64, i64)) {
        if position > self.current_position_max {
            self.current_position_max = position;
        }
        if position < self.current_position_min {
            self.current_position_min = position;
        }
        self.current_sample_count += 1;
        if self.current_sample_count == self.cycle_sample_count {
            callback(self.current_position_min, self.current_position_max);
            self.reset();
        }
    }
}

fn do_calibrate(config: &Config) {
    let mut refpll = noptica::Dpll::new(
        noptica::Dpll::frequency_to_ftw(config.ref_min, config.sample_rate),
        noptica::Dpll::frequency_to_ftw(config.ref_max, config.sample_rate),
        config.refpll_ki,
        config.refpll_kp);
    let mut position_tracker = noptica::PositionTracker::new();
    let mut min_max_monitor = MinMaxMonitor::new(
        ((config.ref_min + config.ref_max)/2.0*config.position_mon_time) as u32);

    noptica::sample(&config.sample_command, |rising, _falling| {
        refpll.tick(rising & (1 << config.bit_ref) != 0);
        if refpll.locked() {
            if rising & (1 << config.bit_meas) != 0 {
                let position = position_tracker.edge(refpll.get_phase_unwrapped());
                min_max_monitor.input(position, |min, max| {
                    let displacement = ((max - min) as f64)/(noptica::Dpll::TURN as f64)*config.ref_wavelength;
                    println!("{:.1} um", 1.0e6*displacement);
                });
            }
        } else {
            min_max_monitor.reset();
        }
    })
}


struct MotionTracker {
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
    let mut min_max_monitor = MinMaxMonitor::new(
        ((config.ref_min + config.ref_max)/2.0*config.position_mon_time) as u32);

    // Update duty_min and duty_max when the position is near the middle
    // to avoid glitches.
    let mut prev_position_above_middle = false;
    // Trick: position > position_middle is always false before the first monitor cycle.
    let mut position_middle = i64::max_value();
    let mut new_duty_min = i64::max_value();
    let mut new_duty_max = i64::min_value();

    let mut duty_min = i64::max_value();
    let mut duty_max = i64::min_value();
    let mut prev_in_duty = false;

    let mut first_fringe = 0;
    let mut fringe_count = 0;

    noptica::sample(&config.sample_command, |rising, _falling| {
        refpll.tick(rising & (1 << config.bit_ref) != 0);
        if refpll.locked() {
            let position_opt;
            if rising & (1 << config.bit_meas) != 0 {
                let position = position_tracker.edge(refpll.get_phase_unwrapped());
                min_max_monitor.input(position, |position_min, position_max| {
                    let amplitude = position_max - position_min;
                    let off_duty = ((amplitude as f64)*(1.0 - config.duty_cycle)) as i64;
                    new_duty_min = position_min + off_duty/2;
                    new_duty_max = position_max - off_duty/2;
                    position_middle = (position_max + position_min)/2;
                });
                let position_above_middle = position > position_middle;
                if !position_above_middle && prev_position_above_middle {
                    duty_min = new_duty_min;
                    duty_max = new_duty_max;
                }
                prev_position_above_middle = position_above_middle;
                position_opt = Some(position);
            } else {
                position_opt = None
            };
            motion_tracker.tick(position_opt);

            if rising & (1 << config.bit_input) != 0 {
                let fringe_position = motion_tracker.extrapolated_position();
                let in_duty = (duty_min < fringe_position) && (fringe_position < duty_max);
                if in_duty & !prev_in_duty {
                    first_fringe = fringe_position;
                    fringe_count = 0;
                }
                if !in_duty & prev_in_duty {
                    let wavelength = (fringe_position - first_fringe).abs()/fringe_count;
                    let displacement = ((fringe_position - first_fringe).abs() as f64)/(noptica::Dpll::TURN as f64)*config.ref_wavelength;
                    println!("{:.4} {} {} {:.1}",
                        (wavelength as f64)/(noptica::Dpll::TURN as f64)*1.0e9*config.ref_wavelength,
                        fringe_count,
                        if fringe_position > first_fringe { "UP  " } else { "DOWN" },
                        1.0e9*displacement);
                    fringe_count = 0;
                }
                fringe_count += 1;
                prev_in_duty = in_duty;
            }
        } else {
            min_max_monitor.reset();

            prev_position_above_middle = false;
            position_middle = i64::max_value();
            new_duty_min = i64::max_value();
            new_duty_max = i64::min_value();

            duty_min = i64::max_value();
            duty_max = i64::min_value();
            prev_in_duty = false;
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
