function result = gradeProblem(problem, values, reference)
% gradeProblem grades the evaluated objectives against the reference design and
% measures the constraints.
%
% The total grade is sum(w.*grade)/sum(w) and the optimizer minimizes
% J = 10 - grade. A constraint is scaled by |rhs|, or by 1 when that is about 0,
% so c <= 0 holds when it is met. Constraints are not graded.
n = numel(problem.objectives);
objectives = cell(n, 1);
weights = zeros(n, 1);
grades = zeros(n, 1);
for k = 1:n
    o = problem.objectives(k);
    g = gradeObjective(o.kind, o.limits, values.objectives(k), reference.objectives(k), o.deduction_factor);
    objectives{k} = struct('name', o.name, 'kind', o.kind, 'weight', o.weight, ...
        'deduction_factor', o.deduction_factor, 'reference', reference.objectives(k), ...
        'value', values.objectives(k), 'scale', g.scale, 'rule', g.rule, 'd', g.d, 'grade', g.grade);
    weights(k) = o.weight;
    grades(k) = g.grade;
end
result.objectives = vertcat(objectives{:});
result.grade = sum(weights .* grades) / sum(weights);
result.J = 10 - result.grade;

m = numel(problem.constraints);
constraints = cell(m, 1);
result.c = zeros(m, 1);
for k = 1:m
    c = problem.constraints(k);
    scale = abs(values.rhs(k));
    if scale < 1e-9
        scale = 1;
    end
    if c.relation == "<="
        margin = values.rhs(k) - values.lhs(k);
    else
        margin = values.lhs(k) - values.rhs(k);
    end
    result.c(k) = -margin / scale;
    constraints{k} = struct('name', c.name, 'relation', c.relation, 'value', values.lhs(k), ...
        'limit', values.rhs(k), 'margin', margin, 'ok', margin >= 0);
end
result.constraints = vertcat(constraints{:});
end
