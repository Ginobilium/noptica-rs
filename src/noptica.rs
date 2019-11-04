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

    was_locked: bool,
    wait_lock: Option<u32>
}

impl Dpll {
    pub const TURN: i64 = 0x100000000;  // One turn in DPLL phase units.

    pub fn new(ftw_min: i64, ftw_max: i64, ki: i64, kp: i64) -> Dpll {
        assert!(ftw_min < Dpll::TURN/2);
        assert!(ftw_max < Dpll::TURN/2);
        assert!(Dpll::TURN & (Dpll::TURN - 1) == 0);  // must be a power of 2
        let init_ftw = (ftw_min + ftw_max)/2;
        Dpll {
            ftw_min: ftw_min,
            ftw_max: ftw_max,
            ki: ki,
            kp: kp,
            ftw: init_ftw,
            integrator: init_ftw,
            phase: 0,
            phase_unwrapped: 0,
            was_locked: false,
            wait_lock: Some(0)
        }
    }

    pub fn frequency_to_ftw(frequency: f64, sample_rate: f64) -> i64 {
        (frequency*(Dpll::TURN as f64)/sample_rate) as i64
    }

    pub fn tick(&mut self, edge: bool) {
        self.phase = (self.phase + self.ftw) & (Dpll::TURN - 1);
        self.phase_unwrapped = self.phase_unwrapped.wrapping_add(self.ftw);
        if edge {
            let pe = Dpll::TURN/2 - self.phase;
            self.integrator = clamp(self.integrator + (pe*self.ki >> 32),
                self.ftw_min, self.ftw_max);
            self.ftw = clamp(self.integrator + (pe*self.kp >> 32),
                self.ftw_min, self.ftw_max);

            if pe.abs() <= (self.ftw + self.ftw/3) {
                if let Some(wait_lock) = self.wait_lock {
                    if wait_lock < 1000000 {
                        self.wait_lock = Some(wait_lock + 1);
                    } else {
                        self.wait_lock = None;
                    }
                }
            } else {
                self.wait_lock = Some(0);
            }

            if self.locked() & !self.was_locked {
                eprintln!("DPLL locked");
            }
            if !self.locked() & self.was_locked {
                eprintln!("DPLL lost lock");
            }
            self.was_locked = self.locked();
        }
    }

    pub fn get_phase_unwrapped(&self) -> i64 {
        self.phase_unwrapped
    }

    pub fn locked(&self) -> bool {
        self.wait_lock.is_none()
    }
}

pub struct PositionTracker {
    last_phase: i64,
    current_position: i64
}

impl PositionTracker {
    pub fn new() -> PositionTracker {
        PositionTracker {
            last_phase: 0,
            current_position: 0
        }
    }

    pub fn edge(&mut self, phase: i64) -> i64 {
        let phase_diff = phase.wrapping_sub(self.last_phase);
        self.last_phase = phase;
        self.current_position += Dpll::TURN - phase_diff;
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
    let mut buffer = [0; 1];
    let mut last_sample = 0;
    loop {
        reader.read_exact(&mut buffer).unwrap();
        for shift in [4u8, 0u8].iter() {
            let sample = (buffer[0] >> shift) & 0x0f;
            let rising = sample & !last_sample;
            let falling = !sample & last_sample;
            callback(rising, falling);
            last_sample = sample;
        }
    }
}
