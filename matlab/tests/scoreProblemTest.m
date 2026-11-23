% Run from matlab/: matlab -batch "addpath(genpath('.')); scoreProblemTest"
near = @(a, b) abs(a - b) < 1e-12;

% scale from the start value, or the given one
problem.objectives = [ ...
    struct('name', 'a', 'weight', 1, 'scale', []); ...
    struct('name', 'b', 'weight', 3, 'scale', 2)];
problem.constraints = [ ...
    struct('name', 'c1', 'lhs', '', 'relation', '<=', 'rhs', '')
    struct('name', 'c2', 'lhs', '', 'relation', '>=', 'rhs', '')];
reference = struct('objectives', [-10; 4], 'lhs', [0; 0], 'rhs', [0; 0]);
values = struct('objectives', [-12; 3], 'lhs', [3; 3], 'rhs', [4; 2]);
r = scoreProblem(problem, values, reference);
assert(r.objectives(1).scale == 10 && near(r.objectives(1).loss, -1.2));
assert(r.objectives(2).scale == 2 && near(r.objectives(2).loss, 1.5));
assert(near(r.J, (1 * -1.2 + 3 * 1.5) / 4));
assert(r.objectives(1).start == -10 && r.objectives(1).value == -12);

% equal weights count the same at the start
problem.objectives(2).scale = [];
r = scoreProblem(problem, reference, reference);
assert(near(r.objectives(1).loss, -1) && near(r.objectives(2).loss, 1));

% a start value of about 0 needs a given scale
reference.objectives(1) = 0;
try
    scoreProblem(problem, values, reference);
    error('expected an error');
catch err
    assert(contains(err.message, 'give scale'));
end

% constraints
problem.objectives(1).scale = 1;
r = scoreProblem(problem, values, reference);
assert(near(r.c(1), -0.25) && near(r.c(2), -0.5));
assert(r.constraints(1).ok && r.constraints(2).ok && r.constraints(1).margin == 1);
values.lhs = [5; 1];
r = scoreProblem(problem, values, reference);
assert(~r.constraints(1).ok && ~r.constraints(2).ok && r.c(1) > 0 && r.c(2) > 0);

disp('scoreProblemTest passed');
