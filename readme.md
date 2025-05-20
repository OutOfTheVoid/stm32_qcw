STM32 QCWDRSSTC Controller
==========================

Pin mappings:
-------------
* C6  - phase 1 non-inverted output
* C7  - phase 1 inverted output
* A9  - phase 2 non-inverted output
* A10 - phase 2 inverted output
* C5  - feedback input
* A6  - current feedback

Progress
--------
* closed loop mode locks consistently
* controllable phase delay for ZCS
* controllable conduction angle
* fiber serial control
* overcurrent protection

Todo
----
* low-power Frequency sweep for finding initial resonance
* general run mode
* ramping and finer control
* configurable and live-updatable ramp profiles for eg music
