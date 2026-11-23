function result = run_mks_optimizer(problem, design, start)
    % run_mks_optimizer minimizes J = 10 - grade over the design variables of the
    % problem with fmincon (sqp).
    %
    %   problem  decoded problem JSON
    %   design   dictionary of the --design file; its other variables stay fixed
    %   start    struct with the start value of each design variable
    %
    % fmincon works on variables scaled to [0, 1]. The start design is the
    % reference of the goals. A design where a model fails to solve or a signal is
    % not a finite number counts as NaN for J and Inf for the constraints, so sqp
    % steps back from it. MKS_PROGRESS=1 shows the fmincon iterations.
    progress = getenv('MKS_PROGRESS') == "1";
    setenv('MKS_PROGRESS', '0');    % the solver's own step table would drown the iterations
    names = string({problem.design.name});
    lower = [problem.design.lower]';
    span = [problem.design.upper]' - lower;
    x0 = (cellfun(@(n) start.(n), cellstr(names))' - lower) ./ span;

    reference = evaluateProblem(problem, design);
    startGrade = gradeProblem(problem, reference, reference).grade;

    cache = struct('x', [], 'result', []);
    failures = struct('count', 0, 'message', '');
    nConstraints = numel(problem.constraints);

    options = optimoptions('fmincon', 'Algorithm', 'sqp', ...
        'Display', ternary(progress, 'iter', 'off'), ...
        'FiniteDifferenceType', 'forward', 'FiniteDifferenceStepSize', 1e-4);
    if isfield(problem, 'max_iterations')
        options.MaxIterations = problem.max_iterations;
    end
    if isfield(problem, 'max_evaluations')
        options.MaxFunctionEvaluations = problem.max_evaluations;
    end

    [x, ~, exitflag, output] = fmincon(@objective, x0, [], [], [], [], zeros(size(x0)), ones(size(x0)), @constraints, options);

    final = evaluate(x);
    if isempty(final)
        error('mks:optimize', 'The optimizer returned a design that cannot be evaluated: %s', failures.message);
    end
    result = final;
    result.design = physical(x);
    result.start_design = start;
    result.start_grade = startGrade;
    result.fmincon = struct('exitflag', exitflag, 'message', output.message, ...
        'iterations', output.iterations, 'evaluations', output.funcCount, ...
        'failed_evaluations', failures.count, 'last_error', failures.message);

    function value = objective(x)
        r = evaluate(x);
        value = NaN;
        if ~isempty(r)
            value = r.J;
        end
    end

    function [c, ceq] = constraints(x)
        r = evaluate(x);
        c = Inf(nConstraints, 1);
        if ~isempty(r)
            c = r.c;
        end
        ceq = [];
    end

    function r = evaluate(x)
        % Objective and constraints share one evaluation of the same design.
        if isequal(x, cache.x)
            r = cache.result;
            return
        end
        try
            r = gradeProblem(problem, evaluateProblem(problem, dictionary(x)), reference);
        catch err
            failures.count = failures.count + 1;
            failures.message = err.message;
            r = [];
        end
        cache = struct('x', x, 'result', r);
    end

    function values = physical(x)
        values = struct();
        for k = 1:numel(names)
            values.(names(k)) = lower(k) + x(k) * span(k);
        end
    end

    function d = dictionary(x)
        d = design;
        values = physical(x);
        for k = 1:numel(names)
            d(names(k)) = {values.(names(k))};
        end
    end
end

function value = ternary(condition, a, b)
    if condition
        value = a;
    else
        value = b;
    end
end
