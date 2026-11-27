function mks_optimize_main(problemJson, outJson, varargin)
% mks_solver --optimize <problem.json> <out.json> --design FILE [--dry-run]
%
% The problem JSON is written by clab from the problem TOML. --dry-run
% evaluates the start design once. Otherwise the design is optimized from the
% start design.
design = [];
dryRun = false;
k = 1;
while k <= numel(varargin)
    switch varargin{k}
        case '--design'
            design = readDesign(varargin{k+1});
            k = k + 2;
        case '--dry-run'
            dryRun = true;
            k = k + 1;
        otherwise
            error('mks:args', 'Unknown option %s', varargin{k});
    end
end
if isempty(design)
    error('mks:args', '--optimize needs --design with the start design');
end

problem = jsondecode(fileread(problemJson));
% each volume is read once per run, not per evaluation
problem.volumeData = struct();
if isfield(problem, 'volumes')
    for name = string(fieldnames(problem.volumes))'
        problem.volumeData.(name) = readHmascii(problem.volumes.(name), name);
    end
end
start = startDesign(problem, design);

% The start design is the reference: models that use design variables cannot
% be solved without one, and it sets the objective scales that are not given.
if dryRun
    reference = evaluateProblem(problem, design);
    result = scoreProblem(problem, reference, reference);
    result.design = start;
    result.start_design = start;
    result.start_J = result.J;
else
    result = run_mks_optimizer(problem, design, start);
end

fid = fopen(outJson, 'w');
fwrite(fid, jsonencode(result, 'PrettyPrint', true));
fclose(fid);
end

function start = startDesign(problem, design)
% The start value of every design variable, checked against its bounds and
% against the models that use it.
texts = cellfun(@fileread, struct2cell(problem.models), 'UniformOutput', false);
start = struct();
for k = 1:numel(problem.design)
    d = problem.design(k);
    if ~isKey(design, d.name)
        error('mks:design', 'Design variable %s has no start value in --design', d.name);
    end
    value = design(d.name);
    value = value{1};
    if ~isnumeric(value) || ~isscalar(value)
        error('mks:design', 'Design variable %s must be one number', d.name);
    end
    if value < d.lower || value > d.upper
        error('mks:design', 'Start value %g of %s is outside [%g, %g]', value, d.name, d.lower, d.upper);
    end
    pattern = "\<" + regexptranslate('escape', d.name) + "\>";
    if ~any(cellfun(@(text) ~isempty(regexp(text, pattern, 'once')), texts))
        error('mks:design', 'Design variable %s is not used by any model', d.name);
    end
    start.(d.name) = value;
end
end
