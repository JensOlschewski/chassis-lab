function result = scoreProblem(problem, values, reference)
% scoreProblem weighs the evaluated objectives and measures the constraints.
%
% Each objective counts as value/scale, with its given scale or else |value|
% of the reference (start) design, so equal weights count the same at the
% start. The optimizer minimizes J = sum(w.*value./scale)/sum(w). A constraint
% is passed to fmincon unscaled: c = lhs - rhs for <=, rhs - lhs for >=, so
% c <= 0 holds when it is met.
n = numel(problem.objectives);
objectives = cell(n, 1);
weights = zeros(n, 1);
losses = zeros(n, 1);
for k = 1:n
    o = problem.objectives(k);
    scale = o.scale;
    if isempty(scale)
        scale = abs(reference.objectives(k));
        if scale < 1e-9
            error('mks:problem', ['Objective %s is about 0 for the start design, ' ...
                'so it cannot set the scale: give scale in [objectives.%s]'], o.name, o.name);
        end
    end
    weights(k) = o.weight;
    losses(k) = values.objectives(k) / scale;
    objectives{k} = struct('name', o.name, 'weight', o.weight, 'scale', scale, ...
        'start', reference.objectives(k), 'value', values.objectives(k), 'loss', losses(k));
end
result.objectives = vertcat(objectives{:});
result.J = sum(weights .* losses) / sum(weights);

m = numel(problem.constraints);
constraints = cell(m, 1);
result.c = zeros(m, 1);
for k = 1:m
    c = problem.constraints(k);
    if c.relation == "<="
        margin = values.rhs(k) - values.lhs(k);
    else
        margin = values.lhs(k) - values.rhs(k);
    end
    result.c(k) = -margin;
    constraints{k} = struct('name', c.name, 'relation', c.relation, 'value', values.lhs(k), ...
        'limit', values.rhs(k), 'margin', margin, 'ok', margin >= 0);
end
result.constraints = vertcat(constraints{:});
end
