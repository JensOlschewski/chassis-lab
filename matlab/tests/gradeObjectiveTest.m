% Run from matlab/: matlab -batch "addpath(genpath('.')); gradeObjectiveTest"
near = @(a, b) abs(a - b) < 1e-12;

g = gradeObjective('lower', 10, 12, 0, 20);
assert(g.d == 0 && g.grade == 10 && g.rule == "requirement");

g = gradeObjective('lower', 10, 9, 0, 20);
assert(near(g.d, 0.1) && near(g.grade, 8) && g.scale == 10);

g = gradeObjective('upper', -4, -3, 0, 20);
assert(near(g.d, 0.25) && g.scale == 4);

g = gradeObjective('range', [20 30], 35, 0, 20);
assert(near(g.d, 5 / 30));
g = gradeObjective('range', [20 30], 18, 0, 20);
assert(near(g.d, 2 / 20));
g = gradeObjective('range', [20 30], 25, 0, 20);
assert(g.d == 0);

g = gradeObjective('target', -20, -18, 0, 20);
assert(near(g.d, 0.1) && near(g.grade, 8));

% goals: better than the reference is negative d and a grade above 10
g = gradeObjective('minimize', [], 8, 10, 20);
assert(near(g.d, -0.2) && near(g.grade, 14) && g.rule == "reference");
g = gradeObjective('maximize', [], 8, 10, 20);
assert(near(g.d, 0.2));
g = gradeObjective('maximize', [], -8, -10, 20);
assert(near(g.d, -0.2) && g.scale == 10);

% scale fallbacks: bound, then reference, then 1
g = gradeObjective('target', 0, 0.5, 2, 20);
assert(g.rule == "reference" && g.scale == 2 && near(g.d, 0.25));
g = gradeObjective('target', 0, 0.5, 0, 20);
assert(g.rule == "unit" && g.scale == 1 && near(g.d, 0.5));
g = gradeObjective('minimize', [], 0.5, 0, 20);
assert(g.rule == "unit" && near(g.d, 0.5));

% total grade and constraints
problem.objectives = [ ...
    struct('name', 'a', 'kind', 'lower', 'limits', 10, 'weight', 1, 'deduction_factor', 20); ...
    struct('name', 'b', 'kind', 'minimize', 'limits', [], 'weight', 3, 'deduction_factor', 20)];
problem.constraints = [ ...
    struct('name', 'c1', 'lhs', '', 'relation', '<=', 'rhs', '')
    struct('name', 'c2', 'lhs', '', 'relation', '>=', 'rhs', '')];
reference = struct('objectives', [10; 5], 'lhs', [0; 0], 'rhs', [0; 0]);
values = struct('objectives', [9; 4], 'lhs', [3; 3], 'rhs', [4; 2]);
r = gradeProblem(problem, values, reference);
assert(near(r.grade, (1 * 8 + 3 * 14) / 4) && near(r.J, 10 - r.grade));
assert(near(r.c(1), -0.25) && near(r.c(2), -0.5));
assert(r.constraints(1).ok && r.constraints(2).ok && r.constraints(1).margin == 1);
values.lhs = [5; 1];
r = gradeProblem(problem, values, reference);
assert(~r.constraints(1).ok && ~r.constraints(2).ok && r.c(1) > 0 && r.c(2) > 0);

disp('gradeObjectiveTest passed');
