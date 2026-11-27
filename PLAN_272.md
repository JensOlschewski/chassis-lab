# Volume constraints in `clab optimize`

Status: plan. Issue #272. CLI first, app later.

## Goal

Keep chosen points inside a packaging volume (HyperMesh tet mesh) during
`clab optimize`. Today this is a hand-written MATLAB check in
`beflex-mlta-optimisation/hardpoint_optimization/nonlcon_mks.m`. The user
enters the `.hmascii` file; the solver reads and converts it.

## Decisions

- **Mesh input:** `.hmascii` only (tet4, and corner nodes of tet10). No STL, no
  manual conversion step. The MATLAB side ports the logic of `hmascii2mat.m`.
- **Inside test:** `pointLocation` on `triangulation(tets, nodes)`. This is the
  current check. It is the fastest option measured (see Benchmark).
- **Constraint form:** `inside(volume, points...)` returns the depth in mm,
  positive inside, negative outside, minimum over all points and time steps.
  Used in the normal constraint syntax, `>= 0` means inside, `>= 2` means 2 mm
  margin.
- **Solver rule unchanged:** `c = rhs - lhs` for `>=`, met when `c <= 0`.
- **Continuous at the surface:** outside, `c` grows with the distance, so
  `sqp` gets a direction. Not a 0/1 step as in `nonlcon_mks.m` today.
- **No second parser in Rust.** `clab` checks names and file existence. The
  mesh content is checked by the solver.

## Sign convention

| depth | constraint | c | met |
|---|---|---|---|
| +5 | `>= 0` | -5 | yes |
| 0 (surface) | `>= 0` | 0 | yes (limit) |
| -3 | `>= 0` | +3 | no |
| +5 | `>= 2` | -3 | yes |
| +1 | `>= 2` | +1 | no |

## Input (example)

```toml
[volumes]
rl_volume = "rl-bc-volume.hmascii"
ru_volume = "ru-bc-volume.hmascii"

[constraints]
inside_rl = "inside(rl_volume, pwt.bodies.B3_Lower.points.RL.position) >= 0"
inside_ru = "inside(ru_volume, ru, ru_inner, ru_outer) >= 0"
```

## Benchmark (RL volume, 100 random points, 100 mm box)

| Method | Setup | 100 points | Agrees with tet |
|---|---|---|---|
| Tet mesh, `pointLocation` | load `.mat` 0.09 s | 0.0035 s | (reference) |
| STL, `inpolyhedron` | load STL 0.06 s | 0.13 s | 100/100 |
| STL, `alphaShape` + `inShape` | build 0.005 s | 0.0002 s | 28/100 (not usable) |

Not measured yet: parsing the `.hmascii` directly, and the distance to the
surface. The distance runs for every evaluation and every time step, so it is
the main cost risk.

## Steps

**0. Prototype in MATLAB (gate before any Rust)**
- Port `hmascii2mat.m` to read `*node(`, `*tetra4(` and tet10 corners from
  `rl-bc-volume.hmascii` and `ru-bc-volume.hmascii`.
- `insideDepth(TR, P)`: sign from `pointLocation`, size from the distance to
  the boundary triangles (`freeBoundary`), for 3×m points.
- Time: file read (once per run), and 100 points (per evaluation). Check
  inside/outside against the current `nonlcon_mks.m` result.
- Gate: if the distance is too slow, change the method (for example, cache
  the boundary per volume, or a `.mat` next to the file).

**1. Solver (`matlab/`)**
- `helperFunctions/readHmascii.m`, `helperFunctions/insideDepth.m`, tests in
  `matlab/tests/`. Errors for no nodes or no tets name the file.
- `mks_optimize_main.m`: read each volume once per run, store it in the struct
  `s` under its name. `evaluateProblem` already reads names from `s`.

**2. Problem file (`crates/clab-core/src/config/`)**
- `problem.rs`: `[volumes]` (name = `.hmascii` path, relative to the problem
  file). Unknown keys stay errors.
- `expr.rs`: add `inside` to the function list. First argument must be a known
  volume, at least one point. Volume names unique across models, signals and
  volumes, and not reserved.
- Checks: file exists, is readable, ends in `.hmascii`.

**3. CLI (`crates/clab-cli/src/optimize.rs`)**
- Pass volume paths (absolute) in the problem JSON.
- `--dry-run` prints the start value of each volume constraint.

**4. Tests and docs**
- Rust: parse, name clashes, unknown volume, missing file, `inside` without
  points.
- Solver: small tet mesh (inside, surface, outside), several points, a 3×n
  signal.
- End to end: an example where a point starts outside and ends inside.
- `docs/optimize-problem.md`: `[volumes]` and `inside`.

**5. Release**
- The solver change needs the mks-assets rebuilt on Linux, Windows and macOS
  (MATLAB R2024b), and `desktop.yml` pointed at the new tag.

## Open points

- **RU inner and outer points** (`RU ± d_RU/2`). Expressions cannot compute
  them. Likely: the model carries them as named points.
- **Whole travel or one instant.** A 3×n signal checks the whole travel.
  Indexing (`...position(mid_idx)`) checks one instant. Pick per constraint.
- **Mesh tet10:** confirm the models use corner nodes only in the volume files.
- **App:** only after the CLI works.


## Progress

- [x] 2. Problem file: `[volumes]`, `inside()` checks, tests (Rust)
- [x] 1. Solver: `readHmascii.m`, `inside.m`, `insideTest.m` (cube), wired in `mks_optimize_main.m` / `evaluateProblem.m`
- [ ] 0. Timing on real RL/RU files; verify `.hmascii` line layout (only a synthetic cube so far)
- [ ] 3. End-to-end example in `examples/matlab-examples/`
- [ ] 4. `docs/optimize-problem.md`
- [ ] 5. Release (mks-assets rebuild)
