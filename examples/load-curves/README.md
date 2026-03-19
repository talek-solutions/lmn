# Load Curve Examples

Used with `-L` / `--load-curve`. Each file defines a sequence of stages that scale virtual users (VUs) over time.

| File | Load pattern |
|------|-------------|
| `ramp.json` | Gradual ramp up to peak VUs, hold, then ramp down |
| `morning-ramp.json` | Simulates a morning traffic surge with a slow start |
| `soak.json` | Sustained constant load for endurance testing |
| `spike.json` | Sudden traffic spike to test burst recovery |
| `stepped.json` | Step-function increases to find the concurrency degradation point |
| `stress.json` | Progressive overload to find the breaking point |
