use std::io::{BufReader, Read};
use num_traits::clamp;

pub struct Dpll {
    ftw_min: i64,
    ftw_max: i64,
    ki: i64,
    kp: i64,

    ftw: i64,
    integrator: i64,

    phase: i64,
    phase_unwrapped: i64,
}

impl Dpll {
    pub fn new(ftw_min: i64, ftw_max: i64, ki: i64, kp: i64) -> Dpll {
        assert!(ftw_min < 0x80000000);
        assert!(ftw_max < 0x80000000);
        let init_ftw = (ftw_min + ftw_max)/2;
        Dpll {
            ftw_min: ftw_min,
            ftw_max: ftw_max,
            ki: ki,
            kp: kp,
            ftw: init_ftw,
            integrator: init_ftw,
            phase: 0,
            phase_unwrapped: 0
        }
    }

    pub fn frequency_to_ftw(frequency: f64, sample_rate: f64) -> i64 {
        (frequency*(u32::max_value() as f64)/sample_rate) as i64
    }

    pub fn tick(&mut self, edge: bool) {
        self.phase = (self.phase + self.ftw) & 0xffffffff;
        self.phase_unwrapped = self.phase_unwrapped.wrapping_add(self.ftw);
        if edge {
            let pe = 0x80000000 - self.phase;
            self.integrator = clamp(self.integrator + (pe*self.ki >> 32),
                self.ftw_min, self.ftw_max);
            self.ftw = clamp(self.integrator + (pe*self.kp >> 32),
                self.ftw_min, self.ftw_max);
        }
    }

    pub fn get_phase_unwrapped(&self) -> i64 {
        self.phase_unwrapped
    }
}

pub struct Tracker {
    last_phase: i64,
    current_position: i64
}

impl Tracker {
    pub fn new() -> Tracker {
        Tracker {
            last_phase: 0,
            current_position: 0
        }
    }

    pub fn edge(&mut self, phase: i64) -> i64 {
        let phase_diff = phase.wrapping_sub(self.last_phase);
        self.last_phase = phase;
        self.current_position += 0x100000000 - phase_diff;
        self.current_position
    }
}

pub struct Decimator {
    accumulator: i64,
    current_count: u32,
    max_count: u32
}

impl Decimator {
    pub fn new(max_count: u32) -> Decimator {
        Decimator {
            accumulator: 0,
            current_count: 0,
            max_count: max_count
        }
    }

    pub fn input(&mut self, data: i64) -> Option<i64> {
        self.accumulator += data;
        self.current_count += 1;
        if self.current_count == self.max_count {
            let average = self.accumulator/(self.current_count as i64);
            self.accumulator = 0;
            self.current_count = 0;
            Some(average)
        } else {
            None
        }
    }
}

pub fn sample(command: &str, mut callback: impl FnMut(u8, u8)) {
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();  
    let mut reader = BufReader::new(child.stdout.unwrap());
    let mut br_sample = [0; 1];
    let mut last_sample = 0;
    loop {
        reader.read_exact(&mut br_sample).unwrap();
        let sample = br_sample[0];
        let rising = sample & !last_sample;
        let falling = !sample & last_sample;
        callback(rising, falling);
        last_sample = sample;
    }
}
