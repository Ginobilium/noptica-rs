#![feature(generators, generator_trait)]

extern crate argparse;
extern crate num_traits;
extern crate serde_derive;
extern crate serde_json;
extern crate biquad;

use argparse::{ArgumentParser, StoreTrue, Store};
use serde_derive::Deserialize;
use biquad::Biquad;

use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::ops::{Generator, GeneratorState};
use std::pin::Pin;
use std::cell::Cell;

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
    duty_cycle: f64,        // Fraction of the scan used for counting input laser fringes

    debug: bool,            // Enable debug output of wavelength determination code
    motion_cutoff: f64,     // Cut-off frequency of the motion filter
    min_fringes: u32,       // Minimum number of fringes to count
    fringe_jitter_tol: f64, // Tolerance for fringe distance jitter
    decimation: u32,        // Decimation/averaging factor for the final wavelength output
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Quadrant {
    BelowMin,
    Up,
    AboveMax,
    Down
}

#[derive(Clone)]
struct QuadrantTracker {
    prev_state: Quadrant,
    state: Quadrant,
    min: i64,
    max: i64,
    new_min: i64,
    new_max: i64,
    prev_above_middle: bool,
    middle: i64,
}

impl QuadrantTracker {
    pub fn new() -> QuadrantTracker {
        QuadrantTracker {
            prev_state: Quadrant::BelowMin,
            state: Quadrant::BelowMin,
            min: i64::max_value(),
            max: i64::min_value(),
            new_min: i64::max_value(),
            new_max: i64::min_value(),
            prev_above_middle: false,
            middle: i64::max_value(),
        }
    }

    pub fn reset(&mut self) {
        *self = QuadrantTracker::new();
    }

    pub fn input(&mut self, position: i64) {
        let above_min = position > self.min;  // always false before init
        let below_max = position < self.max;  // always false before init
        let next_state;
        if above_min && below_max {
            next_state = match self.state {
                Quadrant::BelowMin => Quadrant::Up,
                Quadrant::Up => Quadrant::Up,
                Quadrant::AboveMax => Quadrant::Down,
                Quadrant::Down => Quadrant::Down
            }
        } else {
            if above_min {
                next_state = Quadrant::AboveMax;
            } else {
                next_state = Quadrant::BelowMin;  // stays here before init
            }
        }

        self.prev_state = self.state;
        if self.state != next_state {
            match (self.state, next_state) {
                (Quadrant::BelowMin, Quadrant::Up) => (),
                (Quadrant::Up, Quadrant::AboveMax) => (),
                (Quadrant::AboveMax, Quadrant::Down) => (),
                (Quadrant::Down, Quadrant::BelowMin) => (),
                _ => eprintln!("invalid quadrant transition: {:?} -> {:?}",
                    self.state, next_state)
            }
            self.state = next_state;
        }


        // Update min and max when the position is near the middle
        // to avoid glitches.
        let above_middle = position > self.middle;  // always false before init
        if above_middle && !self.prev_above_middle {
            self.min = self.new_min;
            self.max = self.new_max;
        }
        self.prev_above_middle = above_middle;
    }

    pub fn update_limits(&mut self, min: i64, max: i64) {
        self.new_min = min;
        self.new_max = max;
        self.middle = (min + max)/2;
    }

    pub fn up_start(&self) -> bool {
        self.prev_state == Quadrant::BelowMin && self.state == Quadrant::Up
    }

    pub fn up_end(&self) -> bool {
        self.prev_state == Quadrant::Up && self.state == Quadrant::AboveMax
    }

    pub fn down_start(&self) -> bool {
        self.prev_state == Quadrant::AboveMax && self.state == Quadrant::Down
    }

    pub fn down_end(&self) -> bool {
        self.prev_state == Quadrant::Down && self.state == Quadrant::BelowMin
    }
}

#[derive(Debug, Clone, Copy)]
enum FringeCounterEvent {
    Start,
    Fringe(i64),
    End,
}

macro_rules! generator_input {
    ($e:expr) => ({ yield (); $e.get() })
}

fn do_wavemeter(config: &Config) {
    let mut refpll = noptica::Dpll::new(
        noptica::Dpll::frequency_to_ftw(config.ref_min, config.sample_rate),
        noptica::Dpll::frequency_to_ftw(config.ref_max, config.sample_rate),
        config.refpll_ki,
        config.refpll_kp);
    let mut position_tracker = noptica::PositionTracker::new();
    let mut position = 0;
    let motion_filter_coeffs = biquad::Coefficients::<f64>::from_params(
        biquad::Type::LowPass,
        biquad::frequency::Hertz::<f64>::from_hz(config.sample_rate).unwrap(),
        biquad::frequency::Hertz::<f64>::from_hz(config.motion_cutoff).unwrap(),
        biquad::Q_BUTTERWORTH_F64).unwrap();
    let mut motion_filter = biquad::DirectForm2Transposed::<f64>::new(motion_filter_coeffs);
    let mut min_max_monitor = MinMaxMonitor::new((config.sample_rate*config.position_mon_time) as u32);
    let mut quadrant_tracker = QuadrantTracker::new();

    let fringe_counter_input = Cell::new(FringeCounterEvent::Start);
    let mut fringe_counter = || {
        'outer: loop {
            loop {
                if let FringeCounterEvent::Start = generator_input!(fringe_counter_input) {
                    break;
                }
            }

            let mut boundary_fringes = [0i64; 4];
            for i in 0..4 {
                if let FringeCounterEvent::Fringe(position) = generator_input!(fringe_counter_input) {
                    boundary_fringes[i] = position;
                } else {
                    eprintln!("unexpected event (boundary fringe acquisition)");
                    continue 'outer;
                }
            }

            let mut fringes_between_boundary = 0;
            loop {
                match generator_input!(fringe_counter_input) {
                    FringeCounterEvent::Start => {
                        eprintln!("unexpected event (initial fringe counting)");
                        continue 'outer;
                    },
                    FringeCounterEvent::Fringe(position) => {
                        boundary_fringes[2] = boundary_fringes[3];
                        boundary_fringes[3] = position;
                        fringes_between_boundary += 1;
                    },
                    FringeCounterEvent::End => break,
                }
            }

            if fringes_between_boundary < config.min_fringes {
                eprintln!("insufficient fringes between boundary ({})", fringes_between_boundary);
                continue 'outer;
            }

            let nominal_distance = boundary_fringes[1] - boundary_fringes[0];
            let jitter_tol = ((nominal_distance as f64)*config.fringe_jitter_tol) as i64;

            let limit1 = (boundary_fringes[0] + boundary_fringes[1])/2;
            let limit2 = (boundary_fringes[2] + boundary_fringes[3])/2;
            let expected_fringes = fringes_between_boundary + 2;
            let mut f1_acc = boundary_fringes[1];
            let mut f2_acc = boundary_fringes[2];

            for _ in 0..config.decimation-1 {
                loop {
                    if let FringeCounterEvent::Start = generator_input!(fringe_counter_input) {
                        break;
                    }
                }
                let mut last_fringe: Option<i64> = None;
                let mut count: u32 = 0;
                loop {
                    match generator_input!(fringe_counter_input) {
                        FringeCounterEvent::Start => {
                            eprintln!("unexpected event (secondary fringe counting)");
                            continue 'outer;
                        },
                        FringeCounterEvent::Fringe(position) => {
                            if (position > limit1) && (position < limit2)
                                    || (position > limit2) && (position < limit1) {
                                if let Some(last_fringe) = last_fringe {
                                    let distance = position - last_fringe;
                                    if (distance - nominal_distance).abs() > jitter_tol {
                                        eprintln!("distance between fringes above tolerance (got {}, nominal {})",
                                            distance, nominal_distance);
                                        continue 'outer;
                                    }
                                }
                                last_fringe = Some(position);
                                count += 1;
                                if count == 1 {
                                    f1_acc += position;
                                }
                            }
                        },
                        FringeCounterEvent::End => break,
                    }
                }
                if count == expected_fringes {
                    f2_acc += last_fringe.unwrap();
                } else {
                    eprintln!("unexpected fringe count (got {}, expected {})", count, expected_fringes);
                    continue 'outer;
                }
            }

            let f1_avg = f1_acc/(config.decimation as i64);
            let f2_avg = f2_acc/(config.decimation as i64);
            let wavelength = (f2_avg - f1_avg).abs()/(expected_fringes as i64 - 1);
            let wavelength_nm = (wavelength as f64)*config.ref_wavelength/(noptica::Dpll::TURN as f64);
            println!("{:.4}", wavelength_nm*1.0e9);
        }

    };
    let mut fringe_counter_event = |event: FringeCounterEvent| {
        fringe_counter_input.set(event);
        Pin::new(&mut fringe_counter).resume();
    };

    noptica::sample(&config.sample_command, |rising, _falling| {
        refpll.tick(rising & (1 << config.bit_ref) != 0);
        if refpll.locked() {
            if rising & (1 << config.bit_meas) != 0 {
                position = position_tracker.edge(refpll.get_phase_unwrapped());
            }
            let f_position = motion_filter.run(position as f64) as i64;
            min_max_monitor.input(f_position, |position_min, position_max| {
                let amplitude = position_max - position_min;
                let off_duty = ((amplitude as f64)*(1.0 - config.duty_cycle)) as i64;
                quadrant_tracker.update_limits(
                    position_min + off_duty/2,
                    position_max - off_duty/2);
            });
            quadrant_tracker.input(f_position);
            if quadrant_tracker.up_start() {
                fringe_counter_event(FringeCounterEvent::Start);
            }
            if quadrant_tracker.up_end() {
                fringe_counter_event(FringeCounterEvent::End);
            }
            if rising & (1 << config.bit_input) != 0 {
                fringe_counter_event(FringeCounterEvent::Fringe(position));
            }
        } else {
            position = 0;
            min_max_monitor.reset();
            quadrant_tracker.reset();
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
