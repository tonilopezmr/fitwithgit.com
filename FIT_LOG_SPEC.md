# fit.log Format Specification

A compact, line-based text format for storing fitness activity data. Designed for git — every line is a self-contained record, producing clean diffs when appended.

## Line Format

```
<code>,<YYMMDD>,<field1>,<field2>,...
```

- **One record per line**, comma-separated, no spaces
- Lines starting with `#` are comments; empty lines are ignored
- Records are ordered chronologically, newest at the bottom
- Floats use at most 1 decimal place

## Date Encoding

`YYMMDD` — 6 characters, two-digit year (2000–2099).

| YYMMDD   | Date         |
|----------|--------------|
| `260312` | 2026-03-12   |
| `251225` | 2025-12-25   |

## Activity Codes

| Code | Activity | Fields | Example |
|------|----------|--------|---------|
| `S`  | Steps    | total_steps, goal | `S,260312,8500,10000` |
| `R`  | Run      | duration_min, distance_km, pace_min_per_km | `R,260312,32,5.1,6.3` |
| `W`  | Swim     | duration_min, distance_m, laps | `W,260312,45,1500,30` |
| `B`  | Bike     | duration_min, distance_km, avg_speed_kmh | `B,260312,60,25.3,25.3` |
| `G`  | Gym      | sessions | `G,260312,1` |
| `X`  | Stretch  | _(none — line presence means done)_ | `X,260312` |
| `K`  | Ski      | duration_min, runs | `K,260312,180,12` |
| `H`  | Hike     | duration_min, distance_km, elevation_m | `H,260312,95,8.4,650` |

### Code Mnemonics

- `S` = **S**teps
- `R` = **R**un
- `W` = s**W**im (`S` is taken)
- `B` = **B**ike
- `G` = **G**ym
- `X` = stretch/e**X**tend
- `K` = s**K**i
- `H` = **H**ike

## Field Descriptions

### Steps (`S`)
| Field | Type | Description |
|-------|------|-------------|
| total_steps | u32 | Total steps recorded that day |
| goal | u32 | Step goal for that day (e.g. 6000, 10000) |

### Run (`R`)
| Field | Type | Description |
|-------|------|-------------|
| duration_min | u16 | Duration in minutes |
| distance_km | f32 | Distance in kilometers |
| pace_min_per_km | f32 | Average pace in min/km |

### Swim (`W`)
| Field | Type | Description |
|-------|------|-------------|
| duration_min | u16 | Duration in minutes |
| distance_m | u32 | Distance in meters |
| laps | u16 | Number of laps |

### Bike (`B`)
| Field | Type | Description |
|-------|------|-------------|
| duration_min | u16 | Duration in minutes |
| distance_km | f32 | Distance in kilometers |
| avg_speed_kmh | f32 | Average speed in km/h |

### Gym (`G`)
| Field | Type | Description |
|-------|------|-------------|
| sessions | u8 | Number of gym sessions that day (1, 2, etc.) |

### Stretch (`X`)
No fields. The line's existence means stretching was done that day.

### Ski (`K`)
| Field | Type | Description |
|-------|------|-------------|
| duration_min | u16 | Duration in minutes |
| runs | u8 | Number of runs/descents |

### Hike (`H`)
| Field | Type | Description |
|-------|------|-------------|
| duration_min | u16 | Duration in minutes |
| distance_km | f32 | Distance in kilometers |
| elevation_m | u32 | Elevation gain in meters |

## Rules

- A day can have **multiple records** (e.g. steps + gym + stretch)
- Unknown activity codes are silently skipped by the parser
- Adding a new activity type requires only a new single-character code

## Example

```
# fit.log
S,260310,10200,10000
R,260310,44,7.1,6.2
G,260310,1
S,260311,7900,10000
S,260312,8500,10000
X,260312
G,260312,1
```

## Git Diff Behavior

Appending today's activities produces a clean diff:

```diff
 S,260311,7900,10000
+S,260312,8500,10000
+X,260312
+G,260312,1
```

## Size

~11 KB per year of typical daily usage (steps daily + 1–2 other activities on ~60% of days).
