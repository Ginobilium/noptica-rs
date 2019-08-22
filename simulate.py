import sys, os

sample_rate = 24e6
ref_frequency = 2.0e6


ref_phase = 0
ref_ftw = int(ref_frequency*2**32/sample_rate)

fp = os.fdopen(sys.stdout.fileno(), "wb")

while True:
    ref_phase = (ref_phase + ref_ftw) & 0xffffffff

    sample = 0
    if ref_phase >= 0x80000000:
        sample |= 0x01
    fp.write(bytes([sample]))
