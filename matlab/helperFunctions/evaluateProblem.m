function values = evaluateProblem(problem, designDic)
% evaluateProblem solves every model of the problem for a design and evaluates
% the signals, objectives and constraint sides.
%
% Each model gets only the design variables its JSON mentions, so the solver
% does not warn about the others. The signal expressions are the rewritten
% ones of the problem file and see the struct s: s.<model> is a solution and
% s.<signal> an earlier signal.
%
% values.objectives  nx1 objective values
% values.lhs, rhs    mx1 constraint sides
s = struct();
if isfield(problem, 'volumeData')
    s = problem.volumeData;
end
for name = string(fieldnames(problem.models))'
    path = problem.models.(name);
    text = fileread(path);
    own = configureDictionary("string", "cell");
    for variable = keys(designDic)'
        if ~isempty(regexp(text, "\<" + regexptranslate('escape', variable) + "\>", 'once'))
            own(variable) = designDic(variable);
        end
    end
    s.(name) = run_mks_solver(path, own);
end

for k = 1:numel(problem.signals)
    s.(problem.signals(k).name) = evaluate(problem.signals(k).expr, s);
end

values.objectives = zeros(numel(problem.objectives), 1);
for k = 1:numel(problem.objectives)
    values.objectives(k) = scalar(evaluate(problem.objectives(k).expr, s), problem.objectives(k).name);
end

values.lhs = zeros(numel(problem.constraints), 1);
values.rhs = zeros(numel(problem.constraints), 1);
for k = 1:numel(problem.constraints)
    name = problem.constraints(k).name;
    values.lhs(k) = scalar(evaluate(problem.constraints(k).lhs, s), name);
    values.rhs(k) = scalar(evaluate(problem.constraints(k).rhs, s), name);
end
end

function value = evaluate(expression, s)
value = feval(str2func("@(s) " + expression), s);
end

function value = scalar(value, name)
if ~isnumeric(value) || ~isscalar(value) || ~isreal(value) || ~isfinite(value)
    error('mks:signal', '%s must evaluate to one finite number', name);
end
end
