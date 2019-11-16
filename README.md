The Noptica Wavemeter
=====================

An open source laser wavemeter with NO expensive optics and NO machining.

Introduction
------------

Traveling Michelson wavemeters are one of the simplest designs that one can build. However, a high-resolution wavemeter requires the travel of a cube corner over a long range of motion. This motion needs to be smooth and vibration-free, which makes the mechanical design and construction rather difficult, and maintaining alignment over the whole range of travel is also challenging.

This design avoids those difficulties by restricting the range of motion to about a millimeter, which allows a low-cost voice coil to be used as actuator, such as an actual audio speaker with the corner cube glued in the middle.

While this reduced range of motion corresponds to a similar reduction in resolution in a traditional Michelson wavemeter, this design compensates by using the following techniques:
 
1. The position of the fringes of the unknown laser is measured precisely by using a two-frequency HeNe laser as the reference, in a displacement measurement interferometer (DMI) configuration.
2. A DMI only reports the position of the moving cube corner at the MEAS edges, and those edges may not coincide with the fringes of the input laser. In this design, the position is then extrapolated using a low-pass IIR filter. The filter also attenuates other sources of noise, such as quantization noise.
3. The scanning rate (around 50Hz) is higher than that of regular Michelson wavemeters, and many output measurements are averaged.

Current status
--------------

* Prototype built, wavelength output (with unstabilized HeNe tube as input) is stable at <2pm level.
* HP 5501B lasers do not like light (at any wavelength) sent into their aperture; the stabilization circuit fails and the REF output becomes wrong. Workaround is to misalign the input beam, which obviously introduces cosine error. A better solution is needed, maybe rebuild a 2-frequency HeNe but use the waste beam of the tube for REF/intensity measurements unlike the HP design. The HeNe tube itself attenuates incoming light.
