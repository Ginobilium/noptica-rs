import sys, os

sample_rate = 48e6
ref_frequency = 1.97e6

ref_phase = 0
ref_ftw = int(ref_frequency*2**32/sample_rate)

fp = os.fdopen(sys.stdout.fileno(), "wb")

while True:
    sample = 0
    for _ in range(2):
        sample <<= 2

        ref_phase = (ref_phase + ref_ftw) & 0xffffffff
        delta = 0
        meas_phase = (ref_phase + delta) & 0xffffffff

        if ref_phase >= 0x80000000:
            sample |= 0x01
        if meas_phase >= 0x80000000:
            sample |= 0x02
    fp.write(bytes([sample]))
