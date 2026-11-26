# Optimization problem file (`problem.toml`)

Reference for the TOML file read by `clab optimize`. See the README for the
command line flags. A worked example is
`examples/matlab-examples/mlta-optimize-problem.toml`.

The file defines which models to solve, which design variables may change,
what to minimize and which limits must hold. `clab` checks the file (syntax,
names, cycles, body/joint/point names against the model JSON), then hands it to
the MATLAB solver (`mks_solver --optimize`), which runs `fmincon` (`sqp`).

Unknown keys are errors.

## Overview

```toml
[models]
pwt = "model.json"            # alias = path (relative to this file)

[optimizer]                   # optional
max_iterations = 100
max_evaluations = 2000

[design]
HP_L_Z = [426.38, 446.38]     # variable = [lower, upper]

[signals]                     # optional named intermediate values
v_wc = "pwt.characteristics.wheel_velocities.l_wc"
epsilon = "rad2deg(atan(v_wc.x ./ v_wc.z))"

[objectives.epsilon_design]   # at least one
value = "-epsilon(round(numel(epsilon)/2))"
weight = 1.0                  # optional, > 0, default 1
scale = 10.0                  # optional, > 0

[constraints]                 # optional
min_epsilon = "epsilon(1) >= 10"
max_epsilon = "epsilon(1) <= 15"
```

## Sections

### `[models]` (required)

`alias = "path/to/model.json"`. At least one model. Relative paths are resolved
from the problem file's directory. Every model is solved for each candidate
design, and the alias becomes a name usable in expressions.

### `[optimizer]` (optional)

| Key | Meaning |
|---|---|
| `max_iterations` | `fmincon` iteration limit. Positive integer. |
| `max_evaluations` | `fmincon` function evaluation limit. Positive integer. |

Without a key the `fmincon` default applies.

### `[design]` (required)

`VARIABLE = [lower, upper]`. At least one. Bounds must be finite with
`lower < upper`. The variable name is the design variable used in the model JSON.
Each model only receives the variables its JSON mentions.

The start value comes from `--design FILE`. Without it, the start is the
midpoint of the bounds (`clab` asks for confirmation unless `--yes` is given).
Variables in the `--design` file that are not in `[design]` stay fixed.

### `[signals]` (optional)

`name = "expression"`. A signal is a named intermediate value computed from
model results and other signals. Signals may use each other in any order;
`clab` sorts them. Cycles are an error.

### `[objectives.<name>]` (at least one)

| Key | Meaning |
|---|---|
| `value` | Expression that evaluates to **one finite number**. Minimized. |
| `weight` | Positive number, default `1`. |
| `scale` | Positive number. Without it the solver uses \|value\| at the start design. |

The optimizer minimizes `J = sum(weight * value / scale) / sum(weight)`.
To maximize a quantity, negate it (`value = "-x"`). If an objective is about 0
at the start design and has no `scale`, the solver stops and asks for one.

### `[constraints]` (optional)

`name = "lhs <= rhs"` or `name = "lhs >= rhs"`. Exactly one `<=` or `>=` per
constraint, and both sides must evaluate to one finite number. The solver
(`fmincon` `nonlcon`) gets `c = lhs - rhs` for `<=` and `c = rhs - lhs` for
`>=`, unscaled, and requires `c <= 0`. The limit is met when `c <= 0`.

## Expressions

Expressions are a restricted MATLAB subset. They are checked by `clab` and
evaluated by the solver.

**Names.** Model aliases, signal names, and constants. Names for models and
signals use letters, digits and `_`, start with a letter, max 63 characters,
and must be unique across models and signals. Design variables are not usable
as names in expressions.

**Reserved** (cannot be model or signal names): `x y z end s xyz`, all function
names, `pi`.

**Constant:** `pi`.

**Operators:** `+  -  *  /  ^  .*  ./  .^`, unary `-`/`+`, parentheses.

**Functions:**

| Function | Notes |
|---|---|
| `abs sqrt atan atan2` | |
| `rad2deg deg2rad` | |
| `round numel` | |
| `max min mean` | |
| `gradient` | |
| `cross dot vecnorm` | for 3-vectors |
| `interp1` | |

**Fields:** `a.b.c` reads a field of a model or struct signal. The components
`.x`, `.y`, `.z` select a row of a 3×n vector signal (for example
`v.x`, `pwt.characteristics.wheel_velocities.l_wc.z`).

**Indexing:** `name(i)` or `name.field(i)` indexes along the sample axis
(time steps). Only **one** index is allowed. Ranges `a:b` and `end` work only
inside an index, e.g. `eps(2:end)`, `eps(round(end/2))`. Only a model or signal
(optionally followed by fields) can be indexed, not an intermediate result.
Bodies, joints and points are addressed by name, never by index.

**Not allowed:** strings, function handles (`@`), array literals (`[]`, `{}`),
assignments, `;`, `~ & |`, comparisons inside an expression, and a bare `:`.

## Model results

A model alias evaluates to the solution of `run_mks_solver`. Each signal is
a value over the *n* simulation steps (3×n for vectors).

| Path | Content |
|---|---|
| `m.time` | Time, 1×n. |
| `m.bodies.<Body>.velocity.{linear,angular}` | Body velocity, 3×n. |
| `m.bodies.<Body>.cm.position` | Center of mass, 3×n. |
| `m.bodies.<Body>.points.<Point>.position` | Point position, 3×n. |
| `m.bodies.<Body>.points.<Point>.velocity.{linear,angular}` | Point velocity, 3×n. |
| `m.joints.<Joint>.{type,group,bushing}` | Joint properties. |
| `m.joints.<Joint>.{i,j}.{body,point,position}` | Joint sides. |
| `m.joints.<Joint>.{i,j}.velocity.{linear,angular}` | Side velocity. |
| `m.joints.<Joint>.{i,j}.reaction.{force,moment}` | Reaction incl. bushing force. |
| `m.joints.<Joint>.{i,j}.reaction_gradient.{force,moment}` | Elastokinematic mode only. |
| `m.characteristics.*` | Suspension characteristics, e.g. `wheel_velocities.l_wc`, `wheel_points.l_wp`, `wheel_points.l_wc`. |

`clab` checks body, joint and point names against the model JSON, so a typo
fails before the solver starts. Field names after those are checked by the
solver at run time. The full list of `characteristics` fields is defined by the
solver (`createCharStruct`/`run_mks_solver`); see the example problem for the ones
in use.

## Evaluation and failures

- Each candidate design solves every model, then signals, objectives and
  constraint sides are evaluated.
- A design where a model fails to solve, or an expression is not one finite
  number, counts as failed. The optimizer steps back from it. The failure count
  and the last error are reported in the result.
- `--dry-run` evaluates only the start design.
- `--progress` shows the `fmincon` iterations.

## Example

```toml
[models]
pwt = "mlta-optimize-model-file-1e2.json"

[optimizer]
max_iterations = 100

[design]
HP_L_Z = [426.38, 446.38]

[signals]
v_wc      = "pwt.characteristics.wheel_velocities.l_wc"
r_wcwp    = "pwt.characteristics.wheel_points.l_wp - pwt.characteristics.wheel_points.l_wc"
omega_wca = "pwt.bodies.B2_Wheelcarrier.velocity.angular"
v_wp_proj = "cross(omega_wca, r_wcwp) + pwt.characteristics.wheel_velocities.l_wc"
epsilon     = "rad2deg(atan(v_wc.x ./ v_wc.z))"
epsilon_ast = "rad2deg(atan(v_wp_proj.x ./ v_wp_proj.z))"
mid_idx   = "round(numel(epsilon)/2)"
first_idx = "1"

[objectives.epsilon_design]
value = "-epsilon(mid_idx)"

[objectives.epsilon_ast_design]
value = "-epsilon_ast(mid_idx)"

[constraints]
min_epsilon     = "epsilon(first_idx) >= 10"
max_epsilon     = "epsilon(first_idx) <= 15"
min_epsilon_ast = "epsilon_ast(first_idx) >= 8"
max_epsilon_ast = "epsilon_ast(first_idx) <= 15"
```

Here `epsilon` is a 1×n signal, `mid_idx` and `first_idx` are scalar signals
used as indices, and both objectives maximize their value at the middle sample
(by negating it) while the constraints bound the first sample.
